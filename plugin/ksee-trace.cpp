// kSEE trace plugin: probes that report kernel execution to the host.
//
//   KSEE_TRACE=PATTERN[:MODES],...   what to instrument
//
//   KSEE_CURRENT=symbol              variable holding the current task
//   KSEE_RING_WORDS=n                trace ring size (power of two)
//   KSEE_RING_HOME=symbol            define the ring and its writers in the
//                                    object that defines this (start_kernel)
//
// PATTERN is a glob over function names or, if it contains '/', over
// source file paths (relative to the kernel tree, e.g. kernel/sched/*). MODES is a '+'-separated subset of
//   enter  at function entry: the time, the current task and the first
//          four integer/pointer arguments (truncated to 32 bits)
//   exit   before each return: the time, the current task and the value
//   block  at the start of each basic block: the block's index; before
//          each return, a return record; and the function's frame (below)
// and defaults to "enter". Enter and exit probes are inserted before
// optimization, so they survive inlining. Block probes are inserted after
// it, so they describe the code that actually runs (and cannot keep the
// optimizer from removing code the kernel relies on being removed); a
// block's location is that of its first instruction, often in an inlined
// helper. The sequence of blocks is the branch history.
//
// Probes append records to a fixed ring in the module's own memory,
// __ksee_trace = { u32 pos; u32 armed; u32 on; u32 pad; u32 ring[KSEE_RING_WORDS]; },
// where pos counts words ever written (the ring keeps the most recent
// ones); the exported function ksee_trace() returns its address. Probes
// record only while on is nonzero (the host turns it on and off). While
// armed is nonzero, block probes (and returns) also call the ksee.stop(fn,
// n) import, which may stop the kernel there (breakpoints, stepping). pos is
// stored with release order after each record, so a reader that loads it
// sees whole records. The writer assumes one writer at a time (a
// uniprocessor kernel).
//
// A record is a head pair of words followed by continuation pairs; the
// second word's top nibble tags the pair, so a reader can start at any even
// offset (the ring may have overwritten the start of a record) and skip
// continuations until the next head:
//   [fn, 1 << 28 | n]     block n of fn; block 0 (the entry: a call) is
//                         followed by the call's arguments (the first six
//                         integer or pointer ones, truncated to 32 bits)
//   [fn, 2 << 28]         enter fn, followed by time lo, time hi, current,
//                         a0, a1, a2, a3
//   [fn, 3 << 28]         exit fn, followed by time lo, time hi, current,
//                         value
//   [fn, 4 << 28]         return from fn (block mode); while armed, it
//                         calls ksee.stop(fn, 0x0fffffff) too (stepping
//                         sees returns)
//   [addr, 7 << 28 | k]   a store of 1 << k bytes at addr (k < 4), followed
//                         by the old value (low, then for 8 bytes high): the
//                         host undoes stores to see memory as it was; k = 7:
//                         a range whose old contents are not logged, followed
//                         by its length
//   [value, 8 << 28 | i]  continuation i of the preceding head
// (The host's runtime writes kinds 5 and 6 too, and store records for its
// own writes into kernel memory.) fn is a hash of the function name (for a
// function local to its file, of "file:name"); time comes from the ksee.now
// import.
//
// Memory: a function with block probes logs each store it makes (not to
// its own stack slots), and each atomic, before it. Bulk writes (memcpy,
// memset, and wider stores) go to the ksee.bulk(address, length) import,
// which logs them.
//
// The linked module describes its probes in a wasm custom section, which
// wasm-ld concatenates across objects (lines may repeat). Each object's
// part starts with U and its files, which the lines after it name by
// number (FILE below):
//   ksee.probes  U
//                P \t k \t path       file k
//                R \t ring words
//                F \t fn \t name \t FILE:line \t argument types [\t known]
//                  (known: with block probes, a bit mask of the source's
//                  parameters that call records carry; the others were
//                  optimized away, recorded as 0)
//                B \t fn \t n \t FILE:line:column
//                L \t fn \t n \t lines: the source lines the block runs, in
//                  order (each once per stretch between calls), separated
//                  by ';': FILE:line (:line in the same file as the one
//                  before), then @function where it is inlined from
//                  function; "|" where the block calls a
//                  function (which runs in between). Only for blocks with
//                  more than their first line.
// Source locations come from debug info, so compile with -g. (The
// kernel's types are extracted from its DWARF after linking: ksee pack.)
#include "llvm/BinaryFormat/Dwarf.h"
#include "llvm/IR/DebugInfoMetadata.h"
#include "llvm/IR/DebugProgramInstruction.h"
#include "llvm/IR/InstIterator.h"
#include "llvm/IR/IntrinsicInst.h"
#include "llvm/Analysis/ValueTracking.h"
#include "llvm/IR/IRBuilder.h"
#include "llvm/IR/Module.h"
#include "llvm/IR/PassManager.h"
#include "llvm/Passes/PassBuilder.h"
#include "llvm/Plugins/PassPlugin.h"
#include "llvm/Support/GlobPattern.h"
#include "llvm/Support/Path.h"

#include <cstdlib>
#include <map>
#include <set>
#include <tuple>
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

uint32_t fnv(StringRef s, uint32_t h = 2166136261u) {
  for (unsigned char c : s)
    h = (h ^ c) * 16777619u;
  return h;
}

// Functions local to a file are told apart by the file's name.
uint32_t fnId(const Function &F) {
  if (!F.hasLocalLinkage())
    return fnv(F.getName());
  return fnv(F.getName(), fnv(":", fnv(F.getParent()->getSourceFileName())));
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

// The file, relative to the compilation directory (the kernel tree) when it
// is inside it.
std::string sourcePath(const DIScope *S) {
  StringRef file = S->getFilename();
  if (sys::path::is_absolute(file))
    return file.str();
  file.consume_front("./");
  return file.str();
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

enum Kind { KindBlock = 1, KindEnter = 2, KindExit = 3, KindReturn = 4, KindStore = 7, KindValue = 8 };

constexpr unsigned CallArgs = 6;

unsigned ringWords() {
  const char *env = std::getenv("KSEE_RING_WORDS");
  unsigned n = env ? std::strtoul(env, nullptr, 0) : 1u << 21;
  if (!n || (n & (n - 1)))
    report_fatal_error("KSEE_RING_WORDS must be a power of two");
  return n;
}

class KseeTrace : public PassInfoMixin<KseeTrace> {
  const std::vector<Rule> &rules;
  bool late;
  Module *M = nullptr;
  Type *i32 = nullptr, *i64 = nullptr;
  Function *putBlock = nullptr, *putEnter = nullptr, *putExit = nullptr, *putReturn = nullptr;
  Function *putStore[4] = {};         // 1, 2, 4, 8 bytes
  Function *putCall[CallArgs + 1] = {}; // an entry block, with k arguments
  FunctionCallee bulk;
  std::string probes;
  // This object's files, numbered for the probes' lines.
  std::map<std::string, unsigned> files;

  std::string fileRef(const std::string &path) {
    return std::to_string(files.try_emplace(path, files.size()).first->second);
  }

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

  void declareBulk() {
    bulk = M->getOrInsertFunction("__ksee_bulk", FunctionType::get(Type::getVoidTy(M->getContext()), {i32, i32}, false));
    cast<Function>(bulk.getCallee())->addFnAttr("wasm-import-module", "ksee");
    cast<Function>(bulk.getCallee())->addFnAttr("wasm-import-name", "bulk");
  }

  // Log the store (or atomic, or memset/memcpy) `I` is about to make, if it
  // is not to its function's own stack.
  void logStore(Instruction *I) {
    const DataLayout &DL = M->getDataLayout();
    Value *ptr = nullptr, *len = nullptr;
    uint64_t size = 0;
    if (auto *S = dyn_cast<StoreInst>(I))
      ptr = S->getPointerOperand(), size = DL.getTypeStoreSize(S->getValueOperand()->getType());
    else if (auto *A = dyn_cast<AtomicRMWInst>(I))
      ptr = A->getPointerOperand(), size = DL.getTypeStoreSize(A->getValOperand()->getType());
    else if (auto *X = dyn_cast<AtomicCmpXchgInst>(I))
      ptr = X->getPointerOperand(), size = DL.getTypeStoreSize(X->getNewValOperand()->getType());
    else if (auto *MI = dyn_cast<MemIntrinsic>(I))
      ptr = MI->getRawDest(), len = MI->getLength();
    if (!ptr || isa<AllocaInst>(getUnderlyingObject(ptr)))
      return;
    IRBuilder<> B(I);
    Value *addr = B.CreatePtrToInt(ptr, i32);
    int k = size == 1 ? 0 : size == 2 ? 1 : size == 4 ? 2 : size == 8 ? 3 : -1;
    if (!len && k >= 0)
      B.CreateCall(putStore[k], {addr});
    else
      B.CreateCall(bulk, {addr, len ? B.CreateZExtOrTrunc(len, i32) : B.getInt32(size)});
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
      putReturn = writerDecl("__ksee_put_return", 1);
      for (unsigned k = 0; k < 4; k++)
        putStore[k] = writerDecl(("__ksee_put_store" + Twine(1 << k)).str(), 1);
      for (unsigned k = 0; k <= CallArgs; k++)
        putCall[k] = writerDecl(("__ksee_put_call" + Twine(k)).str(), 1 + k);
      declareBulk();
      return;
    }
    declareBulk();
    LLVMContext &C = M->getContext();
    unsigned words = ringWords();
    auto *traceTy = StructType::get(C, {i32, i32, i32, i32, ArrayType::get(i32, words)});
    auto *trace = shared(new GlobalVariable(
        *M, traceTy, false, GlobalValue::ExternalLinkage,
        ConstantAggregateZero::get(traceTy), "__ksee_trace"));
    trace->setAlignment(Align(8));

    // The export that finds the ring.
    for (auto [name, export_, var] : {std::tuple{"__ksee_trace_info", "ksee_trace", (GlobalVariable *)trace}}) {
      Function *info = shared(Function::Create(FunctionType::get(i32, false),
                                               GlobalValue::ExternalLinkage, name, M));
      info->addFnAttr("wasm-export-name", export_);
      IRBuilder<> IB(BasicBlock::Create(C, "", info));
      IB.CreateRet(IB.CreatePtrToInt(var, i32));
    }

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
      // Only while on.
      BasicBlock *write = BasicBlock::Create(C, "write", F), *done = BasicBlock::Create(C, "done", F);
      Value *on = B.CreateLoad(i32, B.CreateStructGEP(traceTy, trace, 2));
      B.CreateCondBr(B.CreateICmpNE(on, B.getInt32(0)), write, done);
      B.SetInsertPoint(write);
      SmallVector<Value *> words;
      body(B, F, words);
      Value *pos = B.CreateStructGEP(traceTy, trace, 0);
      Value *ring = B.CreateStructGEP(traceTy, trace, 4);
      Value *p = B.CreateLoad(i32, pos);
      for (auto [i, w] : enumerate(words)) {
        Value *idx = B.CreateAnd(B.CreateAdd(p, B.getInt32(i)), B.getInt32(ringWords() - 1));
        B.CreateStore(w, B.CreateGEP(i32, ring, idx));
      }
      B.CreateAlignedStore(B.CreateAdd(p, B.getInt32(words.size())), pos, Align(4))
          ->setAtomic(AtomicOrdering::Release);
      B.CreateBr(done);
      B.SetInsertPoint(done);
      if (name == "__ksee_put_block")
        stopHook(B, F, traceTy, trace, F->getArg(1));
      else if (name.starts_with("__ksee_put_call"))
        stopHook(B, F, traceTy, trace, B.getInt32(0));
      else if (name == "__ksee_put_return")
        stopHook(B, F, traceTy, trace, B.getInt32(0x0fffffff));
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
    putReturn = writer("__ksee_put_return", 1, [&](IRBuilder<> &B, Function *F, auto &words) {
      words.push_back(F->getArg(0));
      words.push_back(B.getInt32(KindReturn << 28));
    });
    for (unsigned k = 0; k <= CallArgs; k++)
      putCall[k] = writer(("__ksee_put_call" + Twine(k)).str(), 1 + k, [&](IRBuilder<> &B, Function *F, auto &words) {
        words.push_back(F->getArg(0));
        words.push_back(B.getInt32(KindBlock << 28));
        for (unsigned i = 0; i < k; i++)
          value(B, words, F->getArg(1 + i));
      });
    // Stores: the old value, before the store.
    for (unsigned k = 0; k < 4; k++)
      putStore[k] = writer(("__ksee_put_store" + Twine(1 << k)).str(), 1, [&](IRBuilder<> &B, Function *F, auto &words) {
        Value *addr = F->getArg(0);
        Value *old = B.CreateZExt(B.CreateAlignedLoad(IntegerType::get(C, 8 << k),
                                                      B.CreateIntToPtr(addr, PointerType::getUnqual(C)), Align(1)), i64);
        words.push_back(addr);
        words.push_back(B.getInt32(KindStore << 28 | k));
        value(B, words, B.CreateTrunc(old, i32));
        if (k == 3)
          value(B, words, B.CreateTrunc(B.CreateLShr(old, 32), i32));
      });
    probes += "R\t" + std::to_string(words) + "\n";
  }

  // if (armed) ksee.stop(fn, n), at B (which then continues after it).
  void stopHook(IRBuilder<> &B, Function *F, Type *traceTy, Value *trace, Value *n) {
    LLVMContext &C = M->getContext();
    Value *armed = B.CreateLoad(i32, B.CreateStructGEP(traceTy, trace, 1));
    BasicBlock *stop = BasicBlock::Create(C, "stop", F), *done = BasicBlock::Create(C, "done", F);
    B.CreateCondBr(B.CreateICmpNE(armed, B.getInt32(0)), stop, done);
    B.SetInsertPoint(stop);
    FunctionCallee hook = M->getOrInsertFunction(
        "__ksee_stop", FunctionType::get(Type::getVoidTy(C), {i32, i32}, false));
    cast<Function>(hook.getCallee())->addFnAttr("wasm-import-module", "ksee");
    cast<Function>(hook.getCallee())->addFnAttr("wasm-import-name", "stop");
    B.CreateCall(hook, {F->getArg(0), n});
    B.CreateBr(done);
    B.SetInsertPoint(done);
  }

  // Whether a call runs code with probes (its blocks come in between).
  static bool runsProbes(const CallBase &CB) {
    const Function *callee = CB.getCalledFunction();
    if (!callee)
      return true; // an indirect call
    if (callee->isIntrinsic() || callee->getName().starts_with("__ksee_"))
      return false;
    return !callee->hasFnAttribute("wasm-import-module");
  }

  // The block's lines, as for an L line; empty if just its first line.
  std::string blockLines(BasicBlock &BB) {
    std::vector<std::string> items;
    std::set<std::string> seen; // (since the last call)
    std::string file;           // the last item's
    for (Instruction &I : BB) {
      if (auto *CB = dyn_cast<CallBase>(&I); CB && !isa<DbgInfoIntrinsic>(CB) && runsProbes(*CB)) {
        if (!items.empty() && items.back() != "|")
          items.push_back("|");
        seen.clear();
        continue;
      }
      DILocation *L = I.getDebugLoc().get();
      if (!L || !L->getLine())
        continue;
      std::string path = fileRef(sourcePath(L->getScope()));
      std::string item = path + ":" + std::to_string(L->getLine());
      if (L->getInlinedAt())
        if (DISubprogram *SP = L->getScope()->getSubprogram())
          item += "@" + SP->getName().str();
      if (seen.insert(item).second) {
        items.push_back(path == file ? item.substr(path.size()) : item);
        file = path;
      }
    }
    while (!items.empty() && items.back() == "|")
      items.pop_back();
    if (items.size() <= 1)
      return "";
    std::string out;
    for (auto &i : items)
      out += (out.empty() ? "" : ";") + i;
    return out;
  }

  std::string location(const DILocation *L) {
    return (fileRef(sourcePath(L->getScope())) + ":" + Twine(L->getLine()) + ":" +
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
    uint32_t id = fnId(F);
    DISubprogram *SP = F.getSubprogram();

    SmallVector<BasicBlock *> blocks;
    for (BasicBlock &BB : F)
      blocks.push_back(&BB);
    // The source's parameters: the value each has at entry (an argument, or
    // a constant the optimizer put in its place), from the debug records
    // (after optimization, arguments may no longer be the source's).
    SmallVector<Value *, CallArgs> params;
    unsigned known = 0;
    if ((modes & Block) && SP) {
      for (Instruction &I : instructions(F))
        for (DbgVariableRecord &R : filterDbgVars(I.getDbgRecordRange())) {
          DILocalVariable *V = R.getVariable();
          unsigned k = V ? V->getArg() : 0;
          if (!k || k > CallArgs || V->getScope()->getSubprogram() != SP || R.getNumVariableLocationOps() != 1)
            continue;
          Value *v = R.getVariableLocationOp(0);
          if (!isa_and_nonnull<Argument, ConstantInt>(v) || (known >> (k - 1) & 1))
            continue;
          if (params.size() < k)
            params.resize(k, nullptr);
          params[k - 1] = v;
          known |= 1 << (k - 1);
        }
    }

    // The function's own writes to memory (before any probe is added).
    SmallVector<Instruction *> writes;
    if (modes & Block)
      for (Instruction &I : instructions(F))
        if (isa<StoreInst, AtomicRMWInst, AtomicCmpXchgInst, MemIntrinsic>(I))
          writes.push_back(&I);

    if (modes & Block)
      for (auto [n, BB] : enumerate(blocks)) {
        IRBuilder<> B(&*BB->getFirstInsertionPt());
        if (n == 0) {
          // The entry: the call, with its arguments, in the source's order
          // (0 for those the optimizer took away: see known).
          SmallVector<Value *, 1 + CallArgs> args{B.getInt32(id)};
          for (unsigned k = 0; k < params.size(); k++)
            args.push_back(params[k] ? toI32(B, params[k]) : B.getInt32(0));
          B.CreateCall(putCall[args.size() - 1], args);
        } else {
          B.CreateCall(putBlock, {B.getInt32(id), B.getInt32(n)});
        }
        std::string loc = "?";
        for (Instruction &I : *BB)
          if (DILocation *L = I.getDebugLoc().get(); L && L->getLine()) {
            loc = location(L);
            break;
          }
        probes += ("B\t" + Twine(id) + "\t" + Twine(n) + "\t" + loc + "\n").str();
        if (std::string lines = blockLines(*BB); !lines.empty())
          probes += ("L\t" + Twine(id) + "\t" + Twine(n) + "\t" + lines + "\n").str();
      }

    if (modes & Block) {
      // A return record at each return.
      for (BasicBlock *BB : blocks)
        if (auto *R = dyn_cast<ReturnInst>(BB->getTerminator())) {
          // (A musttail call must stay right before its return.)
          Instruction *at = R;
          if (auto *C = dyn_cast_or_null<CallInst>(R->getPrevNode()); C && C->isMustTailCall())
            at = C;
          IRBuilder<> RB(at);
          RB.CreateCall(putReturn, {RB.getInt32(id)});
        }
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

    for (Instruction *I : writes)
      logStore(I);

    std::string where = "?", argTypes;
    if (SP) {
      where = fileRef(sourcePath(SP)) + ":" + std::to_string(SP->getLine());
      auto sig = SP->getType()->getTypeArray();
      for (unsigned i = 1; i < sig.size() && i <= 4; i++)
        argTypes += (i > 1 ? "," : "") + typeName(sig[i]);
    }
    probes += ("F\t" + Twine(id) + "\t" + F.getName() + "\t" + where + "\t" +
               argTypes + ((modes & Block) ? "\t" + Twine(known) : Twine()) + "\n").str();
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

    if (!probes.empty())
    {
      std::string head = "U\n";
      for (auto &[path, k] : files)
        head += "P\t" + std::to_string(k) + "\t" + path + "\n";
      emitCustomSection(Mod, "ksee.probes", head + probes);
    }
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
