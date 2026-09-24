// kSEE trace plugin: probes that report kernel execution to the host.
//
//   KSEE_TRACE=PATTERN[:MODES],...   what to instrument
//   KSEE_TYPES=struct.field,...      struct fields whose layout to publish
//
// PATTERN is a glob over function names or, if it contains '/', over
// source file paths. MODES is a '+'-separated subset of
//   enter  ksee.enter(fn, a0, a1, a2, a3) at function entry, with the first
//          four integer/pointer arguments (truncated to 32 bits, 0 if absent)
//   exit   ksee.exit(fn, ret) before each return (ret is 0 for void)
//   block  ksee.block(fn, n) at the start of basic block n
// and defaults to "enter". Probes are inserted before optimization, so
// blocks follow the source-level control flow; probes in inlined code keep
// their own ids. fn is a hash of the function name.
//
// The linked module describes its probes in two wasm custom sections, which
// wasm-ld concatenates across objects (lines may repeat):
//   ksee.probes  F \t fn \t name \t file:line \t argument types
//                B \t fn \t n \t line:column
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

class KseeTrace : public PassInfoMixin<KseeTrace> {
  const std::vector<Rule> &rules;
  FunctionCallee enter, exit, block;
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

  FunctionCallee import(Module &M, StringRef name, FunctionType *T) {
    FunctionCallee f = M.getOrInsertFunction(("__ksee_" + name).str(), T);
    auto *decl = cast<Function>(f.getCallee());
    decl->addFnAttr("wasm-import-module", "ksee");
    decl->addFnAttr("wasm-import-name", name);
    return f;
  }

  static Value *toI32(IRBuilder<> &B, Value *V) {
    Type *T = V->getType();
    if (T->isPointerTy())
      return B.CreatePtrToInt(V, B.getInt32Ty());
    if (T->isIntegerTy())
      return B.CreateZExtOrTrunc(V, B.getInt32Ty());
    return B.getInt32(0);
  }

  void instrument(Function &F, unsigned modes) {
    uint32_t id = fnId(F.getName());
    DISubprogram *SP = F.getSubprogram();

    // Collect first: probes add no blocks, but keep iteration simple.
    SmallVector<BasicBlock *> blocks;
    for (BasicBlock &BB : F)
      blocks.push_back(&BB);

    if (modes & Block)
      for (auto [n, BB] : enumerate(blocks)) {
        IRBuilder<> B(&*BB->getFirstInsertionPt());
        B.CreateCall(block, {B.getInt32(id), B.getInt32(n)});
        std::string loc = "?";
        for (Instruction &I : *BB)
          if (DebugLoc DL = I.getDebugLoc()) {
            loc = (Twine(DL.getLine()) + ":" + Twine(DL.getCol())).str();
            break;
          }
        probes += ("B\t" + Twine(id) + "\t" + Twine(n) + "\t" + loc + "\n").str();
      }

    if (modes & Exit)
      for (BasicBlock *BB : blocks)
        if (auto *R = dyn_cast<ReturnInst>(BB->getTerminator())) {
          IRBuilder<> B(R);
          Value *ret = R->getReturnValue();
          B.CreateCall(exit, {B.getInt32(id), ret ? toI32(B, ret) : B.getInt32(0)});
        }

    if (modes & Enter) {
      IRBuilder<> B(&*F.getEntryBlock().getFirstInsertionPt());
      SmallVector<Value *, 5> args{B.getInt32(id)};
      for (Argument &A : F.args())
        if (args.size() < 5)
          args.push_back(toI32(B, &A));
      while (args.size() < 5)
        args.push_back(B.getInt32(0));
      B.CreateCall(enter, args);
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
  KseeTrace(const std::vector<Rule> &rules) : rules(rules) {}

  PreservedAnalyses run(Module &M, ModuleAnalysisManager &) {
    LLVMContext &C = M.getContext();
    Type *i32 = Type::getInt32Ty(C), *vd = Type::getVoidTy(C);
    enter = import(M, "enter", FunctionType::get(vd, {i32, i32, i32, i32, i32}, false));
    exit = import(M, "exit", FunctionType::get(vd, {i32, i32}, false));
    block = import(M, "block", FunctionType::get(vd, {i32, i32}, false));

    for (Function &F : M)
      if (!F.isDeclaration() && !F.getName().starts_with("__ksee_"))
        if (unsigned modes = modesFor(F))
          instrument(F, modes);

    std::string types = describeTypes(M);
    if (!probes.empty())
      emitCustomSection(M, "ksee.probes", probes);
    if (!types.empty())
      emitCustomSection(M, "ksee.types", types);
    // Drop the declarations nobody called, so they are not imported.
    for (FunctionCallee f : {enter, exit, block})
      if (auto *decl = cast<Function>(f.getCallee()); decl->use_empty())
        decl->eraseFromParent();
    return probes.empty() ? PreservedAnalyses::all() : PreservedAnalyses::none();
  }
};

} // namespace

extern "C" LLVM_ATTRIBUTE_WEAK PassPluginLibraryInfo llvmGetPassPluginInfo() {
  return {LLVM_PLUGIN_API_VERSION, "ksee-trace", "0.2", [](PassBuilder &PB) {
            static const std::vector<Rule> rules = parseRules();
            PB.registerPipelineStartEPCallback(
                [](ModulePassManager &MPM, OptimizationLevel) {
                  MPM.addPass(KseeTrace(rules));
                });
          }};
}
