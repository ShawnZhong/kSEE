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
//     stores) go to the kernel's __ksee_bulk(address, length, line, from)
//     (arch/wasm/kernel/ksee.c), which logs them in pieces. A store that
//     copies (a value loaded and stored as it is, a memcpy) says where from.
// A function that loops, but writes no memory the host must undo and calls
// nothing (a checksum, a search: crc32c, find_next_bit) is sealed: only
// its call and its returns are recorded, not each block of each turn. (Its
// inside is then a box: what it read and what it returned.)
// Compile with -g: locations and parameters come from debug info.
//
// Probes append records to the kernel's ring through its writers,
// __ksee_put and __ksee_bulk (arch/wasm/kernel/ksee.c: the ring, its
// header, and when it records).
//
// A record is its values (32-bit words), then its head, two words [a,
// tag]: the tag's top nibble says what the record is, the next how many
// values come before the head (the writer's), the rest its fields (24
// bits); so a reader walks back from the ring's end, record by record:
//   [fn, 1 << 28 | n]     block n of fn; block 0 (a call) has its
//                         arguments: the source's parameters, up to seven, a
//                         word each, two (low, high) for one wider than 32
//                         bits (see F lines; 0 for any the optimizer took
//                         away), then its frame base (the
//                         stack pointer after its prologue: its locals in
//                         memory are at their DWARF offsets from it)
//   [fn, 4 << 28]         a return from fn, its values the value (low,
//                         then for one wider than 32 bits high; none, a
//                         function that returns nothing)
//   [addr, 7 << 28 | line << 4 | k]
//                         a store of 1 << k bytes at addr (k < 4), its
//                         values the old value (low, then for 8 bytes
//                         high), then, if it copies (a value loaded and
//                         stored as it is, a memcpy), where its bytes come
//                         from. line: which of its block's lines (the L
//                         line's items, "|" apart, counted from 0) the
//                         store is on (20 bits)
//   [lhs, 6 << 28 | line << 4 | p]
//                         a comparison deciding a branch (or a select),
//                         of lhs with its value (for one wider than 32
//                         bits, then lhs's high word, rhs's); p: its
//                         predicate (LLVM's icmp's, from eq: eq ne ugt uge
//                         ult ule sgt sge slt sle); line as a store's
//   [value, 9 << 28 | line << 12 | k]
//                         variable k (12 bits) of the function (its V
//                         line) takes `value` here (the low word; the high
//                         one its value, for a wide one): a named variable
//                         the code keeps in a wasm local, not memory, as
//                         the debug info says it changes (a #dbg_value);
//                         line as a store's (12 bits)
// (The host's writes into kernel memory are logged too, through the
// kernel's ksee_bulk export; and ksee_record(1)'s Task record starts a
// recording: arch/wasm/kernel/ksee.c.) fn is a hash of the function's name (for a
// function local to its file, of "file:name").
//
// The linked module describes its code in a wasm custom section, which
// wasm-ld concatenates across objects (lines may repeat). Each object's
// part starts with U and its files, which the lines after it name by
// number (FILE below):
//   ksee.code  U
//                P \t k \t path       file k
//                I \t k \t function \t FILE:line
//                                     inline site k: function, inlined at FILE:line
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

constexpr unsigned CallArgs = 7; // the syscall entry's: the number and six
constexpr unsigned Values = 2 * CallArgs + 1; // a record's at most: a call's arguments (wide ones two words) and its frame base
// (The kinds the plugin writes; Store records are ksee.c's.)
enum class Kind : uint32_t { Block = 1, Return = 4, Compare = 6, Var = 9 };

// (The tag's bits 24-27 are the writer's: how many values the record has.)
constexpr uint32_t Fields = 24;
constexpr uint32_t tag(Kind kind, uint32_t fields = 0) {
  return (static_cast<uint32_t>(kind) << 28) | (fields & ((1u << Fields) - 1));
}

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

// Whether `V` is an integer wider than 32 bits (two words).
bool isWide(const Value *V) {
  return V->getType()->isIntegerTy() && V->getType()->getIntegerBitWidth() > 32;
}

// Where a probe for `I` goes, before it: before a return, before the
// musttail call right before it, if any (that must stay right before it).
Instruction *beforeReturn(Instruction *I) {
  if (auto *C = dyn_cast_or_null<CallInst>(I->getPrevNode()); isa<ReturnInst>(I) && C && C->isMustTailCall())
    return C;
  return I;
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
  Type *i32 = nullptr;
  // The writers: a record's values (in `values`), then its head [a, tag];
  // a write's Store records (the old bytes, read while recording only; a
  // copy's say where from).
  Function *put = nullptr, *bulk = nullptr;
  GlobalVariable *values = nullptr;
  // Each write's line: the index of its line among its block's (see blockLines).
  DenseMap<const Instruction *, unsigned> lineOf;
  std::string code;
  std::map<std::string, unsigned> files; // this object's, numbered
  std::map<std::string, unsigned> sites; // its inline sites ("function\tFILE:line"), numbered

  // A scope's file, by its number: its path as the debug info names it
  // (relative to the kernel tree, for its own), without a leading "./".
  std::string fileRef(const DIScope *S) {
    StringRef path = S->getFilename();
    path.consume_front("./");
    return std::to_string(files.try_emplace(path.str(), files.size()).first->second);
  }

  // FILE:line, `line` of a scope's file.
  std::string at(const DIScope *S, unsigned line) {
    return fileRef(S) + ":" + std::to_string(line);
  }

  // Instruction `I`'s line (see blockLines), in a field of `bits` bits.
  unsigned lineField(const Instruction *I, unsigned bits) {
    return std::min(lineOf.lookup(I), (1u << bits) - 1);
  }

  // Record `vs` and [a, tag]: the values in __ksee_values, then the call
  // (nothing runs between: the kernel is uniprocessor).
  void record(IRBuilder<> &B, Value *a, uint32_t tag, ArrayRef<Value *> vs = {}) {
    assert(vs.size() <= Values);
    for (auto [i, v] : enumerate(vs))
      B.CreateStore(v, B.CreateConstInBoundsGEP2_32(values->getValueType(), values, 0, i));
    B.CreateCall(put, {a, B.getInt32(tag), B.getInt32(vs.size())});
  }

  // The writers (arch/wasm/kernel/ksee.c, which holds the ring), declared.
  void declareRuntime() {
    Type *vd = Type::getVoidTy(M->getContext());
    auto function = [&](const std::string &name, unsigned n) {
      return cast<Function>(M->getOrInsertFunction(name, FunctionType::get(vd, SmallVector<Type *>(n, i32), false)).getCallee());
    };
    put = function("__ksee_put", 3);
    values = cast<GlobalVariable>(M->getOrInsertGlobal("__ksee_values", ArrayType::get(i32, Values)));
    bulk = function("__ksee_bulk", 4);
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
      file = fileRef(sig);
      items.push_back(at(sig, sig->getLine()));
      seen[items.back()] = 0;
      lines = 1;
    }
    for (Instruction &I : BB) {
      // (A call's own line is the stretch's last: the line calling.)
      DILocation *L = I.getDebugLoc().get();
      if (L && L->getLine()) {
        std::string path = fileRef(L->getScope());
        std::string item = path + ":" + std::to_string(L->getLine());
        // The functions it is inlined from, innermost first, each with where
        // it is inlined (a line of the next one out).
        for (DILocation *At = L; DILocation *Site = At->getInlinedAt(); At = Site) {
          DISubprogram *SP = At->getScope()->getSubprogram();
          std::string site = (SP ? SP->getName().str() : std::string("?")) + "\t" + at(Site->getScope(), Site->getLine());
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
    if (isWide(V)) {
      Value *v = B.CreateZExtOrTrunc(V, B.getInt64Ty());
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
    else if (auto *MI = cast<MemIntrinsic>(I))
      ptr = MI->getRawDest(), len = MI->getLength();
    IRBuilder<> B(I);
    Value *addr = B.CreatePtrToInt(ptr, i32);
    Value *line = B.getInt32(lineField(I, Fields - 4));
    // (A copy, a value loaded and stored as it is, or a memcpy: where from.
    // Not a copy to where it is from: that says nothing of where it came
    // from.)
    Value *from = nullptr;
    if (auto *S = dyn_cast<StoreInst>(I); S && isa<LoadInst>(S->getValueOperand()))
      from = cast<LoadInst>(S->getValueOperand())->getPointerOperand();
    else if (auto *MT = dyn_cast<MemTransferInst>(I))
      from = MT->getRawSource();
    if (from == ptr)
      from = nullptr;
    // (One of 1, 2, 4 or 8 bytes is one Store record of that size.)
    B.CreateCall(bulk, {addr, len ? B.CreateZExtOrTrunc(len, i32) : B.getInt32(size), line,
                        from ? B.CreatePtrToInt(from, i32) : B.getInt32(0)});
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
      if (it->second >= 1u << 12)
        continue;
      if (fresh)
        code += ("V\t" + Twine(id) + "\t" + Twine(it->second) + "\t" + Twine(t.of) + "\t" + t.function + "\t" + V->getName() + "\t" +
                   Twine(V->getArg()) + "\t" + Twine(arg + 1) + "\n").str();
      if (arg >= 0)
        continue;
      IRBuilder<> B(beforeReturn(t.I));
      SmallVector<Value *, 2> w;
      words(B, v, w);
      Value *low = w[0];
      w.erase(w.begin());
      // (Its line: the one computing the value, if in this block; the
      // record goes before the next instruction, a later line's.)
      auto *D = dyn_cast<Instruction>(v);
      unsigned line = lineField(D && D->getParent() == t.I->getParent() ? D : t.I, 12);
      record(B, low, tag(Kind::Var, line << 12 | it->second), w);
    }

  }

  void instrument(Function &F) {
    uint32_t id = fnId(F.getName(), F.hasLocalLinkage(), *M);
    DISubprogram *SP = F.getSubprogram();

    // The source's parameters: the value each has at entry (an argument, or
    // a constant the optimizer put in its place), from the debug records
    // (after optimization, arguments may no longer be the source's).
    SmallVector<Value *, CallArgs> params;
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
        if (!isa_and_nonnull<Argument, ConstantInt>(v) || (params.size() >= k && params[k - 1]))
          continue;
        if (params.size() < k)
          params.resize(k, nullptr);
        params[k - 1] = v;
      }
    }

    // (Sealed: the entry block's probe only, the call. The tree is built
    // once, before any probe is added: adding calls changes no block, so it
    // stays right for the sets too.)
    DominatorTree DT(F);
    bool seal = sealed(F, DT, writes);
    unsigned n = 0;
    for (BasicBlock &BB : F) {
      if (seal && n > 0)
        break;
      IRBuilder<> B(&*BB.getFirstInsertionPt());
      // (The entry block's is the call: its arguments follow, then the
      // frame base, where its locals are: see ksee.types's Q lines, the pack's.)
      SmallVector<Value *, Values> args;
      if (n == 0) {
        for (Value *p : params)
          p ? words(B, p, args) : args.push_back(B.getInt32(0));
        Function *fa = Intrinsic::getOrInsertDeclaration(M, Intrinsic::frameaddress, {PointerType::getUnqual(M->getContext())});
        args.push_back(B.CreatePtrToInt(B.CreateCall(fa, {B.getInt32(0)}), i32));
      }
      record(B, B.getInt32(id), tag(Kind::Block, n), args);
      // (The entry block starts at the function's signature: entering a
      // call stops there, with its arguments, before its first statement.)
      code += ("L\t" + Twine(id) + "\t" + Twine(n) + "\t" + blockLines(BB, n == 0 ? SP : nullptr) + "\n").str();
      n++;
    }

    // (The variables first: one set just before a return is recorded
    // before the return's record, in its call.)
    if (!seal)
      recordSets(F, DT, id, SP, params, sets);

    for (BasicBlock &BB : F)
      if (auto *R = dyn_cast<ReturnInst>(BB.getTerminator())) {
        IRBuilder<> B(beforeReturn(R));
        Value *ret = R->getReturnValue();
        // (Its value, if it returns one.)
        SmallVector<Value *, 2> w;
        if (ret)
          words(B, ret, w);
        record(B, B.getInt32(id), tag(Kind::Return), w);
      }

    for (Instruction *I : writes)
      logStore(I);

    // (A comparison deciding a branch: its operands, after it; its
    // predicate and line in its tag. Not in a sealed function: its inside
    // is a box.)
    if (!seal) {
      for (ICmpInst *C : compares) {
        IRBuilder<> B(C->getNextNode());
        unsigned line = lineField(C, Fields - 4), pred = C->getPredicate() - CmpInst::FIRST_ICMP_PREDICATE;
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
        record(B, l[0], tag(Kind::Compare, line << 4 | pred), rest);
      }
    }

    // (The parameters the call record has, and the wide ones among them.)
    unsigned known = 0, wide = 0;
    for (auto [k, p] : enumerate(params))
      if (p)
        known |= 1 << k, wide |= isWide(p) << k;
    std::string where = SP ? at(SP, SP->getLine()) : "?";
    code += ("F\t" + Twine(id) + "\t" + F.getName() + "\t" + where + "\t" + Twine(known) + "\t" + Twine(seal) + "\t" +
               Twine(wide) + "\n").str();
  }

public:
  PreservedAnalyses run(Module &Mod, ModuleAnalysisManager &) {
    M = &Mod;
    i32 = Type::getInt32Ty(Mod.getContext());
    SmallVector<Function *> todo;
    for (Function &F : Mod)
      if (!F.isDeclaration() && !F.getName().starts_with("__ksee_"))
        todo.push_back(&F);
    declareRuntime();
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
    emitCustomSection(Mod, "ksee.code", head + code);
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
