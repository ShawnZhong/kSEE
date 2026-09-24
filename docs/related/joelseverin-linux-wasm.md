# Joel Severin's linux-wasm — reference notes for kSEE

Sources read: `refs/joelseverin-linux-wasm` (HEAD `3103d5c`, 2026-05-14) and
`refs/joelseverin-linux` (branch `wasm-7.0`, HEAD `039e5f3e583f`). Paths below are
relative to the respective repo: kernel paths (`arch/wasm/...`, `fs/...`) are in
`joelseverin-linux`; runtime/build paths (`runtime/...`, `linux-wasm.sh`, `patches/...`,
`tools/...`) are in `joelseverin-linux-wasm`. **[inferred]** marks conclusions that
were not verified by running code or reading the source.

---

## 1. Summary

| Item | Value |
|---|---|
| Repos | https://github.com/joelseverin/linux-wasm (build scripts + runtime), https://github.com/joelseverin/linux (branch `wasm-7.0`), https://github.com/joelseverin/llvm (branch `wasm-18.1.2`) |
| Demo | https://joelseverin.github.io/linux-wasm/ (boots BusyBox `hush` shell in xterm.js) |
| Announcements | LKML 2025-11-01 https://lkml.iu.edu/2511.0/00248.html ; LWN https://lwn.net/Articles/1044786/ |
| Author | Joel Severin (`joel.severin@icemanor.se`), sole author of every non-upstream commit |
| License | GPL-2.0-only (the scripts and runtime in `LICENSE`; the kernel patches carry SPDX GPL-2.0-only headers) |
| Kernel base | Linux **v7.0** (upstream HEAD `028ef9c96e96`, 2026-04-12). The first public release was based on 6.4.16 |
| Activity | linux-wasm repo: 43 commits, 2025-10-31 → 2026-05-14. Kernel series rebased/committed 2026-05-14 (author dates go back to 2024-05). Development was active as of May 2026 |
| Maturity | Research prototype. Boots to a shell in the browser. Builds allnoconfig and allyesconfig. The README says all KUnit tests pass on wasm32 and wasm64. The arch commit is titled "WIP v4". The musl and BusyBox patches are labelled "NOMERGE: Hacks ... (minimal and incorrect)". The JS runtime is described as "the current JS hack" |
| Related | README "History": the port was co-evolved with Tom Bl's port (https://github.com/tombl/linux, `refs/tombl-linux`). Tom's port has virtio and device-tree support, which Joel's does not have yet |

Kernel patch series on top of v7.0 (`git log 028ef9c96e96..HEAD`, oldest first):
`f3b12bc0db69` net printk fmt fix · `b864072b7316` net_test SKB align · `ab8abc15e32b`
damon ops check · `89fed48b26dd` kunit skip NULL-deref on NOMMU · `9530750df7d9`
asm-generic/barrier.h include · `a3e1177bea0a` arch-specific panic hook · `e9628544499a`
NOMERGE kbuild mk_elfconfig/modpost · `bd0389f64430` **WIP v4: wasm: Add support for
Wasm** · `c94192c4083b` binfmt_wasm · `039e5f3e583f` hvc_wasm console. In total:
99 files, +4364/−28.

## 2. Toolchain

- **A patched LLVM is required.** `linux-wasm.sh` (`fetch-llvm`) clones
  `joelseverin/llvm` branch `wasm-18.1.2` (base LLVM 18.1.2) and builds `clang;lld` plus
  `compiler-rt` with `LLVM_TARGETS_TO_BUILD=WebAssembly`. It then builds compiler-rt
  builtins a second time for wasm64. The README describes the patches as:
  (a) "a hack patch that enables GNU ld-style linker scripts in wasm-ld", and (b) "various
  fixes for Wasm that makes it more compatible with normal ELF programs". The LLVM tree is
  not cloned locally, so the exact diff was **not reviewed**. `arch/wasm/kernel/vmlinux.lds.S`
  depends on (a): it uses `SECTIONS`, `. = CONFIG_KERNEL_START`, PERCPU/INIT sections and
  symbol assignment. Its comments list the hack's limits: no section alignment (it aligns
  `.` instead), no `SORT_BY_ALIGNMENT`, and **no `STABS_DEBUG`/`DWARF_DEBUG`/`ELF_DETAILS`**.
  Symbol aliases must come after placement (`jiffies = jiffies_64`).
- **Wrapper `tools/fake-llvm/llvm-wrapper.py`.** The kernel is built with
  `LLVM=<repo>/tools/fake-llvm/ REAL_LLVM=<install>/llvm/bin/ ARCH=wasm
  CROSS_COMPILE=wasm32-unknown-unknown-`, where each tool name is a symlink to the wrapper.
  The wrapper:
  - Rewrites `--target=*` to `{wasm32|wasm64}-unknown-unknown`, chosen from `-march=`. For
    `*-linux-*` targets it adds `-D__linux__ -D__unix__`, and for `-gnu` it adds `-D__gnu_linux__`.
    `scripts/Makefile.clang` gains `CLANG_TARGET_FLAGS_wasm := wasm-linux-musl`.
  - Always adds `-Xclang -target-feature +atomics +bulk-memory`, which `--shared-memory`
    needs.
  - Forces `-D__builtin_return_address=(void*)`, so `__builtin_return_address()` always
    returns 0. This breaks `_RET_IP_`/`_THIS_IP_`-based tracing and lockdep call sites.
  - Final-link `wasm-ld` flags: `-no-gc-sections --no-merge-data-segments --no-entry
    --export-all --import-memory --shared-memory --max-memory=4GiB (wasm32) / 16GiB (wasm64)
    --import-undefined` (plus `--import-table` for `-shared`). It duplicates `--start-group`
    contents because wasm-ld has no group semantics, and drops `--build-id`, `-z noexecstack`
    and `-X`.
  - `llvm-objcopy`: drops `--set-section-flags .modinfo=noload`.
- **Arch flags** (`arch/wasm/Makefile`): `-nostdlib -fno-builtin`, `-march=wasm32|wasm64`,
  and `KBUILD_LDFLAGS += -mwasm32|-mwasm64`.
- **wasm32 and wasm64** are both supported (`choice 32BIT/64BIT`, defconfigs
  `wasm32_nommu_defconfig` / `wasm64_nommu_defconfig`). wasm64 uses Memory64. The kernel side
  "works quite well", but user space for wasm64 is "quite experimental". The JS side runs
  wasm64 through a hack that truncates every host-callback argument to a JS `Number`
  (`runtime/linux-worker.js`, `init`). It therefore only works below 4 GiB.
- **Assembly** is hand-written Wasm text-in-`.S` (`.functype`, `global.get`, and so on) with
  `iLONG` macros (`arch/wasm/include/asm/asm.h`) that expand to i32/i64.
- **Userland**: musl 1.2.5 + `patches/musl/0001-NOMERGE-...patch` (2461 lines: new
  `arch/wasm`, `src/misc/wasm/syscalls.S`, setjmp/longjmp, clone, and signal trampolines).
  BusyBox 1.36.1 + `patches/busybox/0001-NOMERGE-...patch` (1846 lines, mainly
  `vfork` → `clone(CLONE_VM|CLONE_VFORK)`). User binaries are built `-fPIC -shared` (with a
  `dylink.0` section).
- Docker (`docker/linux-wasm-base`, Ubuntu 20.04). A full build means building LLVM
  (heavy). The README also notes an LLVM runtime rebuild bug: the build succeeds only on
  every other run.

## 3. arch/wasm structure

~3.34 kLOC in `arch/wasm` (82 files). The whole series is ~3.8 kLOC arch plus ~0.5 kLOC
outside.

```
arch/wasm/Kconfig, Kconfig.debug, Makefile, Kbuild
configs/  base.config wasm32.config wasm64.config allyes.config wasm{32,64}_nommu_defconfig
include/asm/  ~38 headers (atomic.h/cmpxchg.h via __atomic builtins, current.h, cpuflags.h,
              irqflags.h, page.h, ptrace.h, smp.h, syscall.h, thread_info.h, wasm.h = host imports,
              futex.h, panic.h, stacktrace.h ...)
include/uapi/asm/  ptrace.h (pt_regs/switch_stack), unistd.h, sigcontext.h, setup.h ...
kernel/  entry.S 302   head.S 95   process.c 288   smp.c 391   traps.c 210   signal.c 189
         time.c 88  setup.c 86  vmlinux.lds.S 80  sys_wasm32.c 54  cpu.c 46  asm-offsets.c 47
         reboot.c 39  irq.c 34  sys_wasm.c 31  syscall_table.c 26  random.c 21  irqflags.c 21
         ptrace.c 13  cpuflags.c 5
lib/     delay.c, multi3.c (__multi3 for 128-bit mul)
mm/      init.c (empty_zero_page, arch_zone_limits_init)
```

Key Kconfig choices (`arch/wasm/Kconfig`, `configs/base.config`):
- **MMU: none.** No `select MMU`. `PAGE_OFFSET 0`, `__va/__pa` are identity maps, and
  `PGTABLE_LEVELS 2` exists only as a stub. The README lists MMU support as future work.
- **SMP is mandatory in practice.** `config WASM` does `select SMP`, and the SMP help text
  says: "every task needs one CPU, since there is no preemption and no interrupts. If you say
  N here, you will only ever be able to run one task." `NR_CPUS` defaults to **64** (range
  1–8192). `smp_init_cpus()` marks all NR_CPUS as possible. The demo boots with `maxcpus=3`,
  and more CPUs are onlined on demand.
- **Preemption: `select ARCH_NO_PREEMPT`** (effectively PREEMPT_NONE).
- **Ticks**: `HZ=100`, `CONFIG_NO_HZ_FULL=y`, `GENERIC_CLOCKEVENTS_BROADCAST` +
  `ARCH_HAS_TICK_BROADCAST`. Demo cmdline: `nohz_full=0,2-63 rcu_nocbs=0,2-63` (CPU 1
  is the housekeeping/IRQ CPU).
- Other selects: `THREAD_INFO_IN_TASK`, `GENERIC_ENTRY`, `HAVE_SYSCALL_TRACEPOINTS`,
  `UACCESS_MEMCPY`, `ARCH_USE_QUEUED_RWLOCKS`, `GENERIC_SMP_IDLE_THREAD`,
  `HAVE_CONTEXT_TRACKING_USER`, `HAVE_VIRT_CPU_ACCOUNTING_GEN`, `ARCH_SUPPORTS_LTO_CLANG(_THIN)`,
  `ARCH_HAS_UBSAN`, and `HAVE_PAGE_SIZE_4KB/64KB`.
- **Not selected**: `HAVE_FUNCTION_TRACER`, `HAVE_DYNAMIC_FTRACE`, `HAVE_KPROBES`,
  `HAVE_ARCH_JUMP_LABEL`, `HAVE_PERF_EVENTS`, `STACKTRACE_SUPPORT`, `HAVE_EBPF_JIT`, and
  `MODULES` (no module support). Spinlocks come from generic `asm-generic/spinlock.h`
  (ticket).
- Memory knobs: `KERNEL_START=0x10000`, `BOOT_MEM_PAGES=0x2000` (Wasm 64 KiB pages, so
  512 MiB), `WASM_PAGE_SHIFT=16`, and `TASK_SIZE 0xC0000000` (cosmetic on NOMMU). The kernel
  stack is `THREAD_SIZE = PAGE_SIZE` (one page; only the C shadow stack lives there).
- The defconfig disables `KALLSYMS`, `COREDUMP`, `FILE_LOCKING`, `INOTIFY`, `IO_URING`,
  `AIO`, and `VIRTIO_MENU`.

## 4. Boot & host runtime

**Instantiation model: one `WebAssembly.Module` for vmlinux, instantiated once per Worker,
all instances importing one shared `WebAssembly.Memory`.**
- `runtime/index.html` runs `WebAssembly.compileStreaming(fetch("vmlinux-<variant>.wasm"))`
  and calls `linux(worker_url, variant, vmlinux, cmdline, initrd, ...)` in `runtime/linux.js`.
- `linux.js` creates `new WebAssembly.Memory({initial: 30, maximum: 0x10000, shared: true,
  address: 'i32'|'i64'})`. It then calls `make_cpu(0)`, which runs `make_vmlinux_runner()`:
  `new Worker(linux-worker.js)` plus a `postMessage({method:"init", vmlinux, memory, locks,
  last_task, runner_type})`.
- Each Worker (`runtime/linux-worker.js`, `init`) calls `WebAssembly.instantiate(vmlinux,
  {env:{...host_callbacks, memory}})`. Data segments are passive and `__wasm_init_memory`
  is guarded by an in-memory atomic flag, so only the first instance initialises
  .data/.bss. Later instances just get fresh Wasm globals. **Per-instance Wasm globals are
  the "registers"**: `current`, `__stack_pointer`, `__user_stack_pointer`, `__user_tls_base`
  (declared in `arch/wasm/kernel/entry.S`). `get_current()` (`asm/current.h`) is
  `global.get current`.
- Primary CPU (`runner_type:"primary_cpu"`):
  1. Posts `start_primary` with `exports.init_task`.
  2. Writes the cmdline directly into `exports.boot_command_line`.
  3. Calls `memory.grow()` to fit the initrd, copies it in, and stores `initrd_start/initrd_end`.
  4. Calls `exports._start()`.

  `_start` (`arch/wasm/kernel/head.S`) sets `current=init_task` and
  `__stack_pointer=init_stack+CPU_THREAD_STACK_START`. It records `memory_start=KERNEL_START`
  and `memory_kernel_break=memory.size*64K`, which covers vmlinux statics and the initrd. It
  then tries to `memory.grow` up to `BOOT_MEM_PAGES` (retrying smaller sizes on failure),
  sets `memory_end`, and calls `start_kernel()`. `setup_arch()` (`kernel/setup.c`) reserves
  `[memory_start, kernel_break)` in memblock, adds up to `memory_end`, caps memblock at
  0x80000000, and seeds the RNG through `wasm_random_get_bytes` (`kernel/random.c`,
  `crypto.getRandomValues`).
- Secondary CPUs: `__cpu_up()` (`kernel/smp.c`) calls the host import `wasm_start_cpu(cpu,
  idle)`. The Worker posts to the main thread, because Workers cannot spawn Workers in most
  browsers. The main thread creates a new Worker (`runner_type:"secondary_cpu"`), which calls
  `_start_secondary(idle_task)` (head.S) and then `start_secondary()` (smp.c):
  `notify_cpu_starting`, `set_cpu_online`, enable the IPI percpu IRQ,
  `complete(&cpu_running)`, `wasm_clockevent_enable()`, and `cpu_startup_entry()`.
- **Memory layout (wasm32)**: `0..0x10000` is unused (a NULL guard, though not trapping) ·
  vmlinux at `KERNEL_START=0x10000` (init/text-less data/percpu/rodata/data/bss; Wasm code
  itself is not in linear memory) · initrd · the rest is memblock/buddy up to `memory_end`.
  User program images (the file bytes), data/bss and stacks are all allocated from this same
  kernel heap through `vm_mmap` (NOMMU). **There is no separation between kernel and user
  memory.**
- **Imports from JS** (`arch/wasm/include/asm/wasm.h` + drivers): `wasm_panic`,
  `wasm_dump_stacktrace`, `wasm_start_cpu`, `wasm_stop_cpu`, `wasm_create_and_run_task`,
  `wasm_release_task`, `wasm_serialize_tasks`, `wasm_load_executable`,
  `wasm_user_mode_tail`, `wasm_cpu_clock_get_monotonic`, `wasm_random_get_bytes`,
  `wasm_driver_hvc_put`, `wasm_driver_hvc_get`. (`wasm_reload_program` and
  `wasm_clone_callback` are declared but unused **[inferred]**.) `--import-undefined` means
  any unresolved symbol silently becomes an import.
- **Exports used by JS**: `_start`, `_start_secondary`, `ret_from_fork`,
  `wasm_syscall_0..6`, `get_user_stack_pointer`, `get_user_tls_base`, `raise_exception`,
  `raise_interrupt`, `init_task`, `boot_command_line`, `initrd_start/end`
  (`--export-all`, so everything is exported).
- **Console**: `drivers/tty/hvc/hvc_wasm.c` (hvc backend, polled by khvcd).
  `wasm_driver_hvc_put` posts `console_write` to the main thread, which writes to xterm.js.
  `wasm_driver_hvc_get` posts `console_read` and **blocks the calling Worker with
  `Atomics.wait`** until the main thread copies bytes from `input_buffer` into memory.
- **Clock**: clocksource `wasm_cpu_clock` (`kernel/time.c`) reads
  `performance.timeOrigin+performance.now()` as ns (µs precision), rating 200. The
  clockevent `wasm_timer` is a per-CPU one-shot. `set_next_event` calls
  `wasm_program_timer()` (smp.c), which stores an absolute expiry in the per-CPU
  `local_timer_expiries` and does `memory.atomic.notify` on `raised_irqs`.
- **IRQs** (`kernel/irq.c`, `kernel/smp.c`): `NR_IRQS=64`, IRQ0=IPI and IRQ1=TIMER (both
  percpu_devid), on a no-op `wasm_irq_chip`. Pending IRQs are a per-CPU `atomic64_t
  raised_irqs` bitmask. `raise_interrupt(cpu, irq)` ORs the bit and notifies; JS may call
  it too. **IRQs are only taken in two places**:
  1. In `arch_cpu_idle()`. The idle task does `memory.atomic.wait64(&raised_irqs, 0,
     timeout-until-expiry)`. When the timer expires it raises `WASM_IRQ_TIMER`; then it
     xchg's the mask and dispatches each bit via `do_irq_stacked()` (traps.c), which builds
     a `pt_regs` on the stack and runs `irqentry_enter` → `generic_handle_irq` →
     `irqentry_exit`.
  2. In `run_all_irqs()`, called from `finish_arch_post_lock_switch()`
     (`asm/entry.h`), but only when `smp_processor_id()==IRQ_CPU` (=1).

  IPIs (`send_ipi_message`) are a per-CPU `raised_ipis` mask plus IRQ0. They handle
  RESCHEDULE, CALL_FUNC, RECEIVE_BROADCAST (tick broadcast from IRQ_CPU), and IRQ_WORK.
  "Interrupt enable" is a per-CPU software flag `wasm_cpuflags` (`CPUFLAGS_INTERRUPT`,
  `CPUFLAGS_USER_MODE`; `asm/cpuflags.h`, `kernel/irqflags.c`).
- **Exceptions**: a Wasm trap caught in JS (`wasm_error`) calls
  `exports.raise_exception()` (traps.c). That delivers `SIGILL` in user mode, or
  `make_task_dead(SIGILL)` in kernel mode. `panic()` calls `arch_panic_notify()` →
  `wasm_panic`, which throws a JS `Trap("panic")`.
- Hosting needs COOP/COEP (`runtime/server.py`) for SharedArrayBuffer.

## 5. Tasks, context switching & CPUs

**The model is one Worker per task_struct (kthreads, user tasks, and each CPU's idle task),
plus one Worker per CPU (the CPU's boot+idle Worker, which is the idle task).** Every
Worker has its own vmlinux instance and therefore its own Wasm call stack and globals.
Only the in-memory shadow stack (`task->stack`) and heap are in shared memory.

- `copy_thread()` (`kernel/process.c`) sets `_TIF_NEVER_RUN` and fills `pt_regs` and
  `switch_stack` (`fn`, `fn_arg`, `tls`; `uapi/asm/ptrace.h`) at the top of the child stack
  page. It then calls **`user_task_set_affinity()`**, which pins every user task to the
  first free CPU in `user_cpus`, skipping `IRQ_CPU`. It onlines that CPU with
  `cpu_device_up()` if needed, calls `set_cpus_allowed_ptr(p, cpumask_of(cpu))`, and sets
  `PF_NO_SETAFFINITY`. With no free CPU it returns `-EBUSY`, so `fork` fails; the hard limit
  is about NR_CPUS−1 live user tasks. Kthreads are not pinned. `release_thread()` frees the
  CPU slot.
- **`__switch_to(prev, next)`** (`kernel/process.c`):
  - If `next` has `_TIF_NEVER_RUN`, it clears the flag and calls
    `wasm_create_and_run_task(prev, next, comm, mm->start_code, end_code, start_data, 0)`.
    The JS side (`host_callbacks.wasm_create_and_run_task`) posts `create_and_run_task` to
    the main thread, which runs `make_task()` to create a new Worker. The current Worker
    then blocks in `serialize_me()`.
  - Otherwise it calls `wasm_serialize_tasks(prev, next)`. JS posts `serialize_tasks`, and
    **the main thread** writes `tasks[next].last_task[0]=prev` and sets
    `running` flags. It then does `Atomics.store/notify` on `next`'s private `serialize`
    lock (a per-Worker SAB `Int32Array`). The caller blocks in `serialize_me()`, which does
    `Atomics.wait(lock, 0)`.
  - When `prev` is later switched back to, its `Atomics.wait` returns and `serialize_me`
    returns `last_task[0]`. `__switch_to` then returns that as `last`. `current` and
    `__stack_pointer` never change because they are that instance's own globals.
  - **Parked tasks wait in `Atomics.wait` inside a host import called from `__switch_to`.**
    Their Wasm/JS stack is frozen in place. Every context switch round-trips through the
    main thread's event loop, because the notify is done by `linux.js`, not directly by the
    Worker **[inferred: adds main-thread latency to every switch]**.
- New task entry: the new Worker instantiates vmlinux and calls
  `exports.ret_from_fork(prev, next)` (entry.S). That sets `current=next` and
  `__stack_pointer = task_stack_page+THREAD_STACK_START`, then calls `__ret_from_fork()`
  (process.c): `schedule_tail(prev)`; runs `switch_stack->fn(fn_arg)` for kthreads;
  `syscall_exit_to_user_mode()`; and `cpuflags_overwrite()`. It returns "is clone child".
  entry.S then loads `__user_stack_pointer`/`__user_tls_base` from pt_regs/switch_stack
  and calls `_user_mode_tail` (for signal delivery). JS then instantiates the user module
  and calls `_start` or `__libc_clone_callback`.
- Task death: `release_thread()` calls `wasm_release_task`, and `linux.js` `kill_task()`
  runs `worker.terminate()`. If the dying task is still running, it sets `kill=true` and
  terminates after its final `serialize_tasks`.
- **Why SMP / one CPU per user task**: Wasm cannot suspend a running task, so a CPU-bound
  user task can never be preempted (`ARCH_NO_PREEMPT`, no tick while not idle). Giving each
  user task its own dedicated CPU makes the *browser/OS thread scheduler* do the
  multiplexing. The per-CPU serialization (the "CPU token") is only really exercised:
  - during boot, when everything runs on CPU 0;
  - between the idle task and kthreads on a CPU;
  - between a user task and its CPU's idle task. Blocking syscalls go to idle, idle
    waits on `raised_irqs`, and a wakeup IPI brings it back.
- **Ticks/preemption**: none while a task runs. A busy CPU receives no timer IRQs at all
  (the `arch_cpu_idle` comment says "best-effort fashion on CPUs where there are tasks
  running, as they cannot be stopped"). `NO_HZ_FULL` plus broadcast from IRQ_CPU keeps
  timekeeping going. `need_resched` is only acted on at syscall exit
  (`syscall_exit_to_user_mode` → `schedule()`) or in explicit `schedule()` calls.
  Kthreads must cooperate.
- **UP**: `SMP=n` is allowed by Kconfig but explicitly unsupported ("big risk you will lock
  up"). Three things break under UP **[inferred]**:
  - `IRQ_CPU=1` is hard-coded (`asm/processor.h`).
  - `user_task_set_affinity()` assumes spare CPUs.
  - IRQs would only be serviced when CPU 0 goes idle.

## 6. Userspace & syscalls

- **Format**: plain Wasm modules, **not ELF**. `fs/binfmt_wasm.c` (446 lines, based on
  binfmt_flat/elf_fdpic) accepts `\0asm` v1 whose *first* section is the custom `dylink.0`
  section. PIC is required because of NOMMU. `load_wasm_file()` does the following:
  1. `vm_mmap`s the whole file (it becomes `mm->start_code..end_code`).
  2. Parses `WASM_DYLINK_MEMINFO` (mem size/align, table size/align).
  3. Allocates `data_size` anonymous memory (`start_data`, which also holds bss).
  4. Allocates a 2-page + args stack (`MAP_GROWSDOWN`; `start_brk` at its bottom).

  `create_wasm_tables()` builds argc/argv/envp plus a fake auxv (`AT_NOTELF`, `AT_PAGESZ`,
  uid/gid, `AT_SECURE`) on a page-aligned stack. `CONFIG_BINFMT_SCRIPT` handles `#!`.
- **Loading into the host**: `start_thread()` (process.c) calls
  `wasm_load_executable(start_code, end_code, start_data, 0)`. JS runs
  `WebAssembly.compile(memory.buffer.slice(bin_start, bin_end))`, which *copies* the bytes
  out of kernel memory. `start_thread` also sets `_TIF_RELOAD_PROGRAM`. The user instance is
  created in the *same Worker* as that task's vmlinux instance. Its imports are:
  - the shared `memory`;
  - `__memory_base = data_start` and `__table_base = table_start` (always 0);
  - a fresh `__stack_pointer` Global;
  - a private `__indirect_function_table` (fixed 4096 entries, "TODO: fix this!");
  - **`__wasm_syscall_N` bound directly to `vmlinux_instance.exports.wasm_syscall_N`**
    (no JS in the syscall path).

  JS then calls `__wasm_apply_data_relocs`, `__wasm_call_ctors`, and `_start`.
- **Same memory, no isolation**: user code runs in the same linear memory as the kernel and
  can read or write every kernel structure. `access_ok` is range-only and
  `UACCESS_MEMCPY` makes `copy_{to,from}_user` a plain `memcpy`.
- **Syscall entry** (`arch/wasm/kernel/entry.S`, `WASM_SYSCALL_ASM_HEAD/FOOT`;
  `kernel/traps.c`, `WASM_SYSCALL_N`). musl calls `__wasm_syscall_N(sp, tp, nr, a0..)`.
  - The asm head saves the user sp/tp into globals and switches `__stack_pointer` to the
    task's kernel stack. It stores the user sp into `pt_regs->stack_pointer` and the tp into
    `switch_stack->tls`.
  - The C part does `_exception_enter` (save cpuflags, clear INT|USER) and fills
    `pt_regs.syscall_nr/args`. It calls `syscall_enter_from_user_mode()` (generic entry, so
    **sys_enter/sys_exit tracepoints, audit and ptrace work**), dispatches through
    `sys_call_table` (`kernel/syscall_table.c`, asm-generic unistd plus `sys_wasm32.c`
    64-bit-arg wrappers), and calls `syscall_exit_to_user_mode()`.
  - **Restart** (`-ERESTART*`) is a C `do{}while` loop, because there is no IP to rewind.
  - The foot restores the globals and calls `_user_mode_tail` → `user_mode_tail()`, which
    returns a "flow" code. A non-zero code calls the JS `wasm_user_mode_tail(flow)`.
- **exec**: flow −1 makes JS throw `Trap("reload_program")`. That unwinds the old user
  instance's JS/Wasm stack, and the catch handler instantiates the pre-compiled new module
  and runs `_start` in the same Worker.
- **fork / vfork / clone**:
  - `fork` is impossible (NOMMU).
  - musl `vfork()` is stubbed: it prints "vfork() is not implemented yet, use clone()
    instead!".
  - BusyBox is patched to use `clone(fn, NULL, CLONE_VM|CLONE_VFORK|SIGCHLD, arg)`
    (hush, init, archival).
  - musl `_Clone` pushes fn/arg on the (parent or new) stack. The kernel creates a new
    Worker. In it, `ret_from_fork` returns 1 and JS calls the user export
    `__libc_clone_callback`, which pops fn/arg, `call_indirect`s, and ends with `SYS_exit`.
    The new Worker re-instantiates the *same* module from `mm->start_code`, sharing
    `__memory_base`, so globals and heap are shared.
  - musl rejects `CLONE_THREAD|CLONE_SETTLS|CLONE_CHILD_CLEARTID`, so **pthreads are
    effectively unsupported** **[inferred from the musl patch]**.
- **Signals** (`kernel/signal.c`): `setup_rt_frame()` writes an `rt_sigframe` (sig,
  info/uc pointers, handler, siginfo, ucontext with sp/tls) on the user stack, lowers
  `regs->stack_pointer`, and sets `_TIF_DELIVER_SIGNAL`.
  - At syscall tail (flow 1) JS calls the user export `__libc_handle_signal` (musl asm),
    which calls the handler and then `rt_sigreturn`.
  - `sys_rt_sigreturn` restores sp/tls/sigmask and sets `_TIF_RETURN_SIGNAL` (flow 2).
    JS then throws `Trap("signal_return")` to unwind back to the interrupted syscall site.
    Stacked signals use flow 3.
  - Signals are delivered **only at syscall/ret_from_fork boundaries**. A task spinning in
    user code never sees `SIGINT`/`SIGKILL` **[inferred]**.
- **Futex**: `asm/futex.h` implements atomic ops with `__atomic` builtins. Userland
  `Atomics.wait`-style futexes are not used. musl uses the futex syscall.

## 7. Changes outside arch/

| File | Change | Why |
|---|---|---|
| `fs/binfmt_wasm.c`, `fs/Kconfig.binfmt`, `fs/Makefile` | new `BINFMT_WASM` (depends on `ARCH_HAS_BINFMT_WASM`) | load `dylink.0` Wasm executables |
| `drivers/tty/hvc/hvc_wasm.c`, `Kconfig`, `Makefile` | new `HVC_WASM` | host console |
| `include/asm-generic/panic.h` (new), `include/asm-generic/Kbuild`, `kernel/panic.c` | `arch_panic_notify(buf)` in `vpanic()` behind `ARCH_HAVE_PANIC_NOTIFY` | surface panics to the JS host |
| `include/asm-generic/barrier.h` | `#include <asm/processor.h>` | `smp_cond_load_relaxed` needs `cpu_relax()` |
| `include/uapi/linux/elf-em.h`, `include/uapi/linux/audit.h` | `EM_WASM 264`, `AUDIT_ARCH_WASM32/64` | `syscall_get_arch()` / audit / seccomp |
| `scripts/Makefile.clang` | `CLANG_TARGET_FLAGS_wasm := wasm-linux-musl` | target triple |
| `scripts/mod/mk_elfconfig.c`, `scripts/mod/modpost.c` (NOMERGE) | accept `\0asm` magic; emit a dummy vmlinux export file | kbuild assumes ELF vmlinux.o |
| `lib/kunit/kunit-test.c` | skip the NULL-deref test on NOMMU | address 0 is valid memory |
| `mm/damon/core.c` (+kunit), `net/core/net_test.c`, `net/core/skbuff.c` | generic bug fixes found by allyes/kunit on Wasm | unrelated to the port |

No changes were made to `kernel/sched/`, `kernel/entry/`, `mm/nommu.c` or the tracing
code. The port is cleanly contained.

## 8. Known limitations / bugs

1. **Scalability is capped by NR_CPUS**: each live user process or thread consumes one
   CPU; `-EBUSY` when they run out. Each task also costs one Worker, one vmlinux instance
   and at least one user instance.
2. **No preemption or ticks on busy CPUs.** A CPU-bound kthread can starve its CPU. The
   `run_all_irqs()` comment describes KUnit lock-ups when IRQ_CPU gets tasks. KUnit tests
   that "hog the CPU in kernel mode" time out.
3. **The affinity/CPU-allocation code is self-described as needing review**
   (`user_task_set_affinity()` has a `hack:` label and TODOs; `release_thread()` TODO). CPU
   hotplug/GC of unused CPUs is not implemented.
4. **Every context switch hops through the main thread** (`postMessage` →
   `linux.js serialize_tasks` → `Atomics.notify`). A busy or blocked main thread stalls the
   kernel **[inferred]**.
5. **No memory protection**. User code shares linear memory with the kernel. The
   `0..0x10000` NULL guard does not trap.
6. No `fork`/`vfork` (clone+VFORK only), no pthreads (`CLONE_THREAD` rejected), and no
   `mmap` of shared pages beyond NOMMU semantics.
7. The user function table is a fixed 4096 entries with `table_start` always 0, so there is
   no dlopen/shared libs.
8. `instruction_pointer()`, `KSTK_EIP`, `__get_wchan` and `profile_pc` are all 0.
   `__builtin_return_address` is forced to 0. `show_stack` returns only a JS `Error().stack`
   string. There is no unwinder, no KALLSYMS in defconfig, and no DWARF (linker-script hack).
9. wasm64 host glue truncates to `Number` (<4 GiB only). wasm64 userland is experimental.
10. The console is polled (hvc/khvcd), and `hvc_get` blocks a Worker on `Atomics.wait`
    for a main-thread reply.
11. No block/virtio/net drivers, no device tree. The rootfs is initramfs only.
12. The LLVM build is fragile (every-other-run bug) and depends on an out-of-tree wasm-ld
    linker-script patch. `--import-undefined` hides link errors as runtime import failures.
13. `asm/stacktrace.h` `on_thread_stack()` still says "current is directly derived from
    the stack pointer" and compares `current & THREAD_MASK`. With THREAD_INFO_IN_TASK and a
    global `current`, this looks stale or incorrect **[inferred]**.
14. The LKML post (Nov 2025, 6.x era) says the system "will crash sooner or later".
    Stability has reportedly improved in 7.0 (KUnit passes), but this was not tested here.

## 9. Relevance to kSEE

### Directly reusable
- **The Worker-per-task_struct mechanism already exists** and matches kSEE's design.
  - Keep `__switch_to` → `wasm_create_and_run_task` / `wasm_serialize_tasks`.
  - Keep the per-Worker vmlinux instance with instance-local `current` and
    `__stack_pointer` globals.
  - Keep `ret_from_fork` in entry.S and the `_TIF_NEVER_RUN` first-run path.
  - The per-task `serialize` lock plus `last_task` handoff *is* a CPU-token handoff; today
    it is just scoped per CPU.
- The generic-entry syscall path (`WASM_SYSCALL_N`, direct `__wasm_syscall_N` imports with
  no JS in the hot path). With `HAVE_SYSCALL_TRACEPOINTS`, `sys_enter`/`sys_exit` will fire
  as-is.
- `binfmt_wasm.c` plus the musl/BusyBox patches give a working PIC-Wasm userland (shell,
  coreutils), with exec, clone+VFORK and signals.
- Clean build infrastructure: defconfigs, allno/allyes/kunit configs, `llvm-wrapper.py`
  and Docker. Up-to-date base (v7.0) with small, contained non-arch changes.
- clocksource/clockevent, hvc console, RNG seeding, panic hook, and exception →
  `raise_exception` plumbing.

### Conflicts with kSEE's design
- **SMP-by-construction vs. kSEE's UP-first.** SMP, IRQ_CPU=1, one-user-task-per-CPU
  pinning, tick broadcast and NO_HZ_FULL are the core of this port's concurrency story.
  Going UP requires:
  - dropping `user_task_set_affinity()` and the `IRQ_CPU` logic;
  - making `SMP=n` work: `smp.c` becomes optional, and `raised_irqs` needs a UP variant;
  - **adding an IRQ/timer poll at safe points** (syscall entry/exit, `schedule()`,
    `finish_arch_post_lock_switch` on CPU 0, `arch_cpu_idle`).

  Under UP with one token, a user task that never syscalls holds the token forever. That is
  the same limitation kSEE would have with any design, but Joel avoided it through SMP.
  This is the main redesign point: medium effort, roughly a few hundred lines in
  `process.c`, `smp.c`, `time.c` and `irq.c` **[inferred]**.
- **The token handoff goes through the main thread.** kSEE would want Worker→Worker
  `Atomics.notify` on a lock in shared memory, so that the scheduler, not JS bookkeeping,
  owns the token. This is easy to change: move `last_task`/lock words into kernel memory,
  for example in `thread_info` next to `instance_depth`, and notify directly.
- Per-CPU idle Workers: under UP there is one idle Worker, which is fine.
- Each user task gets its own Wasm module instance in its own Worker (the same shape kSEE
  wants). Memory is shared kernel memory with no isolation; this is acceptable for a
  teaching and visualisation tool.

### Adding kSEE tracing hooks
- **Tracepoints**: `sched_switch`, `sched_wakeup` and `sys_enter/exit` are generic
  `TRACE_EVENT`s. `CONFIG_TRACEPOINTS` needs no arch support, and without
  `HAVE_ARCH_JUMP_LABEL` static keys fall back to plain branches. A custom probe that writes
  events into a shared-memory ring buffer read by the UI Worker should work without ftrace
  **[inferred]**. Full `CONFIG_FTRACE`/trace_events via tracefs need `TRACING_SUPPORT`
  (which needs `STACKTRACE_SUPPORT`, not selected). You would have to add a stub
  `arch_stack_walk` or bypass tracefs.
- **Source correlation**: the obstacles are:
  1. `__builtin_return_address` is forced to 0 by the wrapper;
  2. there is no instruction pointer;
  3. the wasm-ld linker-script hack does not support `DWARF_DEBUG`;
  4. there is no KALLSYMS.

  kSEE's LLVM location-ID pass would sidestep 1 and 2. **BTF (pahole reads DWARF) is
  blocked until DWARF survives the linker-script link**. That means extending Joel's
  wasm-ld hack or post-processing `.debug_*` custom sections — the largest unknown
  **[inferred]**. A kSEE LLVM pass would also have to be carried on top of Joel's LLVM
  18.1.2 fork, which is an old base.
- Event transport: add a host import (for example `wasm_ksee_emit`) or, better, a
  lock-free SAB ring in kernel memory. `--import-undefined` makes adding imports trivial.

### Verdict: **4 / 5 — the best candidate to fork, with a planned UP rewrite of the concurrency layer.**

Reasons for:
- It is the only port on a current kernel (v7.0) that is maintained, builds
  allyes/allno, and passes KUnit.
- It is small and contained (~3.3 kLOC arch, trivial generic changes).
- It already implements Worker-per-task_struct over one shared `WebAssembly.Memory`, with
  Atomics-based task serialization. That is essentially kSEE's model minus the UP token.
- It uses generic entry, so syscall tracepoints come for free. The GPL-2.0 license is
  compatible.

Reasons against (−1):
- Its concurrency design is SMP/one-CPU-per-task, which kSEE explicitly does not want. IRQ
  delivery, ticks and affinity must be reworked for UP.
- It depends on a patched LLVM 18 wasm-ld (linker scripts) that currently discards DWARF,
  which blocks BTF/source correlation until fixed.
- The context-switch path relays through the JS main thread.
- The musl/BusyBox patches are self-declared hacks (no fork/vfork/pthreads).
