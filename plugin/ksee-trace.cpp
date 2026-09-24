// kSEE trace plugin: probes that report kernel execution to the host.
//
//   KSEE_TRACE=PATTERN[:MODES],...   what to instrument
//   KSEE_TYPES=struct.field,...      struct fields whose layout to publish
//
//   KSEE_CURRENT=symbol              variable holding the current task
//   KSEE_RING_WORDS=n                trace ring size (power of two)
//   KSEE_RING_HOME=symbol            define the ring and its writers in the
//                                    object that defines this (start_kernel)
//
// PATTERN is a glob over function names or, if it contains '/', over
// source file paths. MODES is a '+'-separated subset of
//   enter  at function entry: the time, the current task and the first
//          four integer/pointer arguments (truncated to 32 bits)
//   exit   before each return: the time, the current task and the value
//   block  at the start of each basic block: the block's index
// and defaults to "enter". Enter and exit probes are inserted before
// optimization, so they survive inlining. Block probes are inserted after
// it, so they describe the code that actually runs (and cannot keep the
// optimizer from removing code the kernel relies on being removed); a
// block's location is that of its first instruction, often in an inlined
// helper. The sequence of blocks is the branch history.
//
// Probes append records to a fixed ring in the module's own memory,
// __ksee_trace = { u32 pos; u32 ring[KSEE_RING_WORDS]; }, where pos counts
// words ever written (the ring keeps the most recent ones); the exported
// function ksee_trace() returns its address. pos is stored with release
// order after each record, so a reader that loads it sees whole records.
// The writer assumes one writer at a time (a uniprocessor kernel).
//
// A record is a head pair of words followed by continuation pairs; the
// second word's top nibble tags the pair, so a reader can start at any even
// offset (the ring may have overwritten the start of a record) and skip
// continuations until the next head:
//   [fn, 1 << 28 | n]     block n of fn
//   [fn, 2 << 28]         enter fn, followed by time lo, time hi, current,
//                         a0, a1, a2, a3
//   [fn, 3 << 28]         exit fn, followed by time lo, time hi, current,
//                         value
//   [value, 8 << 28 | i]  continuation i of the preceding head
// fn is a hash of the function name; time comes from the ksee.now import.
//
// The linked module describes its probes in two wasm custom sections, which
// wasm-ld concatenates across objects (lines may repeat):
//   ksee.probes  R \t ring words
//                F \t fn \t name \t file:line \t argument types
//                B \t fn \t n \t file:line:column
//   ksee.types   struct \t field \t byte offset \t byte size
// Types and source locations come from debug info, so compile with -g.
#include "llvm/BinaryFormat/Dwarf.h"
#include "llvm/IR/DebugInfo.h"
#include "llvm/IR/DebugInfoMetadata.h"
#include "llvm/IR/IRBuilder.h"
#include "llvm/IR/Module.h"
#include "llvm/IR/PassManager.h"
#include "llvm/Passes/PassBuilder.h"
#include "llvm/Plugins/PassPlugin.h"
#include "llvm/Support/GlobPattern.h"

#include <cstdlib>
#include <set>
#include <string>
#include <vector>

using namespace llvm;

namespace {

enum Mode { Enter = 1, Exit = 2, Block = 4 };

struct Rule {
  GlobPattern pattern;
  bool byFile;
  unsigned modes;
};

std::vector<StringRef> envList(const char *var) {
  std::vector<StringRef> items;
  const char *env = std::getenv(var);
  SmallVector<StringRef> parts;
  StringRef(env ? env : "").split(parts, ',', -1, false);
  for (StringRef p : parts)
    if (!p.trim().empty())
      items.push_back(p.trim());
  return items;
}

std::vector<Rule> parseRules() {
  std::vector<Rule> rules;
  for (StringRef item : envList("KSEE_TRACE")) {
    auto [pat, modeList] = item.split(':');
    unsigned modes = 0;
    SmallVector<StringRef> names;
    modeList.split(names, '+', -1, false);
    for (StringRef m : names) {
      if (m == "enter")
        modes |= Enter;
      else if (m == "exit")
        modes |= Exit;
      else if (m == "block")
        modes |= Block;
      else
        report_fatal_error("KSEE_TRACE: unknown mode '" + m + "'");
    }
    auto glob = GlobPattern::create(pat);
    if (!glob)
      report_fatal_error("KSEE_TRACE: bad pattern '" + pat + "'");
    rules.push_back({std::move(*glob), pat.contains('/'), modes ? modes : Enter});
  }
  return rules;
}

uint32_t fnId(StringRef name) {
  uint32_t h = 2166136261u; // FNV-1a
  for (unsigned char c : name)
    h = (h ^ c) * 16777619u;
  return h;
}

// "task_struct*", "u32", ...: the type as far as the host needs to know.
std::string typeName(const DIType *T) {
  while (auto *D = dyn_cast_or_null<DIDerivedType>(T)) {
    if (D->getTag() == dwarf::DW_TAG_pointer_type)
      return typeName(D->getBaseType()) + "*";
    if (D->getTag() == dwarf::DW_TAG_typedef &&
        (isa_and_nonnull<DIBasicType>(D->getBaseType()) || !D->getBaseType()))
      return D->getName().str();
    T = D->getBaseType(); // const, volatile, typedefs of structs, ...
  }
  return T && !T->getName().empty() ? T->getName().str() : "?";
}

std::string sourcePath(const DIScope *S) {
  return (S->getDirectory() + "/" + S->getFilename()).str();
}

// Wasm module asm can emit a custom section (a data global cannot).
void emitCustomSection(Module &M, StringRef name, StringRef text) {
  std::string s = (".section .custom_section." + name + ",\"\",@\n.ascii \"").str();
  for (char c : text) {
    if (c == '\n')
      s += "\\n";
    else if (c == '\t')
      s += "\\t";
    else if (c == '"' || c == '\\')
      s += std::string("\\") + c;
    else
      s += c;
  }
  M.appendModuleInlineAsm(s + "\"\n.text\n");
}

enum Kind { KindBlock = 1, KindEnter = 2, KindExit = 3, KindValue = 8 };

unsigned ringWords() {
  const char *env = std::getenv("KSEE_RING_WORDS");
  unsigned n = env ? std::strtoul(env, nullptr, 0) : 1u << 20;
  if (!n || (n & (n - 1)))
    report_fatal_error("KSEE_RING_WORDS must be a power of two");
  return n;
}

class KseeTrace : public PassInfoMixin<KseeTrace> {
  const std::vector<Rule> &rules;
  bool late;
  Module *M = nullptr;
  Type *i32 = nullptr, *i64 = nullptr;
  Function *putBlock = nullptr, *putEnter = nullptr, *putExit = nullptr;
  std::string probes;

  unsigned modesFor(Function &F) {
    unsigned modes = 0;
    DISubprogram *SP = F.getSubprogram();
    for (const Rule &R : rules)
      if (R.byFile ? SP && R.pattern.match(sourcePath(SP))
                   : R.pattern.match(F.getName()))
        modes |= R.modes;
    return modes;
  }

  // The runtime is defined in one object only (wasm-ld keeps every copy of
  // weak or COMDAT data), the one that defines $KSEE_RING_HOME.
  static bool isHome(Module &M) {
    const char *env = std::getenv("KSEE_RING_HOME");
    GlobalValue *home = M.getNamedValue(env ? env : "start_kernel");
    return home && !home->isDeclaration();
  }

  template <typename T> T *shared(T *GV) {
    GV->setVisibility(GlobalValue::HiddenVisibility);
    return GV;
  }

  Function *writerDecl(StringRef name, unsigned nargs) {
    SmallVector<Type *> params(nargs, i32);
    return cast<Function>(M->getOrInsertFunction(
        name, FunctionType::get(Type::getVoidTy(M->getContext()), params, false)).getCallee());
  }

  // The per-kind writers, declared (or, in the home object, defined along
  // with the ring and the export that finds it).
  void createRuntime() {
    if (putBlock)
      return;
    if (!isHome(*M) || M->getFunction("__ksee_put_block")) {
      putBlock = writerDecl("__ksee_put_block", 2);
      putEnter = writerDecl("__ksee_put_enter", 5);
      putExit = writerDecl("__ksee_put_exit", 2);
      return;
    }
    LLVMContext &C = M->getContext();
    unsigned words = ringWords();
    auto *traceTy = StructType::get(C, {i32, ArrayType::get(i32, words)});
    auto *trace = shared(new GlobalVariable(
        *M, traceTy, false, GlobalValue::ExternalLinkage,
        ConstantAggregateZero::get(traceTy), "__ksee_trace"));
    trace->setAlignment(Align(8));

    auto *infoTy = FunctionType::get(i32, false);
    Function *info = shared(Function::Create(infoTy, GlobalValue::ExternalLinkage,
                                             "__ksee_trace_info", M));
    info->addFnAttr("wasm-export-name", "ksee_trace");
    IRBuilder<> IB(BasicBlock::Create(C, "", info));
    IB.CreateRet(IB.CreatePtrToInt(trace, i32));

    FunctionCallee now = M->getOrInsertFunction("__ksee_now", FunctionType::get(i64, false));
    cast<Function>(now.getCallee())->addFnAttr("wasm-import-module", "ksee");
    cast<Function>(now.getCallee())->addFnAttr("wasm-import-name", "now");

    Value *current = nullptr;
    if (const char *sym = std::getenv("KSEE_CURRENT"))
      current = M->getOrInsertGlobal(sym, PointerType::getUnqual(C));

    auto writer = [&](StringRef name, unsigned nargs, auto body) {
      SmallVector<Type *> params(nargs, i32);
      Function *F = shared(Function::Create(FunctionType::get(Type::getVoidTy(C), params, false),
                                            GlobalValue::ExternalLinkage, name, M));
      F->addFnAttr(Attribute::NoInline);
      IRBuilder<> B(BasicBlock::Create(C, "", F));
      SmallVector<Value *> words;
      body(B, F, words);
      Value *pos = B.CreateStructGEP(traceTy, trace, 0);
      Value *ring = B.CreateStructGEP(traceTy, trace, 1);
      Value *p = B.CreateLoad(i32, pos);
      for (auto [i, w] : enumerate(words)) {
        Value *idx = B.CreateAnd(B.CreateAdd(p, B.getInt32(i)), B.getInt32(ringWords() - 1));
        B.CreateStore(w, B.CreateGEP(i32, ring, idx));
      }
      B.CreateAlignedStore(B.CreateAdd(p, B.getInt32(words.size())), pos, Align(4))
          ->setAtomic(AtomicOrdering::Release);
      B.CreateRetVoid();
      return F;
    };
    // Continuation pairs: [value, KindValue << 28 | index].
    auto value = [&](IRBuilder<> &B, SmallVector<Value *> &words, Value *v) {
      unsigned index = words.size() / 2 - 1;
      words.push_back(v);
      words.push_back(B.getInt32(KindValue << 28 | index));
    };
    auto stamp = [&](IRBuilder<> &B, SmallVector<Value *> &words) {
      Value *t = B.CreateCall(now);
      value(B, words, B.CreateTrunc(t, i32));
      value(B, words, B.CreateTrunc(B.CreateLShr(t, 32), i32));
      value(B, words, current ? B.CreatePtrToInt(B.CreateLoad(PointerType::getUnqual(C), current), i32)
                              : B.getInt32(0));
    };
    putBlock = writer("__ksee_put_block", 2, [&](IRBuilder<> &B, Function *F, auto &words) {
      words.push_back(F->getArg(0));
      words.push_back(B.CreateOr(F->getArg(1), B.getInt32(KindBlock << 28)));
    });
    putEnter = writer("__ksee_put_enter", 5, [&](IRBuilder<> &B, Function *F, auto &words) {
      words.push_back(F->getArg(0));
      words.push_back(B.getInt32(KindEnter << 28));
      stamp(B, words);
      for (unsigned i = 1; i < 5; i++)
        value(B, words, F->getArg(i));
    });
    putExit = writer("__ksee_put_exit", 2, [&](IRBuilder<> &B, Function *F, auto &words) {
      words.push_back(F->getArg(0));
      words.push_back(B.getInt32(KindExit << 28));
      stamp(B, words);
      value(B, words, F->getArg(1));
    });
    probes += "R\t" + std::to_string(words) + "\n";
  }

  static std::string location(const DILocation *L) {
    return (sourcePath(L->getScope()) + ":" + Twine(L->getLine()) + ":" +
            Twine(L->getColumn())).str();
  }

  Value *toI32(IRBuilder<> &B, Value *V) {
    Type *T = V->getType();
    if (T->isPointerTy())
      return B.CreatePtrToInt(V, i32);
    if (T->isIntegerTy())
      return B.CreateZExtOrTrunc(V, i32);
    return B.getInt32(0);
  }

  void instrument(Function &F, unsigned modes) {
    createRuntime();
    uint32_t id = fnId(F.getName());
    DISubprogram *SP = F.getSubprogram();

    SmallVector<BasicBlock *> blocks;
    for (BasicBlock &BB : F)
      blocks.push_back(&BB);

    if (modes & Block)
      for (auto [n, BB] : enumerate(blocks)) {
        IRBuilder<> B(&*BB->getFirstInsertionPt());
        B.CreateCall(putBlock, {B.getInt32(id), B.getInt32(n)});
        std::string loc = "?";
        for (Instruction &I : *BB)
          if (DILocation *L = I.getDebugLoc().get(); L && L->getLine()) {
            loc = location(L);
            break;
          }
        probes += ("B\t" + Twine(id) + "\t" + Twine(n) + "\t" + loc + "\n").str();
      }

    if (modes & Exit)
      for (BasicBlock *BB : blocks)
        if (auto *R = dyn_cast<ReturnInst>(BB->getTerminator())) {
          IRBuilder<> B(R);
          Value *ret = R->getReturnValue();
          B.CreateCall(putExit, {B.getInt32(id), ret ? toI32(B, ret) : B.getInt32(0)});
        }

    if (modes & Enter) {
      IRBuilder<> B(&*F.getEntryBlock().getFirstInsertionPt());
      SmallVector<Value *, 5> args{B.getInt32(id)};
      for (Argument &A : F.args())
        if (args.size() < 5)
          args.push_back(toI32(B, &A));
      while (args.size() < 5)
        args.push_back(B.getInt32(0));
      B.CreateCall(putEnter, args);
    }

    std::string where = "?", argTypes;
    if (SP) {
      where = sourcePath(SP) + ":" + std::to_string(SP->getLine());
      auto sig = SP->getType()->getTypeArray();
      for (unsigned i = 1; i < sig.size() && i <= 4; i++)
        argTypes += (i > 1 ? "," : "") + typeName(sig[i]);
    }
    probes += ("F\t" + Twine(id) + "\t" + F.getName() + "\t" + where + "\t" +
               argTypes + "\n").str();
  }

  static std::string describeTypes(Module &M) {
    static const std::vector<StringRef> wanted = envList("KSEE_TYPES");
    std::set<std::string> fields(wanted.begin(), wanted.end());
    if (fields.empty())
      return "";
    DebugInfoFinder finder;
    finder.processModule(M);
    std::string table;
    std::set<std::string> seen;
    for (const DIType *T : finder.types()) {
      auto *S = dyn_cast<DICompositeType>(T);
      if (!S || S->getTag() != dwarf::DW_TAG_structure_type ||
          S->isForwardDecl() || !seen.insert(S->getName().str()).second)
        continue;
      for (const DINode *N : S->getElements()) {
        auto *F = dyn_cast<DIDerivedType>(N);
        if (F && F->getTag() == dwarf::DW_TAG_member &&
            fields.count((S->getName() + "." + F->getName()).str()))
          table += (S->getName() + "\t" + F->getName() + "\t" +
                    Twine(F->getOffsetInBits() / 8) + "\t" +
                    Twine(F->getSizeInBits() / 8) + "\n").str();
      }
    }
    return table;
  }

public:
  // Early: enter and exit probes. Late: block probes.
  KseeTrace(const std::vector<Rule> &rules, bool late) : rules(rules), late(late) {}

  PreservedAnalyses run(Module &Mod, ModuleAnalysisManager &) {
    M = &Mod;
    i32 = Type::getInt32Ty(Mod.getContext());
    i64 = Type::getInt64Ty(Mod.getContext());

    SmallVector<std::pair<Function *, unsigned>> todo;
    for (Function &F : Mod)
      if (!F.isDeclaration() && !F.getName().starts_with("__ksee_"))
        if (unsigned modes = modesFor(F) & (late ? Block : Enter | Exit))
          todo.push_back({&F, modes});
    if (!late && isHome(Mod))
      createRuntime();
    for (auto [F, modes] : todo)
      instrument(*F, modes);

    std::string types = late ? "" : describeTypes(Mod);
    if (!probes.empty())
      emitCustomSection(Mod, "ksee.probes", probes);
    if (!types.empty())
      emitCustomSection(Mod, "ksee.types", types);
    return probes.empty() ? PreservedAnalyses::all() : PreservedAnalyses::none();
  }
};

} // namespace

extern "C" LLVM_ATTRIBUTE_WEAK PassPluginLibraryInfo llvmGetPassPluginInfo() {
  return {LLVM_PLUGIN_API_VERSION, "ksee-trace", "0.2", [](PassBuilder &PB) {
            static const std::vector<Rule> rules = parseRules();
            PB.registerPipelineStartEPCallback(
                [](ModulePassManager &MPM, OptimizationLevel) {
                  MPM.addPass(KseeTrace(rules, false));
                });
            PB.registerOptimizerLastEPCallback(
                [](ModulePassManager &MPM, OptimizationLevel, ThinOrFullLTOPhase) {
                  MPM.addPass(KseeTrace(rules, true));
                });
          }};
}
