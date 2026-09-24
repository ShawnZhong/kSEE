# okuoku WasmLinux (LKL on WebAssembly)

Studied 2026-09-24 from local clones under `refs/`:

| Clone | Upstream | What it is | Head studied |
|---|---|---|---|
| `refs/okuoku-wasmlinux-project` | github.com/okuoku/wasmlinux-project | build environment: submodules, `.config`s, CMake phase scripts, toolchain wrappers | `405ac2d` 2024-10-20 |
| `refs/okuoku-lkl-wasm` (sparse: `arch/lkl`, `tools/lkl`) | github.com/okuoku/lkl-wasm, branch `wasm` (fork of github.com/lkl/linux) | patched LKL kernel | `3520da9` 2024-01-28 |
| `refs/okuoku-wasmlinux-runner` | github.com/okuoku/wasmlinux-runner | "runner": host side (C++20), kernel stubs, syscall entry point, process emulation | `67b75f1` 2024-10-20 |
| `refs/okuoku-wasmlinux-musl` (sparse: wasm32 arch dirs) | github.com/okuoku/wasmlinux-musl, branch `wasm` | musl port (`arch/wasm32`) | `f6bd94f` 2023-12-08 |

Not cloned: `okuoku/wasmlinux-busybox` (lightly patched BusyBox 1.37.0.git),
`okuoku/miniio` (I/O abstraction: libuv on PC, a localhost-only stub on the web),
`okuoku/minitelnet`, `okuoku/wasmlinux-web-assets` (demo page), and `mame/xterm-pty`.

## Summary

- **What.** "WebAssembly native" Linux: the LKL (Linux Kernel Library) kernel,
  musl and BusyBox are each compiled to wasm32 with stock Clang. No CPU is
  emulated. The demo page credits it as "World's first (maybe) WebAssembly native
  Linux system demo".
- **Author.** Takashi Oku (GitHub `okuoku`). One person, working alone.
- **Demo.** https://wasmlinux-demo.pages.dev/ (BusyBox shell through telnetd and
  xterm-pty, served with COOP/COEP). Author's blog article, in Japanese:
  https://zenn.dev/okuoku/articles/89a96bcee4cc93
- **License.** The kernel fork is GPL-2.0 (the Linux tree; GitHub reports
  NOASSERTION). musl is MIT and BusyBox GPL-2.0. `wasmlinux-project` and
  `wasmlinux-runner` have **no license file**.
- **Kernel base.** Linux **6.1.0** (the `Makefile` in lkl-wasm), on LKL
  `arch/lkl`. Config: `CONFIG_LKL=y`. `arch/lkl/Kconfig` forces `!SMP && !MMU`.
  Other settings: `PREEMPT_NONE`, `TINY_RCU`, `HZ=100`, `FLATMEM`, `SLUB`,
  `BINFMT_SCRIPT`. Networking is heavy (INET, IPv6, TCP congestion modules), and
  PCI, BPF and IO_URING are enabled. `mem=64M` is hard-coded on the command line.
- **Activity.** lkl-wasm was created 2023-10 and its last kernel commit is
  2024-01-28 ("wasm FIXME: WAR `kvfree_rcu` offset"). musl and BusyBox were last
  touched 2023-12. runner/project were last touched 2024-10-20 (a build fix). The
  project calls itself "early PoC", and the demo page says the sources are "just
  complete mess". Stars: project 88, lkl-wasm 26. **Dormant since 2024** (inferred
  from commit dates). Upstream LKL (lkl/linux) is still active and was pushed
  2026-09, but none of the wasm work went upstream.

## Toolchain

- Everything uses stock **Clang/LLVM 18**, with no patches (the config records
  `clang version 18.0.0`, Fedora snapshot). Sources are cross-compiled to
  `--target=wasm32`. There is no wasm-specific LLVM work.
- `_toolchain/bin/warp-{cc,ld,ar,nm}` are CMake-script wrappers around the host
  clang:
  - **Kernel (`KERNEL=ON`):** `--target=wasm32 -ffreestanding -Xclang
    -target-feature -Xclang +atomics`.
  - **User (`warp-hosted-cc`):** `-fPIC -nostdinc` against the musl prefix. Links
    use `-nostdlib -shared -Wl,--no-entry -Wl,--export=_start_c crt1.o -lc`. User
    programs are therefore **PIC shared wasm modules** (`__memory_base` and
    `__table_base` are imports), which lets many instances live in one memory.
- Build phases (`phases/*.cmake`):
  1. **Kernel objects and headers.**
     `make ARCH=lkl CROSS_COMPILE=wasm32 CONFIG_OUTPUT_FORMAT=wasm32
     KALLSYMS_EXTRA_PASS=1 olddefconfig`, then `make -C tools/lkl`, then
     `headers_install`. Kbuild's own link fails ("ignore error"). What is kept is
     `vmlinux.a` plus `lib/lib.a`.
  2. **musl.** `--build=wasm32 --disable-shared`. `libm` is replaced by a dummy
     stub (`dummym/dummymath.c`).
  3. **Kernel link outside Kbuild.** The kernel is linked by hand with
     `clang -nostdlib -Wl,--no-entry`. The inputs are `vmlinux.a`, the runner's
     `kmwrap/stubs/*.c` and a generated `initsyms.gen.c`, and the output is
     `kmdist/lin.wasm`. wasm-ld has no linker scripts, so everything
     `vmlinux.lds.S` would provide is faked:
     - `hostwasm_main.c` aliases every section marker to one 64 KiB `dummy_page`.
       This covers `_stext`, `__setup_start/end`, `__start___param`,
       `__start___ex_table`, `__init_begin`, the pci fixups and the rest.
       **Consequence: `__setup()` and `module_param` tables are empty**
       (command-line handlers never run).
     - `geninittbl.cmake` runs `llvm-nm` over `vmlinux.a` and collects every
       `__initcall__*` symbol. It buckets them by level suffix
       (`0 1 1s … rootfs 6 6s 7 7s`) and emits `__initcall0_start[]` …
       `__initcall_end[]` arrays, which are filled at run time by
       `host_lkl_inittbl()`. `vhci_hcd` init is held back and re-exposed as
       syscall 224 (`swapon`), a hack.
     - `initschedclasses.c` defines fixed-size `struct sched_class` slots in
       dl→rt→fair→idle order. It `memcpy`s them from `*_sched_class_temp` (a
       kernel patch renames the real ones) and aliases
       `__sched_class_highest/lowest`, because 6.1 depends on linker-script
       ordering of `sched_class` ("Pray the frontend emits struct in this
       order...").
     - `init_thread_union`/`init_stack` alias the dummy page, and
       `init_thread_info` is copied into it (`0x220` bytes, hard-coded).
  4. **BusyBox.** `busybox_unstripped` is built with `warp-hosted-cc`.
  5. **wasm2c.** `phases/phase5` runs wabt `wasm2c --enable-threads` on
     `lin.wasm` (module `kernel`) and on each user wasm. User binaries are
     replaced in the rootfs by a two-line stub
     `#!/usr/bin/env wasmlinux-id-exec\n<sha256>` and cached in
     `wasmlinux/modcache/<hash>/base.wasm`.
- **Runner.** `hostrunner` is C++20, and its CMake links `kernel.c` and the
  wasm2c user modules with `runner.cpp`. On the web it is built with
  **Emscripten**: `-pthread -sPROXY_TO_PTHREAD -fwasm-exceptions
  -sINITIAL_MEMORY=512MiB -sMALLOC=mimalloc`, plus the xterm-pty JS library. On
  PC it is a native binary using libuv. So the browser artifact goes
  **wasm → C (wasm2c) → wasm (Emscripten)**. The demo page gives the reason: this
  is "just for setjmp/longjmp", and the author planned to drop wasm2c once
  Wasm-EH-based setjmp landed. The code shows a second reason, covered under
  Tasks: wasm2c turns the module instance into a C struct that the runner can
  copy per host thread.

## How LKL is adapted

LKL background: `arch/lkl` is an in-tree architecture that turns Linux into a
library. It has no boot code, page tables, trap entry or interrupt vector. All
platform services come from a `struct lkl_host_operations` table (`lkl_ops`),
supplied by the embedding program:

- semaphores and mutexes
- host threads, their TLS and join
- `mem_alloc`
- `time`, and one-shot timers
- `jmp_buf_set`/`longjmp`
- print and panic
- virtio devices

The app calls `lkl_init(ops)`, `lkl_start_kernel(cmdline)`, and then
`lkl_syscall(no, params)` from any host thread.

WasmLinux changes, from grepping for `__wasm__` and `wasmlinux` in `arch/lkl`
(the shallow clone has no history, so this list is not exhaustive):

- **`kernel/syscalls.c`.**
  - **Syscall dispatch by argument count.** `run_syscall(no, nargs, params)`
    casts the table entry to a 0..6-argument prototype, chosen by `nargs`. Stock
    LKL always passes 6 arguments, but a wasm `call_indirect` traps on a
    signature mismatch. The runner supplies the true count from a generated
    table (`kernel_data/syscall_argc.h` plus fixups). `lkl_syscall` gained an
    `nargs` parameter.
  - **Process and thread contexts, in new functions:**
    - `wasmlinux_newtask(clone_flags)` calls
      `kernel_thread(host_task_stub, NULL, flags)`.
    - `wasmlinux_create_process_ctx()` uses `CLONE_VM|SIGCHLD`.
    - `wasmlinux_create_thread_ctx()` uses the full thread flag set.
    - `wasmlinux_create_ctx(arg, ctid, ptid)` goes through `kernel_clone()`.
    - `wasmlinux_set_ctx(ctx)` binds a `task_struct*` to the calling host
      thread's TLS key.
- **`kernel/signal.c`.** `wasmlinux_get_signal(out)` calls `get_signal()` and
  copies the `struct ksignal` out to the runner.
- **`kernel/threads.c`.** `__switch_to` gained a check: if `prev` is
  `TASK_DEAD`, it calls `lkl_ops->thread_exit()` "to reap userthread".
- **`include/asm/unistd.h`.** Section attribute strings for wasm (`,@`).
  `setup.c`: `lkl_syscall` call sites were given the argument count.
- **Kernel exports** (in the runner's `kmwrap/stubs/hostwasm_main.c`):
  - `init()`: copies `init_thread_info`, fills the initcall and sched-class
    tables, then runs `lkl_init(ops)` and `lkl_start_kernel("mem=64M")`.
  - `syscall(no, nargs, args*)` → `lkl_syscall`.
  - `taskmgmt(op, …)` with ops 1 new-process-ctx, 2 new-thread-ctx, 3 set-ctx,
    4 clone-ctx, 5 get-signal.
  - `memset`/`memcpy` are naive byte loops, and `lkl_printf` is a no-op.
- **Host ops** (`hostwasm_lklops.c`). Every op is marshalled through one import,
  `env.nccc_call64(uint64_t* in, uint64_t* out)`, with `in[0]` = module and
  `in[1]` = function. The modules are 0 admin (print/panic), 1 sync
  (sem/mutex), 2 threads (create/detach/exit/join/self/equal/gettid/TLS), 3 mem
  (alloc/free), 4 timer (time/alloc/set_oneshot/free) and 5 context
  (`jmp_buf_set`/`longjmp`). `virtio_devices`, `page_alloc`, `ioremap`, `mmap`
  and `pci_ops` are null, so there are **no block devices and no virtio**. The
  rootfs is the initramfs/rootfs that LKL creates, populated at run time (see
  below).
- **Runner implementation of the ops** (`runner.cpp`):
  - sem → `sem_t` (POSIX, because of an Emscripten issue with
    `std::counting_semaphore`), and mutex → `std::mutex` or
    `std::recursive_mutex`.
  - thread → `std::thread` (on the web, an Emscripten pthread, i.e. a Web
    Worker).
  - TLS → a `thread_local` array of 128 slots.
  - mem → `mplite` (a buddy allocator) over a pool inside the kernel's linear
    memory.
  - timer → **one dedicated `std::thread` per LKL timer** blocked in
    `condition_variable::wait_for`.
  - `jmp_buf_set/longjmp` → **C++ exceptions**: `jmp_buf_set` runs `f()` inside
    a `try`, and `longjmp` throws a tagged `guardian` object. Host object
    handles are indices into a 4096-entry table.

## Boot & runtime

1. `main()`:
   1. `wasm2c_kernel_instantiate(&the_linux)`.
   2. Grow the kernel memory by 4096 wasm pages (256 MiB). The first 2048 pages
      become the `mplite` pool (128 MiB): all LKL `mem_alloc` (kernel "RAM"),
      plus user data segments, user stacks and argv/env blocks. The next 2048
      pages are kernel thread stacks at 10 pages (640 KiB) each (the code
      comment says "160KiB"; 10 × 64 KiB is 640 KiB), which caps
      things at about 204 host threads, and stacks are never freed.
   3. `w2c_kernel_init()` → `init()` → `lkl_start_kernel`. LKL runs
      `start_kernel` and the initcalls on host threads and returns once init is
      done.
2. The runner creates two kernel pipes and a reader thread for each. These act
   as the console for stdout/stderr.
3. `startup()` plays the role of `/sbin/init`, run from the host: it runs
   BusyBox applets one by one as "processes":
   - `mkdir /tmp /proc /bin …`
   - `mknod /dev/ptmx`
   - `mount proc` and `mount devpts`
   - `busybox --install -s`
4. `thr_pinetd_main()` is a pseudo-inetd on the host. For each connection it
   creates a kernel `socketpair`, a process context and a thread running
   `telnetd -i -l /bin/sh`, and a reader thread that relays output to miniio.
   In the browser, `minitelnet` plus xterm-pty act as the client, over a
   localhost-only miniio stub.
5. There is no kernel-driven `init`, no `kernel_execve`, and no binfmt loader
   for wasm. The runner decides what runs.

## Tasks/threads & scheduling

**LKL model (upstream, unchanged here).** Every Linux task is backed by its own
host thread.

- `copy_thread()` → `lkl_ops->thread_create(thread_bootstrap)`, so kernel threads
  get a new host thread.
- Each `thread_info` holds a `sched_sem` (a host semaphore).
- `__switch_to(prev, next)`:
  1. `lkl_cpu_change_owner(next->tid)`
  2. `sem_up(next->sched_sem)`
  3. `sem_down(prev->sched_sem)`

  Or, if `TIF_SCHED_JB` is set, it longjmps out of `prev`.
- There is a single virtual CPU, `struct lkl_cpu` in `cpu.c`: a mutex, an owner
  thread, a count and a waiter semaphore. `lkl_cpu_get()`/`lkl_cpu_put()` wrap
  every entry into the kernel. IRQs that are raised while the CPU is busy are
  marked pending and run in `lkl_cpu_put()` (`run_irqs`).
- **Host threads that call syscalls** (the app's own threads) get a lazily
  created `TIF_HOST_THREAD` task. The task is `kernel_thread(host_task_stub)`
  with `copy_thread` returning early and creating no new host thread, and it is
  cached in a TLS key. `switch_to_host_task(task)` then wakes that task and
  blocks the caller on its semaphore until the Linux scheduler picks it. In
  effect, the host thread borrows the task's identity while it is in the
  kernel.

**WasmLinux on top of that:**

- **Thread mapping.** In the browser, `std::thread` is an Emscripten pthread
  (a Worker), so each kernel thread, each LKL timer, each user thread or process
  and each I/O relay is its own Worker (inferred from `-pthread
  -sPROXY_TO_PTHREAD`; the thread creation is visible in `runner.cpp`).
  **Context switch = semaphore handoff between Workers**, with `sem_t` built on
  `Atomics.wait`/`notify` over the shared memory.
- **Per-thread kernel instance.** `newinstance()`:
  1. `malloc`s a copy of the whole `w2c_kernel` instance struct.
  2. Clones the funcref table so its entries point to the copy.
  3. Sets the copy's `__stack_pointer` global to a fresh 640 KiB region.
  4. Stores it in `thread_local my_linux`.

  Memory is shared (`the_linux.w2c_memory`, one `wasm_rt_memory_t`), but wasm
  globals, above all the C shadow-stack pointer, are per thread. This is the
  wasm2c equivalent of instantiating the kernel module once per Worker against
  one shared `WebAssembly.Memory`, which is what kSEE plans to do natively.
- **Timers** are host threads that sleep and then call the LKL timer callback,
  which does `lkl_trigger_irq(timer_irq)` → `lkl_cpu_get()` and so on. The clock
  is host `system_clock` in ns.
- **User code runs outside the LKL CPU lock.** A user thread takes the CPU only
  inside `w2c_kernel_syscall`. User threads therefore run truly in parallel on
  separate Workers, whatever `NR_CPUS=1` says. The Linux scheduler arbitrates
  only in-kernel execution and wakeups, never user compute. There is no
  preemption of user code: no tick-driven reschedule at return-to-user,
  because there is no return-to-user path.

**Conceptual gap versus a conventional arch port (kSEE's `arch/wasm`).**

| | LKL / WasmLinux | Conventional arch port (kSEE plan) |
|---|---|---|
| Who owns a task's execution | host thread; kernel is a library entered via `lkl_syscall` | the Linux task; its Worker runs only when it holds the CPU token |
| User code | runs outside the kernel's CPU; parallel; invisible to scheduler | runs as the task's user half under the token; accounted, preemptible at safepoints |
| Syscall entry | host calls an exported function; the task identity is bound per host thread through TLS (`switch_to_host_task`) | user wasm calls an import → arch syscall entry on the *current* task; `pt_regs`, `ret_from_fork`, `exit_to_user_mode` exist |
| fork/exec lineage | runner fabricates tasks (`kernel_thread(host_task_stub)`) and picks what runs; no binfmt | `kernel_clone` + `ret_from_fork` → user entry; `execve` via a binfmt handler that instantiates a wasm module |
| Signals | runner polls: on `-ERESTART*` it calls `get_signal` and invokes the handler | delivered at `exit_to_user_mode` like any arch (still only at safepoints) |
| Interrupts | host timer threads raise IRQs whenever they grab the CPU | arch timer event delivered to the token holder at defined points |
| Link | Kbuild link abandoned; initcalls, sched classes and section markers faked by hand | must solve the same linker-script problem, but inside Kbuild (post-link tool or `.init_array`-style tables) |

The context-switch mechanism (task = host thread, switch = wake next and park
prev on a semaphore) is **the same pattern** as kSEE's "one Worker per task,
hand a CPU token". The difference is what sits on either side of the switch.
In LKL, only kernel-mode execution is under the token and the host program
drives everything else. In kSEE, the whole task, user part included, is
under it.

## Userspace & syscalls

- **Syscall ABI.**
  1. musl `arch/wasm32/syscall_arch.h`: `__syscallN(n, …)` packs the arguments
     into a `long[6]` and calls the import
     `env.wasmlinux_syscall32(nargs, no, args*)`.
  2. Runner `runsyscall32`:
     - intercepts `clone`, `execve` and `execveat`, which are emulated or abort;
     - otherwise overrides `nargs` with the true argument count;
     - then calls the kernel export `syscall(no, nargs, args*)`.
  3. `lkl_syscall`:
     1. `lkl_cpu_get`
     2. look up the host task through TLS, creating it if missing
     3. `switch_to_host_task`
     4. `run_syscall`
     5. `task_work_run`
     6. `lkl_cpu_put`

  - TLS: `__set_thread_area`/`__get_tp` go to the import
    `wasmlinux_tlsrw32`, which is backed by a runner `thread_local`.
  - Threads: musl `__clone` → import `wasmlinux_clone32(fn, stack, flags, arg,
    ptid, tls, ctid)`. With `CLONE_THREAD`, this does `taskmgmt(4)` (a
    `kernel_clone` of a host-stub task) and then a new `std::thread`. That
    thread gets a new kernel instance, sets TLS and `set_ctx`, and calls `fn`
    through the user module's funcref table. Process creation through clone
    `abort()`s.
- **Memory model.** There is **one linear memory for kernel and all user
  programs**, and no protection.
  - The user module's `env.memory` import returns the kernel's memory.
  - Each process instance gets its data segment (`199980` bytes, hard-coded for
    BusyBox) and a 1 MiB stack, both from the kernel's `mplite` pool.
    `__memory_base` points at that data segment, and `__stack_pointer` is per
    context.
  - User pointers are kernel pointers (NOMMU LKL uaccess is a plain copy).
  - A buggy or hostile user program can corrupt the kernel.
  - `mmap`, `mprotect`, `madvise`, `mlock`, `memfd_create`, `bpf`,
    `perf_event_open`, `seccomp` and others are forced to `sys_ni_syscall`
    (`hostwasm_nisys.c`).
- **Programs.** User wasm modules are converted with wasm2c at build time and
  **statically linked into the runner** (`glue_modules.c` contains a
  single-entry table: busybox). `wasmlinux_user_module_load` is unimplemented,
  so no new binaries can be loaded at run time. `execve` of a
  `wasmlinux-id-exec` stub always instantiates BusyBox (argv[0] selects the
  applet).
- **vfork/exec.** In the wasm2c fixup header, `vfork()` is a macro:
  1. It does `setjmp(jb)` and calls `wasmlinux_run_to_execve(&jb)`. That
     creates a process context (`CLONE_VM|SIGCHLD`), switches the calling host
     thread to a **new kernel instance and the child task**, and returns 0.
  2. The parent's code keeps running as the child until the child calls
     `execve`.
  3. `emul_execve`:
     1. reads the child pid;
     2. restores the parent's kernel instance and task;
     3. builds argv/env in the pool;
     4. spawns a new `std::thread` that instantiates a fresh BusyBox bound to
        the child task;
     5. `longjmp`s back to the vfork point, returning the child pid.
  - `execve` without a prior vfork `abort()`s ("FIXME: Replace current image").
- **No fork.** The kernel is NOMMU and has no `mmap`, and WasmLinux cannot copy
  a wasm instance's stack in any case.
- **Signals.** Only synchronous delivery exists: when a syscall returns
  `-ERESTARTSYS`, `-ERESTARTNOINTR`, `-ERESTARTNOHAND` or `-ERESTART_RESTARTBLOCK`
  (-512/-513/-514/-516), the runner calls `taskmgmt(5)` (`get_signal`) and then
  invokes the handler. There is no `sigreturn`/`ucontext` (it passes
  `ucontext = 0`), and the kernel does not restart the syscall. A blocked
  `read()` is not interrupted, which is why the demo says to press Enter
  twice.

## Limitations

- Everything is marked PoC. There are many `FIXME`s and hard-coded sizes:
  - `0x220` bytes of thread_info
  - `199980` bytes of BusyBox data
  - 640 KiB kernel stacks that leak
  - 4096 host objects and 128 TLS slots
- Known bugs listed on the demo page: pipes (`dmesg | less`, issue #4) and
  command sequencing (`ls; ls`, #3). Hangs require a page reload, and nothing
  persists.
- One program (BusyBox). No dynamic loading, `fork` or `mmap`, and no
  asynchronous signals.
- There is no memory isolation between processes, or between user and kernel.
- The web build is Emscripten plus wasm2c output, so the wasm the browser runs
  is not the wasm Clang emitted. Kernel wasm DWARF and function names do not
  survive intact (inference: wasm2c emits C with its own naming, and Emscripten
  debug info then describes the generated C). **Source-level correlation to
  kernel `.c` lines is lost.**
- The kernel is locked at 6.1 with LKL's pre-2024 arch code.
- The fake linker-script tables break `__setup`, `module_param`, `ex_table`,
  pci fixups, `_stext`/`_etext` range checks and kallsyms address ranges. All
  of these section markers are aliased to one dummy page.
- The author himself says: "I don't think WasmLinux will become actual linux
  arch". The long-term plans were static-translation memory protection for
  fork, then mmap with address translation.
- No performance numbers are published. There are two compile hops, a C++
  exception on every `jmp_buf` longjmp, a thread per timer, and a Worker per
  kernel thread (tens of Workers at boot; inferred from typical LKL behavior,
  not counted).

## Relevance to kSEE

**Directly reusable ideas (as designs; the code is not portable):**

1. **Task = thread; switch = wake next, park prev.** LKL's `__switch_to` over
   `sched_sem` is working, shipped evidence for kSEE's "one Worker per
   `task_struct` + CPU token". It already runs a real 6.1 scheduler, RCU,
   workqueues, softirqs and a TCP stack on that primitive in a browser. kSEE's
   token handoff is the same shape (`Atomics.notify` next, `Atomics.wait` self).
2. **Per-Worker kernel instance over one shared memory.** WasmLinux does it by
   copying the wasm2c instance struct, and kSEE does it by instantiating
   `vmlinux.wasm` per Worker with an imported shared `WebAssembly.Memory`. Both
   need per-instance globals (`__stack_pointer`, TLS base) and a shared
   function table layout.
3. **Linker-script substitutes.** An nm-driven initcall table generator, a
   workaround for `sched_class` ordering, and a catalogue of the section symbols
   that must exist. kSEE's `arch/wasm` meets the same list. Compare `tombl/linux`
   (`refs/tombl-linux`), which solves it inside Kbuild.
4. **Syscall argument-count fix-up.** wasm `call_indirect` needs exact
   signatures, so `sys_call_table` dispatch needs a per-syscall argc (or
   `SYSCALL_DEFINE` wrappers that take a fixed 6-long frame). kSEE hits this in
   `arch/wasm` syscall dispatch.
5. **vfork via setjmp + "run parent code as child until exec".** This is a
   workable NOMMU `vfork` semantics without stack copying. kSEE can do better,
   because each task has its own Worker.
6. **Userspace as PIC modules sharing kernel memory.** This is the simplest
   NOMMU process model (`__memory_base` per process instance). kSEE can reuse
   the idea for its minimal userspace.

**What kSEE must not inherit:**

- The library-kernel inversion: host-driven processes, no return-to-user, user
  code outside the scheduler.
- The wasm2c/Emscripten round-trip, which destroys source correlation.
- Hand-faked link tables, and 6.1.

**Fork-suitability verdict: 1/5 as a code base to fork, 4/5 as a reference.**

- **Why not fork:**
  - It is `arch/lkl`, not an arch port. kSEE's thesis needs real
    user/kernel transitions and user execution under the Linux scheduler.
  - The kernel diff is small but LKL-specific.
  - The runner is a C++ host that plays init, and its signal/exec emulation
    would have to be thrown away.
  - It has been dormant since 2024 and has no license on the runner.
- **Why read it:** it is the only public system that runs real Linux kernel C,
  compiled to wasm, with multiple Workers context-switching over shared memory
  in a browser. Its hacks map the exact problems (linking, syscall signatures,
  per-thread globals, vfork, signals) that kSEE's `arch/wasm` must solve
  properly.
