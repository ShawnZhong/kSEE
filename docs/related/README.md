# Related work: Linux on WebAssembly

Research notes from before kSEE had a kernel (September 2026), kept for
their analysis of each system; where they say what kSEE "plans" or
"should" do, see Outcome below and [../kernel.md](../kernel.md) for what it
does. One document per system, written against the source snapshots listed below
(local clones lived in the gitignored `refs/`; re-clone at these commits to
re-check a claim).

| System | Doc | What is Wasm | Kernel | Task model | Source | Fork base |
|---|---|---|---|---|---|---|
| **tombl/linux** | [tombl-linux.md](tombl-linux.md) | kernel + user | 7.1.5, arch/wasm, NOMMU, SMP (runtime `ncpus=1` for UP), PREEMPT_NONE | Worker per task_struct; `__switch_to` hands `thread_info.running_cpu` (CPU token) via atomics | GPL kernel + MIT host (`tombl/distro`) | **5/5** |
| Joel Severin linux-wasm | [joelseverin-linux-wasm.md](joelseverin-linux-wasm.md) | kernel + user | 7.0, arch/wasm, NOMMU, SMP forced | Worker per task, but one CPU pinned per user task; handoff relayed via JS main thread | GPL; patched LLVM 18 | 4/5 |
| LinuxOnTab 2.0 | [linuxontab.md](linuxontab.md) | kernel + user | 6.1.132, downstream of tombl `wasm-6.1` | same as tombl + Asyncify'd userspace for fork/vfork | kernel not public; host JS readable | 2/5 (reference for fork/net/persistence) |
| HardwareJS / brintOS | [hardwarejs-brintos.md](hardwarejs-brintos.md) | kernel + user | 6.12, arch/wasm32, **MMU=y** (software), SMP | kernel Worker + pool Workers; coroutinized kernel & user (LLVM fork) | closed; analyzed from public binaries | 1/5 (reference for fork & safepoint preemption) |
| WasmLinux (okuoku) | [wasmlinux-lkl.md](wasmlinux-lkl.md) | LKL library kernel + user | 6.1 LKL, NOMMU, UP | host thread per task, semaphore handoff under a CPU lock | no license files; dormant since 2024 | 1/5 |
| v86, JSLinux, WebVM, container2wasm, QEMU-wasm | [emulators.md](emulators.md) | CPU emulator | unmodified guest | Worker = vCPU | various | n/a (ground-truth baseline) |

## Outcome

The notes recommended forking tombl/linux. kSEE instead wrote its own
minimal `arch/wasm` on Linux 7.2 ([../kernel.md](../kernel.md)), on
**tombl's execution model**: one Worker per `task_struct`, one shared kernel
`WebAssembly.Memory`, a CPU token handed over in `__switch_to`, NOMMU,
`PREEMPT_NONE`, stock LLVM. A port of its own keeps it small (one CPU from
the start, no section allowlist to maintain, nothing to rebase onto a
moving fork), and tracing needed no tracepoints or BTF after all: an LLVM
pass records everything ([../recording.md](../recording.md)).

Still ideas for later: userspace-only Asyncify fork (LinuxOnTab), safepoint
preemption (HardwareJS), emulator runs of the same programs as a
differential oracle (emulators).

## Source snapshots

| Repo | Branch | Commit | Date |
|---|---|---|---|
| https://github.com/tombl/linux | wasm | `f06dad0dcdfbc8b48309093c9abb68ba47502b7f` | 2026-08-16 |
| https://github.com/tombl/distro | main | `7627e66751f9b14a187e4a40e18c8434be262b2e` | 2026-08-16 |
| https://github.com/joelseverin/linux | wasm-7.0 | `039e5f3e583f56f329657d1fe9945510dba10f41` | 2026-05-14 |
| https://github.com/joelseverin/linux-wasm | master | `3103d5c877a8f6251b4b451c866a46941fd1fab3` | 2026-05-14 |
| https://github.com/kilian-ai/linuxontab | main | `0ec067caaca65f2d51cd076dabde5a4fdd3c1856` | 2026-09-10 |
| https://github.com/okuoku/wasmlinux-project | master | `405ac2dbd6acef74cb10591be01e164f4a5d91bb` | 2024-10-20 |
| https://github.com/okuoku/wasmlinux-runner | master | `67b75f18d11de4eaacf4cd14e85700d22d06ddbb` | 2024-10-20 |
| https://github.com/okuoku/lkl-wasm | wasm | `3520da9491b96bb54c73b79b0a95f575f149d565` | 2024-01-28 |
| https://github.com/okuoku/wasmlinux-musl | wasm | `f6bd94f8d03f8e41dc817b44a6ac981e0280b4f0` | 2023-12-08 |

Binary-only artifacts (no public source): LinuxOnTab 2.0 site assets from
https://linuxontab.com/2.0/ and brintOS image
`brintos/linux-6.12-musl-busybox` (hardwarejs 0.1.0-dev.3) from
https://brintos.io, fetched 2026-09-24.
