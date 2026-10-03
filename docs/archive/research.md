# Historical research notes

> These are preliminary survey notes from 2026-09-25. They are historical and
> non-authoritative. The surveys used limited validation, including only three
> spot-checks of the quantitative ext4 classification. Do not treat the counts
> or literature conclusions as established facts without reproducing the
> research and checking current sources.

These notes are kept to record what was explored, not to define kSEE's current
goal. The current design is described in [../design.md](../design.md).

## Survey observations

### Counterfactual execution and ext4

One direction surveyed was to run the same program again with one host decision
changed, then align and diff the recordings to explain what that decision
caused. Candidate changes included a failed call, a crash at a store, or a
preemption. kSEE's deterministic execution suggested that aligned differences
could be relatively clean.

A first, much smaller experiment showed per-call differences in function
counts against a lesson's normal run. It existed in the page for one day
(commit `bf8b16c`) and was removed as noise. The survey concluded that robust
alignment between two runs was the substantive problem.

The survey classified ext4/jbd2 fixes between June 2023 and `v7.3-rc4` from
commit messages, mostly with agents. It counted 284 fixes: 22 appeared to need
an MMU, DAX, or a similar unavailable feature; 50 appeared to concern error
paths, 45 of those needing call-failure injection; and 21 appeared to concern
crash consistency, 18 of those needing crash injection. It also counted 31 as
fixed after `v7.2` and therefore potentially present unless the stable series
backported them. Only three classifications were spot-checked, and all three
matched. These figures are observations from that preliminary method, not
verified project facts.

### Adversarial-machine survey

A separate project direction, nicknamed "Murphy", considered a machine as
unhelpful as the hardware contract permits. Possible forms were an in-kernel
debug layer on real architectures or a native virtual architecture shaped like
`arch/wasm`: one thread, unlike UML's thread-and-signal SMP or LKL's no-SMP
model.

The survey proposed executable hardware contracts, starting with time and
timers. It noted Linux RV's hybrid automata and suggested timed automata for a
missing formal timekeeping contract. Later layers could include interrupts and
vCPUs, within-contract virtio devices, and a sound memory-model subset checked
against herd7. OZZ and KCSAN were considered existing coverage for memory
ordering. SVA (SOSP 2007) was identified as precedent for an LLVM-IR
architecture; a 2018 remark by Bergmann was the only earlier mention found in
the survey.

Notes from 16 case bugs suggested requirements such as both 64-bit and 32-bit
builds, two or three vCPUs, selectable `PREEMPT_RT`, nested interrupts at exact
points, an nbcon console, and tracing. The largest proposed decision was a real
user space with `fork` for tools such as nft, xfs_scrub, kselftests, and
syzkaller, which would require an MMU or equivalent. Proposed one-CPU NOMMU
retrodiction targets were `b803d7c664d` and `77449e453df`.

The survey also considered worst-case latency search and kernel metastability.
It judged latency search more suitable as one objective of a broader search
engine. The metastability idea lacked supporting findings in a 20-bug sample.
A side-channel survey was blocked at the time.

## Proposals recorded by the survey

These were possible next steps, not commitments:

- Develop case studies before implementation.
- Define oracles such as host-side `fsck` and hang detection.
- Record loads or add KASAN if memory errors become a target.
- Build injection only after the cases and oracles justify it.
- Treat counterfactual alignment as the core research problem rather than
  restoring the removed function-count comparison.
