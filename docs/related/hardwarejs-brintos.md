# HardwareJS / brintOS: Linux 6.12 arch/wasm32 with a coroutinizing LLVM fork

Reference notes for kSEE's fork decision. Written 2026-09-24.

**Source status: the source code is NOT publicly available.** The links on hardwarejs.com
(`https://brintos.io/brintos/{linux,hardwarejs,hwjs-cc}`, `/brintos/llvm-project`) all return 404 ("No such
machine"), and `git ls-remote` fails on them. brintos.io is a SaaS "machine repo" host, not a git forge. The
kernel's `producers` section names `git@gitlab.com:brintos/toolchain/llvm-project.git`. GitLab reports the
`brintos` group as not found (it is private), and cloning asks for credentials. There is no GitHub org
(`api.github.com/orgs/brintos` returns 404), and the npm package `@brintos/hardwarejs` is not published. Web
searches found no mirror. The docs refer to a private superproject, `linux-in-the-browser`, with the kernel
at `packages/modified/linux`.

**What I could verify:** the public machine images on brintos.io can be read anonymously
(`/api/repos/<owner>/<repo>/fs/tree?snapshot=latest` and `/<owner>/<repo>:latest/raw/<path>`), and the
official runtime is a public static asset. I downloaded these, parsed them, and saved them to
`refs/brintos-artifacts/`:
- `vmlinux-6.12.0-wasm32-0.1.0-hwjs-0.1.0-dev.3.wasm` (21.5 MB). It comes from `brintos/linux-6.12-musl-busybox`,
  `/boot`, published 2026-09-21. It includes a **full `name` section of 70,470 function names**, plus imports,
  exports and custom sections.
- `/bin/busybox` (7.2 MB wasm, musl), plus `/etc/inittab` and `/etc/init.d/rcS`.
- `hardwarejs.js` 0.1.0-dev.3 (930 KB, minified esbuild bundle that keeps function names;
  `release.json` sourceCommit `6efb77b5`).
- `vmlinux.info`, `vmlinux.names` and `busybox.info` are dumps made by `wasmparse.py`, a small parser in the
  same folder. `spec/*.txt` holds text snapshots of the hardwarejs.com spec pages and the brintos.io
  blog and docs pages.

Evidence labels used below:
- **[bin]**: verified from the shipped wasm binaries (imports, exports, names, strings, sections).
- **[js]**: verified from the shipped hardwarejs.js bundle.
- **[docs]**: taken from the hardwarejs.com spec or brintos.io docs and blog. This is vendor material and
  has not been checked against code.
- **[inferred]**: my conclusion.

---

## 1. Summary

| Item | Value |
|---|---|
| Sites | https://hardwarejs.com/ is the "open technical specification", snapshot 2026-07-24 (pages: /linux/, /hardwarejs/, /toolchain/, /contracts/, /status/). https://brintos.io is the commercial product: machines in the browser, Wabuntu, VPS, and similar. |
| Blog | "Why we compiled real Linux to WebAssembly" (2026-05-31, updated June 2026), https://brintos.io/blog/why-real-linux-on-wasm |
| Author / owner | **brintOS, Inc.** ("Developed and funded by brintOS, Inc."). The brintos org shows 11 people and was created Jun 2026. Visible users include jdbrinton (probably the founder, J. D. Brinton), alex, sergei, dmitry and kent. The kernel banner's builder is `joel@DESKTOP-2P7Q9MT` [bin]. Nothing suggests this is Joel Severin. |
| License | **Unclear, and no public source.** Toolchain page [docs]: "It's all open source". It says musl is MIT, glibc is LGPL, the wrapper drivers are MIT, and binfmt_wasm is GPL "because it's part of Linux". No LICENSE for HardwareJS or hwjs-cc is visible, and hardwarejs.js has no license header [js]. The distributed vmlinux is GPL-2.0 Linux, but I found no public way to get its source. |
| Kernel base | **Linux 6.12** ("Linux / wasm32 6.12, backport, forked from upstream 6.12"). A 7.0 port and a "platform bundle 0.2" are listed as "coming soon" [docs]. Banner [bin]: `Linux version 6.12.0-wasm32-v0.1-gd8c3b9cac42a ... clang version 22.0.0git (gitlab.com:brintos/toolchain/llvm-project 626f3a58) ... # SMP 2026-09-21`. Spec snapshot pins: kernel `9d3a3566`, HardwareJS `431a0e44`, superproject `513d7b07` [docs]. |
| Versions | Kernel port 0.1.0, HardwareJS 0.1.0-dev.3, Toolchain 0.1. The kernel filename encodes the runtime version: `vmlinux-<kver>-wasm32-<portver>-hwjs-<hwver>.wasm` [docs, bin]. |
| Activity | Very active in commercial, closed development. The site build is `v0.1.18 main@82b4a9c8`. Machine images were updated 2026-09-21 to 09-24. Issue IDs go up to BRINTOS-2836. Image descriptions mention Yocto (`meta-brintos`, `core-image-brintos-*`), Python 3.12, xserver-xorg, OpenOCD and WebUSB FTDI. |
| Maturity | The most feature-complete Linux-on-wasm port I have seen. Claimed features: SMP up to 129 CPUs, fork/exec/dlopen, musl **and** glibc 2.43 with a real ld.so, bash/coreutils/busybox/dropbear/iproute2, virtio fs/net/input/console/blk, DRM framebuffer, WebUSB HCD, Web Serial, Yocto images, and an apt repo "rep.wabuntu.io". The code is heavily instrumented with "never-silent" diagnostics [js]. It still ships as `0.1.0-dev.3`. |
| Source availability | **None.** Only binaries and the prose spec. |

## 2. Toolchain (hwjs-cc)

- **Pinned LLVM 22 fork** ("brintOS LLVM fork", clang 22.0.0git, commit `626f3a582aa3` in the binaries [bin],
  `60513b8d` in the docs). It includes wasm32-hwjs coroutine, linker and machine-code patches. Stock or emsdk clang
  does not work, because LLVM pass plugins have no stable ABI, so the plugin must match the compiler exactly [docs].
- **HwjsCoroutinize** is an out-of-tree LLVM pass plugin. The mechanism [docs]:
  1. Code publishes suspension points with `HWJS_SUSPENDS` (the `hwjs.suspends.v1` annotation). Deliberate
     non-suspending carve-outs are marked `HWJS_NATIVE`.
  2. The pass computes the **transitive suspend-reachable closure** through direct calls, "compatible
     indirect targets" and host boundary imports.
  3. Each function in the closure is lowered with LLVM coroutine intrinsics and the pinned CoroSplit. Live
     values become frame-resident, and frames live in a **continuation arena in linear memory**.
  4. A **flat in-wasm trampoline** ("flat resume drive") runs child frames, so resuming does not rebuild the
     native stack depth. The states are NORMAL, DESCEND, PARK and RETURN.
  5. Frames carry typed relocation descriptors so a continuation can be materialized at a different address.
     Current fork copies at **zero relocation delta**.
  6. Operations: park, resume, reify, fork, and safepoint. Safepoints are compiler-inserted, budget-checked
     yields for "bounded preemption".
  7. It composes with native wasm EH, setjmp/longjmp across frames, alloca, and indirect dispatch. Shapes
     it cannot handle (an opaque pointer escape, an unknown cross-frame transfer) cause a **build failure**.
- **Verified in the binaries [bin]:**
  - Both vmlinux and busybox carry the custom sections `llvm.func_attr.annotate.hwjs.suspends.v1` (124 KB
    in vmlinux) and `...hwjs.native.v1`.
  - CoroSplit output shows up as `<fn>.resume` / `<fn>.destroy` pairs. The kernel has 26,812 base functions,
    and 21,828 of them (**81%**) are coroutinized. Busybox has 3,746 base functions, and 3,442 of them
    (**92%**) are coroutinized.
  - The code section is 17.9 MB for the kernel and 6.7 MB for busybox [inferred: several times larger than
    an untransformed build. The site acknowledges that "performance cost is concentrated at transformed frames"].
- **Kernel vs. userspace: both.** The docs say the kernel is compiled "coro-only", with the pass loaded
  per translation unit via `KCFLAGS=-fpass-plugin=.../HwjsCoroutinize.so`, and "The build arms and then
  verifies the pass ran". User programs are compiled to **bitcode**. `wasmld` then llvm-links the program, the
  runtime and the **libc bitcode pool**, runs the pass once over the merged module, and calls wasm-ld [docs].
  The drivers are `wasmcc`, `wasmcc-glibc`, `wasmld`, `wasmar` and `wasmrun`. The triples are
  `wasm32-hwjs-linux-musl`, `-linux-gnu` and `wasm32-hwjs-unknown`. Autotools packages build unmodified with `CC=wasmcc`.
- Libc: musl 1.2 (arch/wasm32 is about 1,900 LOC plus about 2,050 LOC of `src_overrides`) and glibc 2.43
  (the first glibc with experimental clang support) [docs].
- The runtime is embedded in the kernel [bin]. vmlinux carries `hwjs.rt.so` and `hwjs.rt4.so`, which are
  64 KB wasm side modules (dylink.0) for a single-lane and a 4-lane coroutine runtime, and a
  `hwjs.rt.provenance` record (`commit=d8c3b9ca dirty=0 src=<sha256>`). The host extracts these for dynamic libc
  processes (`extractHwjsRt`, `imageImportsHwjsRt` [js]).
- Target features [bin]: atomics, bulk-memory, multivalue, mutable-globals, nontrapping-fptoint,
  reference-types, sign-ext and call-indirect-overlong. There is **no SIMD, no exception-handling feature
  flag, and no multi-memory** in this build. The toolchain page's claim that wasmcc uses multi-memory
  (importing kernel_memory and user_memory as a pair) is out of date: the spec calls that design "historical",
  and busybox imports only `env.user_memory`.
- No Emscripten, Binaryen/Asyncify or JSPI [docs]. The binaries agree: no asyncify exports [bin].

## 3. arch/wasm32 structure & Kconfig

The arch tree has not been read (no source). Everything below comes from the binaries and the docs.

- **Arch name `wasm32`** (a wasm64 profile is "planned"). The files named in the spec are
  `arch/wasm32/{mm/, configs/wasm32_defconfig, include/asm/{uaccess.h,bootinfo.h,syscall.h,barrier.h}, kernel/{head.c,setup.c,smp.c,
  process.c,signal.c,syscall_table.c,entry.S}}`, plus `binfmt_wasm.c`, `scripts/link-vmlinux-wasm.sh`, and
  `Documentation/wasm/` [docs].
- **MMU: CONFIG_MMU=y, with no hardware enforcement** [bin + inferred]. vmlinux contains `handle_mm_fault`,
  `do_anonymous_page`, `pgd_alloc`, `__pte_alloc`, `copy_page_range`, `dup_mmap` and `do_mmap`, and none
  of the mm/nommu.c symbols. The spec says "Linux still maintains its page allocator and software page tables,
  but WebAssembly supplies no hardware MMU". There is a flat `ZONE_NORMAL` with 4 KiB pages. User memory is
  **a separate shared WebAssembly.Memory per process**, and the kernel reaches it only through host
  usercopy imports (§6). Isolation comes from "WASM instances as the MMU".
- **SMP: yes.** `# SMP` appears in the banner [bin]. `__cpu_up`, `smp_call_function_many`,
  `arch_send_call_function_ipi_mask`, `arch_smp_send_reschedule`, `wasm_ipi_init`, `wasm_ipid` and
  `wasm32_ipi_kick` are all present [bin]. The spec's SMP claim for spinlocks and RCU "with full upstream
  semantics" is [docs] only.
- **NR_CPUS = 129** (CPU 0 plus up to 128 pool CPUs). Evidence [js]: "num_vcpu must be <= 129
  (CONFIG_NR_CPUS compile-time ceiling: 128 pool Workers + CPU 0)". The default topology follows
  hardwareConcurrency and is capped at 8 vCPUs total [docs].
- **Preemption: PREEMPT_NONE, HZ=100** [docs]. This matches the binary: the banner has no `PREEMPT`,
  there is no `preempt_schedule` or `preempt_schedule_irq`, and `__cond_resched` is present [bin].
  "Bounded preemption" comes from compiler safepoints plus a budget (`__hwjs_budget`, `__hwjs_budget_set`)
  and a tick (`wasm32_tick_req`, `wasm_tickd`, `wasm32_tick_dispatch_catchup`). The kernel's preemption model
  is not involved [bin/js].
- Other config [docs, partly bin]: `CONFIG_MODULES=n`. Syscall numbers follow **x86_64**. Enabled:
  tmpfs/devtmpfs/proc/sysfs/FUSE/virtio-fs, virtio console/net/input, DRM/fbdev, the IPv4/IPv6 stack, Unix
  sockets, USB core plus FTDI, and UTS/IPC namespaces. The spec says the following are **off**: swap, block,
  cgroups, audit, PID/user/net namespaces, KASAN, lockdep and ftrace. The dev.3 binary partly disagrees
  (spec drift): it **does** contain `blk_mq_*` (165), `ext4_*` (887) and `wasm_virtblk_register`. It has no
  `trace_event_raw_event_*`, `perf_*`, `kprobe*`, `ftrace*`, `cgroup_*` or `lock_acquire`, so **there is
  no tracing infrastructure** [bin]. It has 58 `bpf_*` symbols, probably socket filters only [inferred].
- Kernel command-line parameters found in strings [bin]: `wasm_virtblk=`, `wasm_virtcon=`, `wasm_virtnet=`,
  `wasm_virtinput=`, `wasm_virtiofs=`, `wasm_virtiofs_cache=`, `wasm_drmfb=`, `wasm_usb=`,
  `wasm_fuse_timeout=`, `wasm_parkdepth=`, `wasm_user_pin`. Console is `console=ttyWASM0`.

## 4. Boot & host runtime

- **Kernel module shape** [bin]:
  - Imports:
    - `env.kernel_memory`: shared memory, 768 to 49,152 pages, which is 48 MiB to 3 GiB.
    - `hwjs_suspend.park () -> ()`
    - `linux.dmesg`
    - `linux.raw_copy_to_user (u,k,n)->i32`, `linux.raw_copy_from_user`, `linux.raw_clear_user`
    - `linux.now_ns ()->i64`
    - `linux.cpu_relax_host`
    - `linux.atomic_wait_i32 (addr,val,timeout_ns i64)->i32`
    - `linux.virtio_kick (dev,q)`
    - `linux.mem_commit (pages)->pages`

    That is **only 10 function imports**. The module has no table import, and **no IRQ or timer imports**.
  - 118 exports. The important ones:
    - boot and syscall: `wasm_start_kernel`, `wasm_syscall`
    - current task: `__wasm32_set_current`, `__wasm32_get_current`, `__wasm32_set_cpu_id`, `__wasm32_yield`, `__wasm32_set_coop_sched`
    - coroutine runtime: `hwjs_resume`, `hwjs_done`, `hwjs_destroy`, `__hwjs_take_parked`, `__hwjs_set_parked`, `__hwjs_frame_alloc/free/publish`, `__hwjs_slice_enter/exit`, `__hwjs_task_frames`, `__hwjs_task_wake_addr`, `__hwjs_lane_*`
    - exec staging: `wasm32_user_exec_load_{begin,step}`, `wasm32_exec_image`, `wasm32_user_exec_{sglist_addr,nsegs,image_size,shebang_*}`
    - dynamic loading: `wasm32_user_dlopen_*`, `wasm32_user_libc_*`
    - rings and shared words: `wasm32_spawn_ring`, `wasm32_signal_ring`, `wasm32_pty_bridge_ring`, `wasm32_serial_bridge_ring`, `wasm32_virtio_irq_pending`, `wasm32_display_ctrl`, `wasm32_tick_req`, `__sched_wakeup_word`, `wasm32_cpu0_park`
    - process lifecycle: `__wasm32_alloc_process_task`, `task_exit_notify_parent`, `__wasm32_group_exit`, `__wasm32_migrate_pending`, and so on.
- **Boot** [docs + js]:
  1. `bootKernel({ram: PhysRam, wasmBytes, workerEntry, num_vcpu, cmdline, ...})` probes wasm features
     (threads, shared memory, bulk memory).
  2. It creates `PhysRam` (the kernel SAB) and compiles vmlinux.
  3. It writes a packed **96-byte `boot_info` v3** record into kernel RAM: magic, abi_version=3, size, flags, RAM
     base and length, cmdline, CPU count and boot CPU, timer frequency and epoch, initrd range, and an entropy seed.
  4. It calls `wasm_start_kernel(info_offset)` on the **kernel Worker (CPU 0)**. The kernel validates the record
     (`wasm_validate_boot_info`), then runs setup, memblock, CPU bring-up and `start_kernel`.
     "wasm_start_kernel never re-enters — boot ends here by construction" [js].
- **Workers** [docs + js]:
  - **CPU 0 is a dedicated kernel Worker**, where kthreads run. The kernel side has
    `__wasm32_kthread_dispatch`, `wasm32_dispatch_loop_merged`, `wasm32_cpu0_idle_park` and
    `wasm32_cpu0_handback_park_*` [bin].
  - **Pool Workers are CPUs 1..N.** Each one instantiates **its own vmlinux instance against the same
    kernel memory**. It also binds the current process's user memory and instantiates the user program in the
    same Worker, so the syscall is an in-Worker call from the user instance's `linux.syscall` to that Worker's
    kernel `wasm_syscall` (`mergedSyscallDoor`, `instantiateProcessWorkerVmlinux`) [js].
  - The JS also has a "merged CPU 0" mode, where one pool lane shares CPU 0's JS thread ("every machine is
    merged" in one diagnostic) [js]. The exact topology rules are not documented.
- **Memory growth.** `mem_commit` grows kernel_memory on demand (`X.grow(...)`) and stores the new page count into
  an exported `wasm32_committed_pages` word [js]. The machine's RAM total is kernel SAB plus one SAB per process.
- **Devices** [docs + bin]:
  - Transport: **virtio-mmio** with split rings in kernel memory. `QUEUE_NOTIFY` becomes the
    `linux.virtio_kick` import, and a TS backend walks the chain, writes the used ring, sets
    `wasm32_virtio_irq_pending` and bumps the `__sched_wakeup_word` futex. On the kernel side,
    `wasm_virtio_irqd` / `wasm_virtio_drain_irqs` are **kthreads that poll pending bits**. There are no real
    interrupts [inferred from names].
  - Backends:
    - virtio-fs, served by a TS FUSE server over a pluggable `FsView` (in production, content-addressed
      cloud storage)
    - virtio-net, with `NetShim` (ARP/DNS plus HTTP terminated into `fetch`, gated by a key file) or a
      WebSocket L3 uplink
    - virtio-input
    - virtio-blk / ext4 for local images
    - `wasmdrm`, an ABGR8888 scanout region in kernel memory that is blitted to a Canvas
    - a wasm USB HCD that sends URBs to WebUSB
    - Web Serial (`/dev/ttySER0`)
    - PTY bridge rings
- **Time.** `now_ns` is an import. Tick requests go through `wasm32_tick_req` and the `wasm_tickd` kthread,
  with `wasm32_jiffies_advance` and catch-up [bin]. It is driven by the host, not by an interrupt [inferred].
- **Node harness.** The same runtime boots under Node (`brintos.js --image x.img | --fs dir`, `--vcpus 1..128`,
  `--ram 256`) [docs].

## 5. Tasks, context switching & CPUs

- **The model is a fixed Worker pool, not a Worker per task.** "Resident tasks multiplex onto the pool instead of
  creating one permanent Worker per process" [docs]. Each pool Worker has a JS "drive loop" that enters
  runnable kernel or user continuations for bounded **slices** (`__hwjs_slice_enter/exit`), and it keeps a
  roster of resident tasks with `switches` counters [js].
- **Context switch** is `__wasm32_switch_to` [bin]. Per the blog, "Our arch/wasm32 switch_to drives it
  directly: when a task blocks, its continuation is parked and the scheduler resumes another's" [docs]. Every
  kernel path that can reach `schedule()` is coroutinized. At the switch, the current frame chain returns up
  through the trampoline into the arena ("park"), and the Worker then runs another resident's
  continuation (`hwjs_resume`). The kernel's `current` is a variable set by the host or arch
  (`__wasm32_set_current`) [bin].
- **Blocking syscalls:**
  1. The kernel path parks. The continuation goes into a slot with a generation and a witness, and
     `wasm_syscall` returns the internal token **-517** to the caller.
  2. The user-side libc recognizes -517 (`__hwjs_libc_syscall_park_abi`, `__wasm32_set_libc_park_abi`) and
     parks the user continuation too (`hwjs_suspend.park`).
  3. The Worker returns to its loop.
  4. Later a wake (device completion, a futex on `__sched_wakeup_word` / `__hwjs_task_wake_addr`, a signal
     ring entry) resumes the kernel continuation. Stale generations are refused [docs + bin + js].
  - Compatibility mode `wasm_user_pin`: the task stays pinned to its Worker and blocks it. Current glibc
    images **require** this because glibc's fast syscall path does not decode -517 [docs].
  - Otherwise, an unbounded `atomic_wait_i32` from a pool resident is a loud "coopwait VIOLATION", and
    `cpu_relax_host` busy-waits cause a "LANE DEADLOCK" diagnostic [js]. In other words, **no code may block
    the Worker's JS thread**, because it is shared by several tasks.
- **Preemption:** compiler safepoints (loop back-edges and similar) check a per-instance
  `__hwjs_budget` global. When it expires they park ("M6 safepoint"), and the host refills the budget on resume
  [js]. A 100 Hz tick sets the need for a reschedule. The kernel is PREEMPT_NONE, so all preemption points are
  cooperative and compiler-inserted.
- **Migration** of a task between pool Workers exists (`__wasm32_migrate_pending`, `hwjs-migrate`
  add-task and claim protocol, `migrate-resume`) [bin + js].
- **Threads:** `clone(CLONE_VM)` is **refused with EAGAIN** unless the image's runtime is multi-lane
  (`__hwjs_lane_cap() > 1`) or `HWJS_EXPERIMENTAL_THREAD_CLONE=1` is set [js]. Multi-lane images run threads on
  "user lanes", co-resident in a Worker or in dedicated Workers (`spawnThreadWorker`) [js].
- **Compared with kSEE's Worker-per-task + CPU token:** HardwareJS uses N Workers for M tasks and copyable
  continuations, so context switching is done in the **compiler**. kSEE uses one Worker per task, so a
  descheduled task simply blocks in `Atomics.wait` with its native stack intact, and it needs **no
  transformation** for switching. HardwareJS's model is what forces 81% of the kernel to be coroutinized
  [inferred].

## 6. Userspace & syscalls

- **Program format:** a plain wasm module plus a custom section `hwjs.abi` (8 bytes in busybox:
  `01 03 02 02 01 01 01 45`), a versioned header covering drive, parked, region, suspend, capreg and fpcast ABI
  versions and a flags byte. The flags cover: shadow stack, TLS, safepoints, dynamic, check-emitting,
  lane-addressed, and preempt-total. `preemptibility` is none, scoped or whole-program [bin + js decoder].
  Busybox imports [bin]:
  - `env.user_memory` (shared, 130 to 16,384 pages, 8 MiB to 1 GiB)
  - `linux.syscall (nr,a0..a5)->i32`
  - `hwjs_suspend.park`

  Busybox exports `_start` and about 90 `__hwjs_*` drive, park, fork, setjmp and region functions
  (`__hwjs_fork_barrier_ok`, `__hwjs_fork_child_collapse`, `__hwjs_region_save/restore`, `hwjs_sj_record`, ...).
- **Loader:** the kernel's `binfmt_wasm` (`load_wasm_binary`, `wasm32_register_binfmt`, `wasm32_parse_module_info`)
  parses the header. It stages the image into kernel memory as a scatter list (`wasm32_user_exec_sglist_addr`,
  `_nsegs`), which the host reads (`readKernelScatterImage`), compiles (with a cache: `userImageCacheKey`) and
  instantiates with the process's user memory. Shebang handling is exported [bin + js]. **ELF is not used**:
  `load_elf_binary` is absent [bin].
- **Per-process memory:** each address space is its own shared `WebAssembly.Memory` (SAB), owned by the
  process Worker and grown on demand. When growth is refused, the host reclaims idle pool Workers and retries
  [js]. The process heap, stack and DSOs all live in it.
- **Syscall path:** user `linux.syscall`, then host glue on the same Worker, then the local vmlinux instance's
  `wasm_syscall(nr,a0..a5)`. Dispatch uses generated uniform wrappers over a table with x86_64 numbers
  (`wasm32_user_syscall_dispatch`, `wasm32_syscall_table_size`). Missing numbers return -ENOSYS
  [bin + docs]. Some calls are handled by the host (a "host-RPC" / "deferred -517 route" exists) [js].
- **copy_to/from_user:** the imports `linux.raw_copy_{to,from}_user` and `raw_clear_user` are run by the host on
  the process Worker, which has both memories. They are bounds-checked and return the number of bytes not
  copied. In a kthread context (the kernel Worker) they throw [docs + js: `uaccessRefuse`]. GUP-style pinning
  uses bounce and shadow copies (`wasm32_gup_bounce_pin_fast`, `wasm32_upin_shadow_get/put`,
  `wasm32_upin_diff_copyback`) [bin].
- **fork:**
  - Real `kernel_clone` plus the arch hook `wasm_arch_clone`. The kernel publishes a request on
    `wasm32_spawn_ring`, and the host `SpawnRingConsumer` / `makeKernelSpawnHandler` then clones the
    **user memory bytes and the parked continuation**. Parent and child return different values from the same
    point [docs + bin + js: `spawnForkWorker`].
  - Stop-the-world barriers (`__hwjs_fork_stw`, `__hwjs_fork_barrier_ok`) make sure other lanes are at slice
    boundaries [bin].
  - This works only because every frame live across the fork syscall is a coroutine frame in linear memory.
- **exec:** kernel-routed and in place. The pid and Worker stay the same, and the user image, stack and
  loader records are replaced (`wasm32_inplace_exec_drop`) [docs + bin].
- **Dynamic linking:** `dlopen` of side modules that went through the same pass (`hwjs.abi` manifest,
  cross-module registries for suspends, capability summaries and fpcast adapters). glibc runs a real `ld.so`
  [docs + bin exports `__hwjs_register_*`].
- **Signals:** Linux decides delivery. `wasm32_signal_ring` plus Atomics wakes the owning Worker, and the kernel
  instance there builds the frame. `rt_sigreturn` restores the wasm32 user context. User-side dispatch is
  `__wasm_signal_dispatch_pending` in libc and delivery happens at safepoints or syscall return
  (`wasm32_sigconsume`, `wasm32_sys_sigaltstack`) [docs + bin].
- **Not available:** ptrace requests (`ptrace_request` is absent, although 18 `ptrace*` helper symbols exist)
  [bin]. There is no mmap of shared file pages across processes [inferred: separate memories make MAP_SHARED
  across processes impossible].

## 7. Changes outside arch/

No source, so the list is inferred from symbol names in vmlinux [bin + inferred]:
- **All of the kernel** is recompiled through the coroutine pass. The pass also needs `HWJS_SUSPENDS`
  annotations on generic suspension points (schedule, wait primitives). The custom section lists about 31k
  annotated function indices, which means annotations or macros were added in generic headers.
- New driver or subsystem hooks with `wasm32_`/`wasm_` names that sit in generic code:
  - pipes: `wasm32_pipe_wait`, `wasm32_pipe_wake_function`
  - tty: `wasm32_tty_read_wait`, `wasm32_tty_park_opt`, `wasm32_tty_hangup_on_session_leader_exit`, the tty bridge
  - FUSE: `wasm32_fuse_userproc_wait`, `wasm32_fuse_reply_timeout_ns`, `wasm32_fuse_red_noabort`
  - mm/gup: `wasm32_gup_bounce_pin_fast`, `wasm32_user_pin_fast`
  - workqueue/block: `wasm32_block_queue_work_on`, `wasm32_block_mod_delayed_work_on`
  - meminfo: `wasm32_adjust_reported_meminfo`
  - init: `wasm32_mount_cloud_rootfs`, `wasm32_mount_proc_sys`, `wasm32_run_init`, `wasm32_try_exec_init`,
    `wasm32_loopback_up`, `wasm32_defer_root_namespace`
- `fs/binfmt_wasm.c` and wasm32 drivers: DRM fb, USB HCD, serial, virtio-mmio glue.
- The toolchain fork affects the kernel build as well: `-fpass-plugin` on every TU, `link-vmlinux-wasm.sh`
  replacing link-vmlinux, and post-link passes that embed `hwjs.rt*.so` and provenance.

## 8. Known limitations

- **Closed source.** You cannot fork, audit or rebuild it. It ships under a commercial "machine repo" model, and
  its runtime versions are "approved" and checked against brintos.io at launch (`/api/hardwarejs/<ver>`) [docs].
- The build depends on a private LLVM 22 fork, and the plugin ABI must match exactly. The kernel is 21.5 MB
  and busybox is 7.2 MB, because the coroutine transform touches 81% of kernel functions and 92% of user
  functions.
- glibc images need the `wasm_user_pin` compatibility mode (a blocking task pins a Worker) [docs].
- Blocking the Worker thread is forbidden. Any kernel busy-wait (`cpu_relax()` is a host call and **not** a
  suspend point) or unannotated wait freezes co-resident tasks [js].
- Threads are refused (EAGAIN) unless the image uses multi-lane builds [js].
- Preemption is only as good as the safepoint coverage. The kernel is PREEMPT_NONE, and interrupts are
  simulated by polling kthreads and the tick.
- No tracing: no ftrace, tracepoints, kprobes, perf, lockdep or KASAN [bin + docs]. No loadable modules, cgroups,
  or PID/user/net namespaces [docs].
- It needs cross-origin isolation (SAB). WebUSB and Web Serial are gated by permissions [docs].
- The spec and binary disagree on some points (block/ext4 enabled, multi-memory claim), so treat the docs
  as approximate.

## 9. Relevance to kSEE

**Ideas worth borrowing (as ideas only, since there is no code to reuse):**
1. **Copyable continuations for fork.** This is the only known working answer to real `fork()` on wasm.
   For kSEE, the idea could apply to **userspace only**. A kSEE Worker-per-task kernel never needs to copy a
   *kernel* stack if fork is implemented as follows: the child's new Worker starts at `ret_from_fork` with a
   fresh kernel stack, and only the **user** continuation (the frames above `linux.syscall`) is duplicated.
   That means coroutinizing only the user program's fork-reachable call chain, which HardwareJS already does
   at link time with whole-program bitcode. With MMU=n, vfork/posix_spawn is the cheaper first step
   [inferred].
2. **Safepoint budgets for preemption.** An LLVM pass inserting budget checks at loop back-edges and function
   entries is the preemption primitive. Under Worker-per-task it becomes much simpler: an expired safepoint just
   calls into the kernel (`schedule()` / preempt path), which gives up the CPU token and `Atomics.wait`s. Nothing
   needs to unwind or park, so **no coroutine transform** is required [inferred]. This is a good later
   path to `PREEMPT` semantics.
3. **Boundary contracts:**
   - a versioned `boot_info` record
   - generation-checked rings for spawn and signal (kernel to host)
   - a single `__sched_wakeup_word` futex for device completions
   - virtio-mmio with TS backends
   - `raw_copy_*_user` imports for a separate user memory
   - a custom-section binary header plus `binfmt_wasm`

   All of these are clean and cheap to copy as designs.
4. **Observability hygiene:**
   - the full `name` section in vmlinux
   - "never-silent" attributed failures
   - provenance records embedded in the image

   These fit kSEE's source-correlated event stream, and kSEE should also keep `name`, and ideally DWARF.

**Conflicts with kSEE's plan:**
- **MMU=y with separate per-process memories** versus kSEE's MMU=n with one kernel memory.
- **SMP with an N-Worker pool that multiplexes tasks** versus Worker-per-task with a UP CPU token.
- The **whole kernel must be coroutinized** by a private LLVM fork, which inflates code about 81% of
  functions deep and adds `.resume`/`.destroy` clones. That would badly complicate source correlation
  (frames live in an arena, not on the wasm stack) and kSEE's instrumentation.
- There is no tracing subsystem to build on.
- It runs on 6.12 rather than a current kernel.

**Fork-suitability verdict: 1 / 5.** It cannot be forked: no kernel, runtime or toolchain source is public, and
the license status of HardwareJS and hwjs-cc is unknown. Even with the source, its core architecture
(coroutinized kernel, Worker pool, MMU=y, SMP, a custom LLVM fork) is the opposite of kSEE's minimal
MMU=n, UP, Worker-per-task design on stock clang. It is valuable as a **design reference** for two things
kSEE will need later, fork via user continuation copying and safepoint-based preemption, and as proof that
both are feasible at production scale.
