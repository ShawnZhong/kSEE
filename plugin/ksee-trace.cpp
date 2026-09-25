// kSEE trace plugin: probes that record the kernel's execution, for the
// host (crates/ksee-core/src/trace.rs decodes them).
//
// After optimization (so they describe the code that actually runs, and
// cannot keep the optimizer from removing code the kernel relies on being
// removed), every function gets:
//   - at the start of each basic block, a block record (the entry block's
//     is the call, with its arguments);
//   - before each return, a return record, with the value;
//   - once its locals in memory exist, a locals record: their addresses;
//   - before each store it makes (not to its own stack slots, except those
//     locals), a store
//     record, with the old value (the host undoes stores to see memory as
//     it was). Bulk writes (memcpy, memset, wider stores) go to the
//     ksee.bulk(address, length) import, which logs them.
// Compile with -g: locations and parameters come from debug info.
//
// Probes append records to a ring in the module's own memory, defined in
// the object that defines start_kernel (wasm-ld keeps every copy of weak
// or COMDAT data): __ksee_trace = { u32 pos; u32 pad; u32 on; u32 pad;
// u32 ring[RING_WORDS]; }; the exported ksee_trace() returns its address.
// pos counts words ever written (the ring keeps the most recent), stored
// with release order after each record, so a reader that loads it sees
// whole records. Probes record only while on is nonzero (the host turns
// it on and off). One writer at a time: the kernel is uniprocessor.
//
// A record is a head pair of words and continuation pairs; the second
// word's top nibble tags the pair, so a reader can start at any even offset
// (the ring may have overwritten the start of a record) and skip
// continuations until the next head:
//   [fn, 1 << 28 | n]     block n of fn; block 0 (a call) is followed by its
//                         arguments: the source's parameters, up to six,
//                         truncated to 32 bits (0 for any the optimizer took
//                         away: see F lines)
//   [fn, 2 << 28 | k]     fn's k locals in memory (address-taken: arrays,
//                         structs, ...), once they exist: their addresses
//                         follow (their names: V lines)
//   [fn, 4 << 28]         a return from fn, followed by the value
//   [addr, 7 << 28 | k]   a store of 1 << k bytes at addr (k < 4), followed
//                         by the old value (low, then for 8 bytes high); k =
//                         7: a range whose old contents are not logged,
//                         followed by its length
//   [value, 8 << 28 | i]  continuation i of the head before
// (The host's runtime writes kinds 5 and 6 too, and store records for its
// own writes into kernel memory.) fn is a hash of the function's name (for a
// function local to its file, of "file:name").
//
// The linked module describes its probes in a wasm custom section, which
// wasm-ld concatenates across objects (lines may repeat). Each object's
// part starts with U and its files, which the lines after it name by
// number (FILE below):
//   ksee.probes  U
//                P \t k \t path       file k
//                R \t ring words
//                F \t fn \t name \t FILE:line \t known
//                  (known: a bit mask of the source's parameters that call
//                  records carry; the others were optimized away)
//                V \t fn \t names   the locals of fn's locals records
//                B \t fn \t n \t FILE:line:column
//                L \t fn \t n \t lines: the source lines the block runs, in
//                  order (each once per stretch between calls), separated
//                  by ';': FILE:line (:line in the same file as the one
//                  before), then, if it is inlined, @function^FILE:line
//                  per function it is inlined from, innermost first (the
//                  line: where that function is inlined, in the next one
//                  out); "|" where the block calls a function (which runs
//                  in between). Only for blocks with more than their first
//                  line, or whose first line is inlined.
#include "llvm/Analysis/ValueTracking.h"
#include "llvm/IR/DebugInfoMetadata.h"
#include "llvm/IR/DebugProgramInstruction.h"
#include "llvm/IR/IRBuilder.h"
#include "llvm/IR/InstIterator.h"
#include "llvm/IR/IntrinsicInst.h"
#include "llvm/IR/Module.h"
#include "llvm/IR/PassManager.h"
#include "llvm/Passes/PassBuilder.h"
#include "llvm/Plugins/PassPlugin.h"
#include "llvm/Support/Path.h"

#include <map>
#include <set>
#include <string>
#include <vector>

using namespace llvm;

namespace {

constexpr unsigned RingWords = 1u << 22; // 16 MiB: a whole boot
constexpr unsigned CallArgs = 6;
constexpr unsigned Locals = 8;
enum Kind { KindBlock = 1, KindLocals = 2, KindReturn = 4, KindStore = 7, KindValue = 8 };

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

// Whether a call runs code with probes (its blocks come in between).
bool runsProbes(const CallBase &CB) {
  const Function *callee = CB.getCalledFunction();
  if (!callee)
    return true; // an indirect call
  if (callee->isIntrinsic() || callee->getName().starts_with("__ksee_"))
    return false;
  return !callee->hasFnAttribute("wasm-import-module");
}

class KseeTrace : public PassInfoMixin<KseeTrace> {
  Module *M = nullptr;
  Type *i32 = nullptr, *i64 = nullptr;
  Function *putBlock = nullptr, *putReturn = nullptr;
  Function *putCall[CallArgs + 1] = {}; // an entry block, with k arguments
  Function *putStore[4] = {};           // 1, 2, 4, 8 bytes
  Function *putLocals[Locals + 1] = {}; // k locals' addresses
  FunctionCallee bulk;
  std::string probes;
  std::map<std::string, unsigned> files; // this object's, numbered

  std::string fileRef(const std::string &path) {
    return std::to_string(files.try_emplace(path, files.size()).first->second);
  }

  template <typename T> T *hidden(T *GV) {
    GV->setVisibility(GlobalValue::HiddenVisibility);
    return GV;
  }

  FunctionCallee import(StringRef name, StringRef import, FunctionType *T) {
    FunctionCallee f = M->getOrInsertFunction(name, T);
    cast<Function>(f.getCallee())->addFnAttr("wasm-import-module", "ksee");
    cast<Function>(f.getCallee())->addFnAttr("wasm-import-name", import);
    return f;
  }

  // The writers: declared, or (in the object that defines start_kernel)
  // defined, with the ring and the export that finds it.
  void createRuntime() {
    LLVMContext &C = M->getContext();
    Type *vd = Type::getVoidTy(C);
    auto names = [&](auto f) {
      f(putBlock, "__ksee_put_block", 2);
      f(putReturn, "__ksee_put_return", 2);
      for (unsigned k = 0; k <= CallArgs; k++)
        f(putCall[k], "__ksee_put_call" + std::to_string(k), 1 + k);
      for (unsigned k = 0; k < 4; k++)
        f(putStore[k], "__ksee_put_store" + std::to_string(1 << k), 1);
      for (unsigned k = 1; k <= Locals; k++)
        f(putLocals[k], "__ksee_put_locals" + std::to_string(k), 1 + k);
    };
    bulk = import("__ksee_bulk", "bulk", FunctionType::get(vd, {i32, i32}, false));
    GlobalValue *home = M->getNamedValue("start_kernel");
    if (!home || home->isDeclaration()) {
      names([&](Function *&F, const std::string &name, unsigned n) {
        F = cast<Function>(M->getOrInsertFunction(name, FunctionType::get(vd, SmallVector<Type *>(n, i32), false)).getCallee());
      });
      return;
    }

    auto *traceTy = StructType::get(C, {i32, i32, i32, i32, ArrayType::get(i32, RingWords)});
    auto *trace = hidden(new GlobalVariable(*M, traceTy, false, GlobalValue::ExternalLinkage,
                                            ConstantAggregateZero::get(traceTy), "__ksee_trace"));
    trace->setAlignment(Align(8));
    Function *info = hidden(Function::Create(FunctionType::get(i32, false), GlobalValue::ExternalLinkage,
                                             "__ksee_trace_info", M));
    info->addFnAttr("wasm-export-name", "ksee_trace");
    IRBuilder<> IB(BasicBlock::Create(C, "", info));
    IB.CreateRet(IB.CreatePtrToInt(trace, i32));
    auto field = [&](IRBuilder<> &B, unsigned k) { return B.CreateStructGEP(traceTy, trace, k); };

    // A writer: `words` of a record from its arguments, while on.
    auto writer = [&](Function *&F, const std::string &name, unsigned n, auto words) {
      F = hidden(Function::Create(FunctionType::get(vd, SmallVector<Type *>(n, i32), false),
                                  GlobalValue::ExternalLinkage, name, M));
      F->addFnAttr(Attribute::NoInline);
      IRBuilder<> B(BasicBlock::Create(C, "", F));
      BasicBlock *write = BasicBlock::Create(C, "write", F), *done = BasicBlock::Create(C, "done", F);
      B.CreateCondBr(B.CreateICmpNE(B.CreateLoad(i32, field(B, 2)), B.getInt32(0)), write, done);
      B.SetInsertPoint(write);
      SmallVector<Value *> w;
      // Continuation pairs: [value, KindValue << 28 | index].
      auto value = [&](Value *v) {
        unsigned index = w.size() / 2 - 1;
        w.push_back(v);
        w.push_back(B.getInt32(KindValue << 28 | index));
      };
      words(B, F, w, value);
      Value *p = B.CreateLoad(i32, field(B, 0));
      for (auto [i, v] : enumerate(w)) {
        Value *idx = B.CreateAnd(B.CreateAdd(p, B.getInt32(i)), B.getInt32(RingWords - 1));
        B.CreateStore(v, B.CreateGEP(i32, field(B, 4), idx));
      }
      B.CreateAlignedStore(B.CreateAdd(p, B.getInt32(w.size())), field(B, 0), Align(4))
          ->setAtomic(AtomicOrdering::Release);
      B.CreateBr(done);
      B.SetInsertPoint(done);
      B.CreateRetVoid();
    };
    names([&](Function *&F, const std::string &name, unsigned n) {
      writer(F, name, n, [&](IRBuilder<> &B, Function *F, auto &w, auto value) {
        if (StringRef(name).starts_with("__ksee_put_call") || name == "__ksee_put_block") {
          bool call = name != "__ksee_put_block";
          w.push_back(F->getArg(0));
          w.push_back(call ? B.getInt32(KindBlock << 28) : B.CreateOr(F->getArg(1), B.getInt32(KindBlock << 28)));
          for (unsigned i = 1; call && i < n; i++)
            value(F->getArg(i));
          return;
        }
        if (StringRef(name).starts_with("__ksee_put_locals")) {
          w.push_back(F->getArg(0));
          w.push_back(B.getInt32(KindLocals << 28 | (n - 1)));
          for (unsigned i = 1; i < n; i++)
            value(F->getArg(i));
          return;
        }
        if (name == "__ksee_put_return") {
          w.push_back(F->getArg(0));
          w.push_back(B.getInt32(KindReturn << 28));
          value(F->getArg(1));
          return;
        }
        // A store of 1 << k bytes: the old value.
        unsigned k = Log2_32(std::stoi(name.substr(strlen("__ksee_put_store"))));
        Value *addr = F->getArg(0);
        Value *old = B.CreateZExt(B.CreateAlignedLoad(IntegerType::get(C, 8 << k),
                                                      B.CreateIntToPtr(addr, PointerType::getUnqual(C)), Align(1)), i64);
        w.push_back(addr);
        w.push_back(B.getInt32(KindStore << 28 | k));
        value(B.CreateTrunc(old, i32));
        if (k == 3)
          value(B.CreateTrunc(B.CreateLShr(old, 32), i32));
      });
    });
    probes += "R\t" + std::to_string(RingWords) + "\n";
  }

  std::string location(const DILocation *L) {
    return (fileRef(sourcePath(L->getScope())) + ":" + Twine(L->getLine()) + ":" + Twine(L->getColumn())).str();
  }

  // The block's lines, as for an L line; empty if just its first line.
  std::string blockLines(BasicBlock &BB) {
    std::vector<std::string> items;
    std::set<std::string> seen; // (since the last call)
    std::string file;           // the last item's
    for (Instruction &I : BB) {
      if (auto *CB = dyn_cast<CallBase>(&I); CB && runsProbes(*CB)) {
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
      // The functions it is inlined from, innermost first, each with where
      // it is inlined (a line of the next one out).
      for (DILocation *At = L; DILocation *Site = At->getInlinedAt(); At = Site) {
        DISubprogram *SP = At->getScope()->getSubprogram();
        item += "@" + (SP ? SP->getName().str() : std::string("?")) + "^" +
                fileRef(sourcePath(Site->getScope())) + ":" + std::to_string(Site->getLine());
      }
      if (seen.insert(item).second) {
        items.push_back(path == file ? item.substr(path.size()) : item);
        file = path;
      }
    }
    while (!items.empty() && items.back() == "|")
      items.pop_back();
    // (Its first line is on the B line, unless it is inlined.)
    if (items.empty() || (items.size() == 1 && items[0].find('@') == std::string::npos))
      return "";
    std::string out;
    for (auto &i : items)
      out += (out.empty() ? "" : ";") + i;
    return out;
  }

  Value *toI32(IRBuilder<> &B, Value *V) {
    Type *T = V->getType();
    if (T->isPointerTy())
      return B.CreatePtrToInt(V, i32);
    if (T->isIntegerTy())
      return B.CreateZExtOrTrunc(V, i32);
    return B.getInt32(0);
  }

  // Log the store (or atomic, or memset/memcpy) `I` is about to make, if it
  // is not to its function's own stack (but for its `locals`).
  void logStore(Instruction *I, const SmallPtrSetImpl<AllocaInst *> &locals) {
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
    if (!ptr)
      return;
    if (auto *A = dyn_cast<AllocaInst>(getUnderlyingObject(ptr)); A && !locals.contains(A))
      return;
    IRBuilder<> B(I);
    Value *addr = B.CreatePtrToInt(ptr, i32);
    int k = size == 1 ? 0 : size == 2 ? 1 : size == 4 ? 2 : size == 8 ? 3 : -1;
    if (!len && k >= 0)
      B.CreateCall(putStore[k], {addr});
    else
      B.CreateCall(bulk, {addr, len ? B.CreateZExtOrTrunc(len, i32) : B.getInt32(size)});
  }

  void instrument(Function &F) {
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
    // The function's own writes to memory (before any probe is added).
    SmallVector<Instruction *> writes;
    // Its locals in memory: the named stack slots (dbg declares) left after
    // optimization (whose address is taken).
    SmallVector<std::pair<AllocaInst *, std::string>> locals;
    SmallPtrSet<AllocaInst *, Locals> slots;
    for (Instruction &I : instructions(F)) {
      if (isa<StoreInst, AtomicRMWInst, AtomicCmpXchgInst, MemIntrinsic>(I))
        writes.push_back(&I);
      for (DbgVariableRecord &R : filterDbgVars(I.getDbgRecordRange())) {
        DILocalVariable *V = R.getVariable();
        // (A declare's location is the slot; an assign's address is.)
        Value *slot = R.isDbgDeclare() ? R.getVariableLocationOp(0) : R.isDbgAssign() ? R.getAddress() : nullptr;
        if (slot && V && !V->getArg() && SP && V->getScope()->getSubprogram() == SP && locals.size() < Locals)
          if (auto *A = dyn_cast<AllocaInst>(slot); A && A->isStaticAlloca() && slots.insert(A).second)
            locals.push_back({A, V->getName().str()});
        unsigned k = V ? V->getArg() : 0;
        if (!SP || !k || k > CallArgs || V->getScope()->getSubprogram() != SP || R.getNumVariableLocationOps() != 1)
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

    for (auto [n, BB] : enumerate(blocks)) {
      IRBuilder<> B(&*BB->getFirstInsertionPt());
      if (n == 0) {
        SmallVector<Value *, 1 + CallArgs> args{B.getInt32(id)};
        for (Value *p : params)
          args.push_back(p ? toI32(B, p) : B.getInt32(0));
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

    for (BasicBlock *BB : blocks)
      if (auto *R = dyn_cast<ReturnInst>(BB->getTerminator())) {
        // (A musttail call must stay right before its return.)
        Instruction *at = R;
        if (auto *C = dyn_cast_or_null<CallInst>(R->getPrevNode()); C && C->isMustTailCall())
          at = C;
        IRBuilder<> B(at);
        Value *ret = R->getReturnValue();
        B.CreateCall(putReturn, {B.getInt32(id), ret ? toI32(B, ret) : B.getInt32(0)});
      }

    for (Instruction *I : writes)
      logStore(I, slots);

    // The locals record, once they all exist (after the last of them).
    if (!locals.empty()) {
      Instruction *last = locals[0].first;
      for (auto &[A, _] : locals)
        if (last->comesBefore(A))
          last = A;
      IRBuilder<> B(last->getNextNode());
      while (isa<AllocaInst>(&*B.GetInsertPoint()))
        B.SetInsertPoint(B.GetInsertPoint()->getNextNode());
      SmallVector<Value *, 1 + Locals> args{B.getInt32(id)};
      std::string names;
      for (auto &[A, name] : locals) {
        args.push_back(B.CreatePtrToInt(A, i32));
        names += (names.empty() ? "" : ",") + name;
      }
      B.CreateCall(putLocals[locals.size()], args);
      probes += ("V\t" + Twine(id) + "\t" + names + "\n").str();
    }

    std::string where = SP ? fileRef(sourcePath(SP)) + ":" + std::to_string(SP->getLine()) : "?";
    probes += ("F\t" + Twine(id) + "\t" + F.getName() + "\t" + where + "\t" + Twine(known) + "\n").str();
  }

public:
  PreservedAnalyses run(Module &Mod, ModuleAnalysisManager &) {
    M = &Mod;
    i32 = Type::getInt32Ty(Mod.getContext());
    i64 = Type::getInt64Ty(Mod.getContext());
    SmallVector<Function *> todo;
    for (Function &F : Mod)
      if (!F.isDeclaration() && !F.getName().starts_with("__ksee_"))
        todo.push_back(&F);
    createRuntime();
    for (Function *F : todo)
      instrument(*F);

    std::string head = "U\n";
    for (auto &[path, k] : files)
      head += "P\t" + std::to_string(k) + "\t" + path + "\n";
    emitCustomSection(Mod, "ksee.probes", head + probes);
    return PreservedAnalyses::none();
  }
};

} // namespace

extern "C" LLVM_ATTRIBUTE_WEAK PassPluginLibraryInfo llvmGetPassPluginInfo() {
  return {LLVM_PLUGIN_API_VERSION, "ksee-trace", "0.3", [](PassBuilder &PB) {
            PB.registerOptimizerLastEPCallback(
                [](ModulePassManager &MPM, OptimizationLevel, ThinOrFullLTOPhase) {
                  MPM.addPass(KseeTrace());
                });
          }};
}
