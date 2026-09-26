# Research directions

Notes from the surveys of 2026-09-25, kept here so that they are not
re-done. None of this is built; kSEE's goal stays the one in
[design.md](design.md).

## Counterfactual execution

Run the same program again with one host decision changed (a call that
fails, a crash at a store, a preemption), then align and diff the
recordings to explain what that decision caused. The machine is
deterministic, so the diff is clean. A first form of it, a per-call
diff of function counts against the example's run, was in the page for a
day (bf8b16c) and dropped as noise; the alignment of two runs is the part
worth doing properly.

Feasibility, from ext4/jbd2 fixes between June 2023 and v7.3-rc4: 284 bug
fixes; 22 need an MMU, DAX or the like; 50 are error-path fixes (45 need
call-failure injection) and 21 crash-consistency fixes (18 need crash
injection); 31 were fixed after v7.2, so those bugs may be in kSEE's
kernel unless 7.2.y backported the fixes. Classified from commit messages
by agents; 3 spot-checked, all matched.

## An adversarial machine ("Murphy")

A separate project idea, not tied to file systems: a machine as unhelpful
as the hardware contract allows, built as an in-kernel debug layer on
real architectures, or as a native virtual architecture in the shape of
arch/wasm (one thread; unlike UML, whose SMP uses threads and signals,
and LKL, which forbids SMP). Its hardware would be an executable model of
the specs: time and timers first (no existing tool covers them, and
syzbot never finds such bugs; there is no formal timekeeping spec, which
timed automata could give, next to Linux RV's hybrid automata), then
interrupts and vCPUs (an ICSE'26 study says 27.5% of concurrency bugs
need a specific interrupt), within-spec virtio devices, and a sound
subset of the memory model checked against herd7 (OZZ and KCSAN cover
memory ordering already). SVA (SOSP'07) is the precedent to cite for an
LLVM-IR architecture; Bergmann's 2018 remark is the only earlier mention.

Requirements from 16 case bugs: build both 64-bit and 32-bit; two or
three vCPUs soon; PREEMPT_RT selectable; nested interrupts at exact
points; an nbcon console and tracing; and, the biggest decision, a real
user space with fork (nft, xfs_scrub, kselftests, syzkaller), so an MMU
or its equivalent. Retrodiction targets that run on one CPU with no MMU:
the ring-buffer fix b803d7c664d and the quota/freeze fix 77449e453df.

Objective search: worst-case latency search is novel as a combination (a
replayable witness plus a blamed section, which rtla, rtsl and tlob do
not give) but fits better as one objective of a search engine than as a
project; kernel metastability (remove the trigger, test recovery, with
fork snapshots) is novel but rests on findings that do not exist yet
(0 of 20 sampled bugs). The time contract stays first. A side-channel
survey was blocked and should not be retried.

Next steps, if taken up: case studies, then the oracles (fsck on the
host, hang detection; memory errors need loads recorded, or KASAN),
before any injection is built.
