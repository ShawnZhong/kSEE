// kSEE trace plugin: probes that record the kernel's execution, for the
// host (crates/ksee-core/src/trace.rs decodes them).
//
// After optimization (so they describe the code that actually runs, and
// cannot keep the optimizer from removing code the kernel relies on being
// removed), every function gets:
//   - at the start of each basic block, a block record (the entry block's
//     is the call, with its arguments and its frame base: its locals are at
//     their DWARF offsets from it);
//   - before each return, a return record, with the value;
//   - before each store it makes (its stack slots' too: a slot is reused by
//     later calls, and a frame's locals must be undone to be seen as they
//     were), a store record, with the old value (the host undoes stores to
//     see memory as it was). Bulk writes (memcpy, memset, wider stores) go to the
//     ksee.bulk(address, length) import, which logs them.
// A function that loops, but writes no memory the host must undo and calls
// nothing (a checksum, a search: crc32c, find_next_bit) is sealed: only
// its call and its returns are recorded, not each block of each turn. (Its
// inside is then a box: what it read and what it returned.)
// Compile with -g: locations and parameters come from debug info.
//
// Probes append records to a ring in the module's own memory, defined in
// the object that defines start_kernel (wasm-ld keeps every copy of weak
// or COMDAT data): __ksee_trace = { u32 pos; u32 pad; u32 on; u32 pad;
// u32 ring[RING_WORDS]; }; the exported ksee_trace() returns its address.
// pos counts words ever written (the ring keeps the most recent), stored
// with release order after each record, so a reader that loads it sees
// whole records. Probes record only while on is nonzero: 1, everything;
// 2, stores only (the host sets it: 1 within a syscall the page asked
// for, 2 between, so that memory stays exact throughout). One writer at a time: the kernel is uniprocessor.
//
// A record is a head pair of words and continuation pairs; the second
// word's top nibble tags the pair, so a reader can start at any even offset
// (the ring may have overwritten the start of a record) and skip
// continuations until the next head:
//   [fn, 1 << 28 | n]     block n of fn; block 0 (a call) is followed by its
//                         arguments: the source's parameters, up to six,
//                         truncated to 32 bits (0 for any the optimizer took
//                         away: see F lines), then its frame base (the
//                         stack pointer after its prologue: its locals in
//                         memory are at their DWARF offsets from it)
//   [fn, 4 << 28]         a return from fn, followed by the value
//   [addr, 7 << 28 | k]   a store of 1 << k bytes at addr (k < 4), followed
//                         by the old value (low, then for 8 bytes high); k =
//                         7: a range whose old contents are not logged,
//                         followed by its length
//   [value, 8 << 28 | i]  continuation i of the head before
// (The host's runtime writes records too: stores, for its own writes into
// kernel memory.) fn is a hash of the function's name (for a
// function local to its file, of "file:name").
//
// The linked module describes its probes in a wasm custom section, which
// wasm-ld concatenates across objects (lines may repeat). Each object's
// part starts with U and its files, which the lines after it name by
// number (FILE below):
//   ksee.probes  U
//                P \t k \t path       file k
//                R \t ring words
//                F \t fn \t name \t FILE:line \t known \t sealed
//                  (known: a bit mask of the source's parameters that call
//                  records carry; the others were optimized away. sealed:
//                  1 if only its call and returns are recorded: see below)
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
#include "llvm/IR/Dominators.h"
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

constexpr unsigned RingWords = 1u << 21;
constexpr unsigned CallArgs = 6;
constexpr unsigned Values = CallArgs + 1; // a record's at most: a call's arguments and its frame base
enum Kind { KindBlock = 1, KindReturn = 4, KindStore = 7, KindValue = 8 };

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
  // The writers: a record's head pair [a, tag] and k values after it; a
  // store of 1, 2, 4 or 8 bytes at an address (the old value, read while
  // recording only).
  Function *put[Values + 1] = {};
  Function *putStore[4] = {};
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

  // Record [a, tag] and `values`.
  void record(IRBuilder<> &B, Value *a, uint32_t tag, ArrayRef<Value *> values = {}) {
    SmallVector<Value *, 2 + Values> args{a, B.getInt32(tag)};
    args.append(values.begin(), values.end());
    B.CreateCall(put[values.size()], args);
  }

  // The writers: declared, or (in the object that defines start_kernel)
  // defined, with the ring and the export that finds it.
  void createRuntime() {
    LLVMContext &C = M->getContext();
    Type *vd = Type::getVoidTy(C);
    GlobalValue *home = M->getNamedValue("start_kernel");
    bool define = home && !home->isDeclaration();
    auto function = [&](const std::string &name, unsigned n) {
      auto *T = FunctionType::get(vd, SmallVector<Type *>(n, i32), false);
      if (!define)
        return cast<Function>(M->getOrInsertFunction(name, T).getCallee());
      Function *F = hidden(Function::Create(T, GlobalValue::ExternalLinkage, name, M));
      F->addFnAttr(Attribute::NoInline);
      return F;
    };
    for (unsigned k = 0; k <= Values; k++)
      put[k] = function("__ksee_put" + std::to_string(k), 2 + k);
    for (unsigned k = 0; k < 4; k++)
      putStore[k] = function("__ksee_put_store" + std::to_string(1 << k), 1);
    bulk = import("__ksee_bulk", "bulk", FunctionType::get(vd, {i32, i32}, false));
    if (!define)
      return;

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

    // Writer F's body: while on (1: everything; 2: stores only, so memory
    // stays exact between recordings), the words `words` makes, into the ring.
    auto body = [&](Function *F, bool store, auto words) {
      IRBuilder<> B(BasicBlock::Create(C, "", F));
      BasicBlock *write = BasicBlock::Create(C, "write", F), *done = BasicBlock::Create(C, "done", F);
      Value *on = B.CreateLoad(i32, field(B, 2));
      B.CreateCondBr(store ? B.CreateICmpNE(on, B.getInt32(0)) : B.CreateICmpEQ(on, B.getInt32(1)), write, done);
      B.SetInsertPoint(write);
      SmallVector<Value *> w;
      // Continuation pairs: [value, KindValue << 28 | index].
      auto value = [&](Value *v) {
        unsigned index = w.size() / 2 - 1;
        w.push_back(v);
        w.push_back(B.getInt32(KindValue << 28 | index));
      };
      words(B, w, value);
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
    for (Function *F : put)
      body(F, false, [&](IRBuilder<> &B, auto &w, auto value) {
        w.push_back(F->getArg(0));
        w.push_back(F->getArg(1));
        for (unsigned i = 2; i < F->arg_size(); i++)
          value(F->getArg(i));
      });
    for (unsigned k = 0; k < 4; k++)
      body(putStore[k], true, [&](IRBuilder<> &B, auto &w, auto value) {
        Value *addr = putStore[k]->getArg(0);
        Value *old = B.CreateZExt(B.CreateAlignedLoad(IntegerType::get(C, 8 << k),
                                                      B.CreateIntToPtr(addr, PointerType::getUnqual(C)), Align(1)), i64);
        w.push_back(addr);
        w.push_back(B.getInt32(KindStore << 28 | k));
        value(B.CreateTrunc(old, i32));
        if (k == 3)
          value(B.CreateTrunc(B.CreateLShr(old, 32), i32));
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

  // Whether `F` is sealed (see the top): it loops (a block reaches one that
  // dominates it), writes no memory, and calls nothing but intrinsics.
  static bool sealed(Function &F, ArrayRef<Instruction *> writes) {
    if (!writes.empty())
      return false;
    for (Instruction &I : instructions(F))
      if (isa<CallBase>(I) && !isa<IntrinsicInst>(I))
        return false;
    DominatorTree DT(F);
    for (BasicBlock &BB : F)
      for (BasicBlock *S : successors(&BB))
        if (DT.dominates(S, &BB))
          return true;
    return false;
  }

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
    if (!ptr)
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
    for (Instruction &I : instructions(F)) {
      if (isa<StoreInst, AtomicRMWInst, AtomicCmpXchgInst, MemIntrinsic>(I))
        writes.push_back(&I);
      for (DbgVariableRecord &R : filterDbgVars(I.getDbgRecordRange())) {
        DILocalVariable *V = R.getVariable();
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

    // (Sealed: the entry block's probe only, the call.)
    bool seal = sealed(F, writes);
    for (auto [n, BB] : enumerate(blocks)) {
      if (seal && n > 0)
        break;
      IRBuilder<> B(&*BB->getFirstInsertionPt());
      // (The entry block's is the call: its arguments follow.)
      SmallVector<Value *, CallArgs + 1> args;
      for (Value *p : n == 0 ? params : SmallVector<Value *, CallArgs>{})
        args.push_back(p ? toI32(B, p) : B.getInt32(0));
      // (Then the frame base, where its locals are: see the Q lines.)
      if (n == 0) {
        Function *fa = Intrinsic::getOrInsertDeclaration(M, Intrinsic::frameaddress, {PointerType::getUnqual(M->getContext())});
        args.push_back(B.CreatePtrToInt(B.CreateCall(fa, {B.getInt32(0)}), i32));
      }
      record(B, B.getInt32(id), KindBlock << 28 | n, args);
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
        record(B, B.getInt32(id), KindReturn << 28, {ret ? toI32(B, ret) : B.getInt32(0)});
      }

    for (Instruction *I : writes)
      logStore(I);

    std::string where = SP ? fileRef(sourcePath(SP)) + ":" + std::to_string(SP->getLine()) : "?";
    probes += ("F\t" + Twine(id) + "\t" + F.getName() + "\t" + where + "\t" + Twine(known) + "\t" + Twine(seal) + "\n").str();
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
