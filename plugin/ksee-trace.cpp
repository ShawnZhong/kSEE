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
//     see memory as it was) and the line it is on (so that stepping a line
//     lands where its first store is). Bulk writes (memcpy, memset, wider
//     stores) go to the ksee.bulk(address, length, line, from) import,
//     which logs them. A store that copies (a value loaded and stored as
//     it is, a memcpy) says where from.
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
//                         arguments: the source's parameters, up to seven, a
//                         word each, two (low, high) for one wider than 32
//                         bits (see F lines; 0 for any the optimizer took
//                         away), then its frame base (the
//                         stack pointer after its prologue: its locals in
//                         memory are at their DWARF offsets from it)
//   [fn, 4 << 28]         a return from fn, followed by the value (low,
//                         then for one wider than 32 bits high)
//   [addr, 7 << 28 | line << 4 | k]
//                         a store of 1 << k bytes at addr (k < 4), followed
//                         by the old value (low, then for 8 bytes high); k =
//                         7: a range whose old contents are not logged,
//                         followed by its length. Then, if it copies (a
//                         value loaded and stored as it is, a memcpy),
//                         where its bytes come from. line: which of its
//                         block's lines (the L line's items, "|" apart,
//                         counted from 0) the store is on
//   [lhs, 6 << 28 | line << 4 | p]
//                         a comparison deciding a branch (or a select),
//                         of lhs with the value after it (for one wider
//                         than 32 bits, then lhs's high word, rhs's); p: its predicate (LLVM's icmp's, from eq:
//                         eq ne ugt uge ult ule sgt sge slt sle); line as
//                         a store's
//   [value, 9 << 28 | line << 16 | k]
//                         variable k of the function (its V line) takes
//                         `value` here (the low word; the high one after
//                         it, for a wide one): a named variable the code
//                         keeps in a wasm local, not memory, as the debug
//                         info says it changes (a #dbg_value); line as a
//                         store's (12 bits)
//   [value, 8 << 28]      a continuation of the head before
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
//                I \t k \t function \t FILE:line
//                                     inline site k: function, inlined at FILE:line
//                R \t ring words
//                F \t fn \t name \t FILE:line \t known \t sealed \t wide
//                  (known: a bit mask of the source's parameters that call
//                  records carry; the others were optimized away. sealed:
//                  1 if only its call and returns are recorded: see below.
//                  wide: a bit mask of those that take two words)
//                V \t fn \t k \t of \t function \t name \t param \t arg
//                                     variable k of fn (its records'): a
//                                     variable of `function` (fn, or one
//                                     inlined into it; `of` its id, as
//                                     fn's), `name`, its parameter number
//                                     (0: a local); arg: if nonzero, it is
//                                     fn's parameter arg (a word of the
//                                     call record) throughout, with no
//                                     records (an inlined call's parameter
//                                     passed one of fn's)
//                L \t fn \t n \t lines: block n, the source lines it runs, in
//                  order (each once per stretch between calls), separated
//                  by ';': FILE:line (:line in the same file as the one
//                  before), then, if it is inlined, @k per function it is
//                  inlined from, innermost first (k: an I line's: the
//                  function, and where it is inlined, in the next one
//                  out); "|" where the block calls a function (which runs
//                  in between). Block 0's first line is the function's
//                  signature, where a call begins.
#include "llvm/ADT/DenseMap.h"
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

#include <algorithm>
#include <map>
#include <string>
#include <vector>

using namespace llvm;

namespace {

constexpr unsigned RingWords = 1u << 21;
constexpr unsigned CallArgs = 7; // the syscall entry's: the number and six
constexpr unsigned Values = 2 * CallArgs + 1; // a record's at most: a call's arguments (wide ones two words) and its frame base
enum Kind { KindBlock = 1, KindReturn = 4, KindCompare = 6, KindStore = 7, KindValue = 8, KindVar = 9 };

uint32_t fnv(StringRef s, uint32_t h = 2166136261u) {
  for (unsigned char c : s)
    h = (h ^ c) * 16777619u;
  return h;
}

// A function's id: its name's hash; one local to a file is told apart by
// the file's name.
uint32_t fnId(StringRef name, bool local, const Module &M) {
  return local ? fnv(name, fnv(":", fnv(M.getSourceFileName()))) : fnv(name);
}

// A scope's file, as the debug info names it (relative to the kernel
// tree, for its own), without a leading "./".
std::string sourcePath(const DIScope *S) {
  StringRef file = S->getFilename();
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
  // recording only), and one that copies (where from, after).
  Function *put[Values + 1] = {};
  Function *putStore[4] = {}, *putCopy[4] = {};
  FunctionCallee bulk;
  // Each write's line: the index of its line among its block's (see blockLines).
  DenseMap<const Instruction *, unsigned> lineOf;
  std::string probes;
  std::map<std::string, unsigned> files; // this object's, numbered
  std::map<std::string, unsigned> sites; // its inline sites ("function\tFILE:line"), numbered

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
      putStore[k] = function("__ksee_put_store" + std::to_string(1 << k), 2);
    for (unsigned k = 0; k < 4; k++)
      putCopy[k] = function("__ksee_put_copy" + std::to_string(1 << k), 3);
    bulk = import("__ksee_bulk", "bulk", FunctionType::get(vd, {i32, i32, i32, i32}, false));
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
      // Continuation pairs: [value, KindValue << 28].
      auto value = [&](Value *v) {
        w.push_back(v);
        w.push_back(B.getInt32(KindValue << 28));
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
      for (Function *F : {putStore[k], putCopy[k]})
        body(F, true, [&, F](IRBuilder<> &B, auto &w, auto value) {
          Value *addr = F->getArg(0);
          Value *old = B.CreateZExt(B.CreateAlignedLoad(IntegerType::get(C, 8 << k),
                                                        B.CreateIntToPtr(addr, PointerType::getUnqual(C)), Align(1)), i64);
          w.push_back(addr);
          w.push_back(B.CreateOr(B.getInt32(KindStore << 28 | k), B.CreateShl(F->getArg(1), 4)));
          value(B.CreateTrunc(old, i32));
          if (k == 3)
            value(B.CreateTrunc(B.CreateLShr(old, 32), i32));
          if (F == putCopy[k])
            value(F->getArg(2));
        });
    probes += "R\t" + std::to_string(RingWords) + "\n";
  }

  // The block's lines, as for an L line. (And each write's line, for
  // logStore: its index among the items, "|" apart; a write with no
  // location is on the line before it. `sig`: the function, for its entry
  // block: its signature's line comes first.)
  std::string blockLines(BasicBlock &BB, DISubprogram *sig) {
    std::vector<std::string> items;
    std::map<std::string, unsigned> seen; // (since the last call: item -> its index)
    std::string file;                     // the last item's
    unsigned lines = 0, cur = 0;
    if (sig) {
      file = fileRef(sourcePath(sig));
      items.push_back(file + ":" + std::to_string(sig->getLine()));
      seen[items.back()] = 0;
      lines = 1;
    }
    for (Instruction &I : BB) {
      // (A call's own line is the stretch's last: the line calling.)
      DILocation *L = I.getDebugLoc().get();
      if (L && L->getLine()) {
        std::string path = fileRef(sourcePath(L->getScope()));
        std::string item = path + ":" + std::to_string(L->getLine());
        // The functions it is inlined from, innermost first, each with where
        // it is inlined (a line of the next one out).
        for (DILocation *At = L; DILocation *Site = At->getInlinedAt(); At = Site) {
          DISubprogram *SP = At->getScope()->getSubprogram();
          std::string site = (SP ? SP->getName().str() : std::string("?")) + "\t" +
                             fileRef(sourcePath(Site->getScope())) + ":" + std::to_string(Site->getLine());
          item += "@" + std::to_string(sites.try_emplace(site, sites.size()).first->second);
        }
        auto [it, fresh] = seen.try_emplace(item, lines);
        if (fresh) {
          items.push_back(path == file ? item.substr(path.size()) : item);
          file = path;
          lines++;
        }
        cur = it->second;
      }
      // (Every instruction's line, a call's too: the line calling.)
      lineOf[&I] = cur;
      // (One stretch per call, empty ones too: the host counts a stretch
      // at each return into the block.)
      if (auto *CB = dyn_cast<CallBase>(&I); CB && runsProbes(*CB)) {
        items.push_back("|");
        seen.clear();
      }
    }
    while (!items.empty() && items.back() == "|")
      items.pop_back();
    std::string out;
    for (auto &i : items)
      out += (out.empty() ? "" : ";") + i;
    return out;
  }

  // The words of argument `V`: one, or two (low, high) if it is wider than
  // 32 bits.
  void words(IRBuilder<> &B, Value *V, SmallVectorImpl<Value *> &out) {
    if (V->getType()->isIntegerTy() && V->getType()->getIntegerBitWidth() > 32) {
      Value *v = B.CreateZExtOrTrunc(V, i64);
      out.push_back(B.CreateTrunc(v, i32));
      out.push_back(B.CreateTrunc(B.CreateLShr(v, 32), i32));
    } else if (V->getType()->isPointerTy()) {
      out.push_back(B.CreatePtrToInt(V, i32));
    } else {
      out.push_back(V->getType()->isIntegerTy() ? B.CreateZExtOrTrunc(V, i32) : B.getInt32(0));
    }
  }

  // Whether `F` is sealed (see the top): it loops (a block reaches one that
  // dominates it), writes no memory, and calls nothing but intrinsics.
  // (`DT`: built before any probe is added; adding calls changes no block.)
  static bool sealed(Function &F, const DominatorTree &DT, ArrayRef<Instruction *> writes) {
    if (!writes.empty())
      return false;
    for (Instruction &I : instructions(F))
      if (isa<CallBase>(I) && !isa<IntrinsicInst>(I))
        return false;
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
    Value *line = B.getInt32(std::min(lineOf.lookup(I), (1u << 24) - 1));
    // (A copy, a value loaded and stored as it is, or a memcpy: where from.
    // Not a value stored back where it was loaded from: that says nothing
    // of where it came from.)
    Value *from = nullptr;
    if (auto *S = dyn_cast<StoreInst>(I); S && isa<LoadInst>(S->getValueOperand()))
      from = cast<LoadInst>(S->getValueOperand())->getPointerOperand();
    if (from == ptr)
      from = nullptr;
    else if (auto *MT = dyn_cast<MemTransferInst>(I))
      from = MT->getRawSource();
    Value *src = from ? B.CreatePtrToInt(from, i32) : nullptr;
    if (!len && k >= 0 && src)
      B.CreateCall(putCopy[k], {addr, line, src});
    else if (!len && k >= 0)
      B.CreateCall(putStore[k], {addr, line});
    else
      B.CreateCall(bulk, {addr, len ? B.CreateZExtOrTrunc(len, i32) : B.getInt32(size), line, src ? src : B.getInt32(0)});
  }

  // A struct passed by value (byval) is copied by the backend, after this
  // pass, into the caller's stack: stores no record would have, so memory
  // as it was would be wrong where the copy lands. Make each copy here
  // instead, a memcpy (recorded as any) into a slot of the caller's own,
  // passed as a plain pointer: wasm passes a byval argument as a pointer
  // to the caller's copy anyway, so the calling convention is the same
  // (and its callees lose byval too: see run).
  void copyByval(Function &F) {
    const DataLayout &DL = M->getDataLayout();
    SmallVector<std::pair<CallBase *, unsigned>> args;
    for (Instruction &I : instructions(F))
      if (auto *CB = dyn_cast<CallBase>(&I))
        for (unsigned k = 0; k < CB->arg_size() && k < CB->getFunctionType()->getNumParams(); k++)
          if (CB->isByValArgument(k))
            args.push_back({CB, k});
    IRBuilder<> E(&*F.getEntryBlock().getFirstInsertionPt());
    for (auto [CB, k] : args) {
      Type *T = CB->getParamByValType(k);
      Align A = std::max(CB->getParamAlign(k).valueOrOne(), DL.getPrefTypeAlign(T));
      AllocaInst *slot = E.CreateAlloca(T);
      slot->setAlignment(A);
      IRBuilder<> B(CB);
      B.SetCurrentDebugLocation(CB->getDebugLoc());
      B.CreateMemCpy(slot, A, CB->getArgOperand(k), Align(1), DL.getTypeAllocSize(T));
      CB->setArgOperand(k, slot);
      CB->removeParamAttr(k, Attribute::ByVal);
    }
  }

  // Whether `V` (an i1) decides a branch or a select: directly, or
  // through the and, or and xor that combine conditions.
  static bool decides(Value *V, unsigned depth = 3) {
    for (User *U : V->users()) {
      if (isa<BranchInst>(U))
        return true;
      if (auto *S = dyn_cast<SelectInst>(U); S && S->getCondition() == V)
        return true;
      if (auto *Op = dyn_cast<BinaryOperator>(U); Op && depth && Op->getType()->isIntegerTy(1) &&
          (Op->getOpcode() == Instruction::And || Op->getOpcode() == Instruction::Or || Op->getOpcode() == Instruction::Xor) &&
          decides(Op, depth - 1))
        return true;
    }
    return false;
  }

  // Each named variable, as the code sets it (`sets`: its debug records,
  // and the instructions before which it has their values): see the top.
  // Not a parameter as it is at the call, which the call record has.
  void recordSets(Function &F, const DominatorTree &DT, uint32_t id, DISubprogram *SP, ArrayRef<Value *> params,
                  ArrayRef<std::pair<DbgVariableRecord *, Instruction *>> sets) {
    // (A debug record's value need not dominate it, as a use must: only one
    // that does.)
    std::map<std::pair<uint32_t, std::string>, unsigned> vars;
    // (A variable that only ever takes one value, where a set of it follows
    // another, surely: nothing new; only the sets no other dominates. Each
    // inline site's copy of a variable is a variable of its own.)
    std::map<std::pair<DILocalVariable *, DILocation *>, SmallVector<unsigned>> same;
    for (auto [k, s] : enumerate(sets))
      same[{s.first->getVariable(), s.first->getDebugLoc()->getInlinedAt()}].push_back(k);
    std::vector<bool> redundant(sets.size());
    for (auto &[_, ks] : same) {
      Value *v = sets[ks[0]].first->getVariableLocationOp(0);
      if (any_of(ks, [&](unsigned k) { return sets[k].first->getVariableLocationOp(0) != v; }))
        continue;
      for (unsigned k : ks)
        redundant[k] = any_of(ks, [&](unsigned j) {
          Instruction *a = sets[j].second, *b = sets[k].second;
          return a == b ? j < k : DT.dominates(a, b);
        });
    }
    // (Each set's variable: its function's id, as fnId's (one local to its
    // file by the file), and name; or none.)
    struct Set { DbgVariableRecord *R; Instruction *I; uint32_t of; StringRef function; int arg; };
    SmallVector<Set> todo;
    for (auto [n, set] : enumerate(sets)) {
      auto [R, I] = set;
      DILocalVariable *V = R->getVariable();
      DISubprogram *VS = V ? V->getScope()->getSubprogram() : nullptr;
      Value *v = R->getVariableLocationOp(0);
      if (redundant[n] || !VS || (VS == SP && V->getArg() && isa<Argument>(v)))
        continue;
      if (auto *D = dyn_cast<Instruction>(v); D && !DT.dominates(D, I))
        continue;
      StringRef fname = VS == SP ? F.getName() : VS->getName();
      uint32_t of = VS == SP ? id : fnId(fname, VS->isLocalToUnit(), *M);
      // (A parameter of an inlined call that is one of the call's own
      // arguments, which the call record has: its value, for the whole
      // inlined call, is that; no record.)
      int arg = -1;
      if (VS != SP && V->getArg())
        for (auto [k, p] : enumerate(params))
          if (p == v)
            arg = k;
      todo.push_back({R, I, of, fname, arg});
    }
    // (A variable is the call's argument k if every set of it is.)
    std::map<std::pair<uint32_t, std::string>, int> argOf;
    for (Set &t : todo) {
      auto [it, fresh] = argOf.try_emplace({t.of, t.R->getVariable()->getName().str()}, t.arg);
      if (!fresh && it->second != t.arg)
        it->second = -1;
    }
    for (Set &t : todo) {
      DILocalVariable *V = t.R->getVariable();
      Value *v = t.R->getVariableLocationOp(0);
      int arg = argOf[{t.of, V->getName().str()}];
      auto [it, fresh] = vars.try_emplace({t.of, V->getName().str()}, vars.size());
      if (it->second >= 1u << 16)
        continue;
      if (fresh)
        probes += ("V\t" + Twine(id) + "\t" + Twine(it->second) + "\t" + Twine(t.of) + "\t" + t.function + "\t" + V->getName() + "\t" +
                   Twine(V->getArg()) + "\t" + Twine(arg + 1) + "\n").str();
      if (arg >= 0)
        continue;
      // (Before a musttail call, which must stay right before its return.)
      Instruction *at = t.I;
      if (auto *C = dyn_cast_or_null<CallInst>(at->getPrevNode()); isa<ReturnInst>(at) && C && C->isMustTailCall())
        at = C;
      IRBuilder<> B(at);
      SmallVector<Value *, 2> w;
      words(B, v, w);
      Value *low = w[0];
      w.erase(w.begin());
      // (Its line: the one computing the value, if in this block; the
      // record goes before the next instruction, a later line's.)
      auto *D = dyn_cast<Instruction>(v);
      unsigned line = lineOf.lookup(D && D->getParent() == t.I->getParent() ? D : t.I);
      record(B, low, KindVar << 28 | std::min(line, (1u << 12) - 1) << 16 | it->second, w);
    }

  }

  void instrument(Function &F) {
    uint32_t id = fnId(F.getName(), F.hasLocalLinkage(), *M);
    DISubprogram *SP = F.getSubprogram();
    SmallVector<BasicBlock *> blocks;
    for (BasicBlock &BB : F)
      blocks.push_back(&BB);

    // The source's parameters: the value each has at entry (an argument, or
    // a constant the optimizer put in its place), from the debug records
    // (after optimization, arguments may no longer be the source's).
    SmallVector<Value *, CallArgs> params;
    unsigned known = 0;
    // The function's own writes to memory, and the comparisons that decide
    // its branches (before any probe is added).
    SmallVector<Instruction *> writes;
    SmallVector<ICmpInst *> compares;
    // (Where the code sets a named variable kept in a wasm local: its
    // value, and the instruction before which it has it.)
    SmallVector<std::pair<DbgVariableRecord *, Instruction *>> sets;
    for (Instruction &I : instructions(F)) {
      if (isa<StoreInst, AtomicRMWInst, AtomicCmpXchgInst, MemIntrinsic>(I))
        writes.push_back(&I);
      if (auto *C = dyn_cast<ICmpInst>(&I); C && !C->getType()->isVectorTy() && decides(C))
        compares.push_back(C);
      for (DbgVariableRecord &R : filterDbgVars(I.getDbgRecordRange())) {
        if (R.isDbgValue() && R.getNumVariableLocationOps() == 1 && !R.getExpression()->getNumElements()) {
          Value *v = R.getVariableLocationOp(0);
          if (v && !isa<UndefValue>(v) && (v->getType()->isIntegerTy() || v->getType()->isPointerTy()))
            sets.push_back({&R, &I});
        }
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

    // (Sealed: the entry block's probe only, the call. The tree is built
    // once, before any probe is added: adding calls changes no block, so it
    // stays right for the sets too.)
    DominatorTree DT(F);
    bool seal = sealed(F, DT, writes);
    for (auto [n, BB] : enumerate(blocks)) {
      if (seal && n > 0)
        break;
      IRBuilder<> B(&*BB->getFirstInsertionPt());
      // (The entry block's is the call: its arguments follow.)
      SmallVector<Value *, Values> args;
      for (Value *p : n == 0 ? params : SmallVector<Value *, CallArgs>{})
        p ? words(B, p, args) : args.push_back(B.getInt32(0));
      // (Then the frame base, where its locals are: see the Q lines.)
      if (n == 0) {
        Function *fa = Intrinsic::getOrInsertDeclaration(M, Intrinsic::frameaddress, {PointerType::getUnqual(M->getContext())});
        args.push_back(B.CreatePtrToInt(B.CreateCall(fa, {B.getInt32(0)}), i32));
      }
      record(B, B.getInt32(id), KindBlock << 28 | n, args);
      // (The entry block starts at the function's signature: entering a
      // call stops there, with its arguments, before its first statement.)
      probes += ("L\t" + Twine(id) + "\t" + Twine(n) + "\t" + blockLines(*BB, n == 0 ? SP : nullptr) + "\n").str();
    }

    // (The variables first: one set just before a return is recorded
    // before the return's record, in its call.)
    if (!seal)
      recordSets(F, DT, id, SP, params, sets);

    for (BasicBlock *BB : blocks)
      if (auto *R = dyn_cast<ReturnInst>(BB->getTerminator())) {
        // (A musttail call must stay right before its return.)
        Instruction *at = R;
        if (auto *C = dyn_cast_or_null<CallInst>(R->getPrevNode()); C && C->isMustTailCall())
          at = C;
        IRBuilder<> B(at);
        Value *ret = R->getReturnValue();
        SmallVector<Value *, 2> w;
        ret ? words(B, ret, w) : w.push_back(B.getInt32(0));
        record(B, B.getInt32(id), KindReturn << 28, w);
      }

    for (Instruction *I : writes)
      logStore(I);

    // (A comparison deciding a branch: its operands, after it; its
    // predicate and line in its tag. Not in a sealed function: its inside
    // is a box.)
    for (ICmpInst *C : seal ? SmallVector<ICmpInst *>{} : compares) {
      IRBuilder<> B(C->getNextNode());
      unsigned line = std::min(lineOf.lookup(C), (1u << 24) - 1), pred = C->getPredicate() - CmpInst::FIRST_ICMP_PREDICATE;
      // (Its operands, 32 bits each; wider, their high words after.)
      // (A signed one's narrow operands sign-extended: -1 as -1, not 255.)
      auto operand = [&](unsigned k) {
        Value *v = C->getOperand(k);
        bool narrow = v->getType()->isIntegerTy() && v->getType()->getIntegerBitWidth() < 32;
        return C->isSigned() && narrow ? B.CreateSExt(v, i32) : v;
      };
      SmallVector<Value *, 2> l, r;
      words(B, operand(0), l);
      words(B, operand(1), r);
      SmallVector<Value *, 3> rest{r[0]};
      if (l.size() > 1)
        rest.append({l[1], r[1]});
      record(B, l[0], KindCompare << 28 | line << 4 | pred, rest);
    }

    unsigned wide = 0;
    for (auto [k, p] : enumerate(params))
      if (p && p->getType()->isIntegerTy() && p->getType()->getIntegerBitWidth() > 32)
        wide |= 1 << k;
    std::string where = SP ? fileRef(sourcePath(SP)) + ":" + std::to_string(SP->getLine()) : "?";
    probes += ("F\t" + Twine(id) + "\t" + F.getName() + "\t" + where + "\t" + Twine(known) + "\t" + Twine(seal) + "\t" +
               Twine(wide) + "\n").str();
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
      copyByval(*F);
    // (No byval left: every call makes its copy itself, see copyByval.)
    for (Function &F : Mod)
      for (unsigned k = 0; k < F.arg_size(); k++)
        F.removeParamAttr(k, Attribute::ByVal);
    for (Function *F : todo)
      instrument(*F);

    std::string head = "U\n";
    for (auto &[path, k] : files)
      head += "P\t" + std::to_string(k) + "\t" + path + "\n";
    for (auto &[site, k] : sites)
      head += "I\t" + std::to_string(k) + "\t" + site + "\n";
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
