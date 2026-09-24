# tombl/linux: Tom Stokes' Linux-on-WebAssembly port

Reference notes for kSEE's fork decision. Sources: local clone `refs/tombl-linux` (shallow, HEAD `f06dad0dc`),
host runtime sparse clone `refs/tombl-distro/packages/kernel` (HEAD `7627e66`), GitHub API metadata.
"[inferred]" marks conclusions not stated in code or docs.

---

## 1. Summary

| Item | Value |
|---|---|
| Kernel repo | https://github.com/tombl/linux (default branch `wasm`) |
| Host runtime | https://github.com/tombl/distro, `packages/kernel` (npm `@lowland/kernel`). It moved there in commit `b7b4e9124` "wasm: move host code out of the kernel tree (#53)" (2026-08-13). It used to be `tools/wasm/src/*.ts`, and `tools/wasm/README.md` is now a stub pointing to the new location. |
| Userland ports | https://github.com/tombl/musl, https://github.com/tombl/busybox, plus more ports in tombl/distro (tar, util-linux, ...) |
| Author | Thomas Stokes ("tombl", `git@tombl.dev`). Effectively a single-author project. |
| License | Kernel: GPL-2.0 (GitHub reports NOASSERTION because of the kernel's multi-license COPYING). Host TS/JS: MIT (`packages/kernel/LICENSE`). |
| Kernel base | **Linux 7.1.5**. The port is one squashed commit `f692ca850` "arch/wasm: port to Linux 7.1" (2026-07-24) on top of `155b42bec` "Linux 7.1.5", followed by about 20 incremental `wasm:` commits. |
| Activity | Repo created 2024-02-07. Last push 2026-08-16 (`f06dad0dc`, enables signalfd/timerfd/fadvise). distro last pushed 2026-08-23. Very active in Jul–Aug 2026 (PRs #40–#53). 162 stars. |
| Demo | https://linux.tombl.dev (from README; also the docs site). Project homepage: https://low.land |
| Maturity | Boots to a busybox shell on musl, runs multicore with virtio blk/net/console/fs/vsock/rng, and has fork/exec/clone/signals/futex/POSIX semaphores. The distro is growing (ports being added). No MMU and no ptrace. Engineering quality is high: detailed comments on memory-ordering protocols and a host test suite (`test/wasm.test.ts`, 787 LOC). |
| LKML | The given URL (lkml.iu.edu/2511.0/00248.html) is **Joel Severin's** announcement "Port of Linux to WebAssembly" (2025-11-01, his port is based on 6.4-era code). No archived reply from Tom was found in the 2511.0–2511.3 indexes on lkml.iu.edu, and lore.kernel.org was blocked by Anubis. The comparison below comes from the code. |

## 2. Toolchain

- **Stock LLVM, not patched.** `flake.nix` uses `pkgs.llvmPackages_19` from nixos-25.11 (`clang-unwrapped`, `lld`, `libllvm`), plus `wabt` (`wasm2wat`), `dtc`, and perl.
- Top-level `Makefile` hard-codes `ARCH = wasm` and `LLVM = 1`. `scripts/Makefile.clang` adds `CLANG_TARGET_FLAGS_wasm := wasm32`, so the compiler gets `--target=wasm32` (bare wasm32, not wasi or emscripten).
- **wasm32 only.** `config 64BIT` exists but defaults to n. `TASK_SIZE U32_MAX`. The kernel memory is memory32 with a 4 GiB maximum (the host leaves out the last 64 KiB page).
- CFLAGS (`arch/wasm/Makefile`): `-matomics -mbulk-memory -mextended-const -mmultivalue -mmutable-globals -mnontrapping-fptoint -mreference-types -msign-ext -msimd128 -mllvm -wasm-enable-sjlj`, `-D__DISABLE_EXPORTS`, and `-D__builtin_return_address=wasm_kernel_return_address` (the host import always returns 0).
- LDFLAGS: `--import-memory --max-memory=4294967296 --shared-memory --no-merge-data-segments --export-table`. `CONFIG_LD_IS_WASMLD=y` makes the build skip `--build-id`, `-z noexecstack`, the linker script (`--script`), `--start-group`, and `mksysmap`.
- **No linker scripts.** wasm-ld has no linker-script support, so `vmlinux.lds` is not used for layout. The workaround has these parts:
  - `--no-merge-data-segments` keeps each input section as a named data segment.
  - `arch/wasm/scripts/sections.pl` reads `wasm2wat vmlinux.o` output, finds the `memory.init $<segment>` calls inside `__wasm_init_memory`, and writes `arch/wasm/sections.json` with the address and size of an allowlisted set of sections: initcall levels, `__param`, `__modver`, `.note.Linux`, `.builtin_fw`, `.con_initcall.init`, `.init.setup`, `.data.once`, and the five `.data..percpu*` sections.
  - `llvm-objcopy` embeds that JSON as custom section `.linux.sections`, and the initramfs as `.linux.initramfs`, into `vmlinux.wasm`.
  - At boot, the host passes the JSON to the kernel as DT `/chosen/sections`. `init_sections()` (`arch/wasm/kernel/sections.c`) fills in pointer variables such as `__initcall3_start` and `__start___param`. This is why `include/linux/init.h`, `init/main.c`, `lib/buildid.c`, and `moduleparam.h` switch from arrays to pointers under `CONFIG_WASM`.
  - Per-CPU data is copied into a memblock allocation. Every `PERCPU_PTR` goes through `arch_remap_percpu_ptr` → `__percpu_section_remap()`, a linear search over 5 ranges.
- `jiffies` is an LLVM `alias("jiffies_64")` (`kernel/time/timer.c`), replacing the usual linker-script alias.
- The build produces `vmlinux.wasm` (`make vmlinux.wasm`). Output is a single module that each Worker instantiates.

## 3. arch/wasm structure

About **5,000 LOC** in total (`find arch/wasm -type f | xargs wc -l` = 5012). The host TypeScript is about 6.5k LOC more.

```
arch/wasm/
  Kconfig (90)  Makefile (39)  configs/defconfig (64)  scripts/sections.pl (74)
  drivers/virtio_wasm.c (464)           virtio transport over host imports
  kernel/
    process.c (225)   __switch_to, copy_thread, task_entry: Worker-per-task core
    smp.c (224)       secondary CPU bring-up (one Worker per CPU idle task), IPIs
    irq.c (285)       per-CPU pending-IRQ bitmaps, arch_cpu_idle, cpu_relax, timer deadline poll
    time.c (139)      clocksource/clockevent via get_now_nsec + per-CPU deadline
    syscall.c (95)    exported `syscall` entry, syscall_table, set_thread_area
    signal.c (118)    synchronous signal-handler invocation, restart logic
    fork.c (54)       sys_clone replacement (fn,arg based), wasm_call_clone_fn
    binfmt_wasm.c (410) exec of wasm modules, sys_wasm_get_args, auxv
    remote_vm.c (267) cross-Worker user-memory access mailbox (process_vm_*, /proc/pid/mem)
    semaphore.c (412) kernel-backed POSIX named semaphores (sys_wasm_sem_*)
    sections.c (165)  section table + percpu remap
    setup.c (150)     _start (exported "boot"), setup_arch, DT load, machine_halt
    globals.c (61)    wasm globals: __stack_pointer, current_cpu, current_task, thread_done
    time/early_printk/cpuinfo/stacktrace/ptrace(show_regs only)/idle(empty)
  mm/ init.c (43) page.c (31)            memory.grow on demand ("materialize")
  lib/ multi3.c                          __multi3 helper
  include/asm/ ~40 headers (atomic, cmpxchg, barrier, ticket spinlock.h, uaccess, wasm_imports.h, ...)
  include/uapi/asm/ unistd.h (arch syscalls), sigcontext.h (pt_regs), ptrace.h, signal.h
```

Kconfig choices (`arch/wasm/Kconfig`):
- `MMU` is `def_bool n` (nommu). The port selects `FLATMEM`, `HAVE_PAGE_SIZE_64KB` (PAGE_SIZE matches the 64 KiB wasm page), `THREAD_INFO_IN_TASK`, `GENERIC_ENTRY`, and `ARCH_HAS_SYSCALL_WRAPPER`.
- `THREAD_SIZE` = 2 pages = 128 KiB. This covers the kernel stack plus `pt_regs`, and `setup_arch` asserts it is large enough.
- **SMP is forced.** Kconfig has `select SMP` and `config SMP def_bool y`, with `NR_CPUS` 64. The actual CPU count comes from DT `/chosen/ncpus` (host option `cpus`), so ncpus=1 gives a UP machine at runtime but still an SMP kernel build. Parallel hotplug is on (`HOTPLUG_PARALLEL`).
- **Preemption:** `select ARCH_NO_PREEMPT`, so only PREEMPT_NONE is possible. This matches kSEE.
- HZ=100. `NO_HZ_IDLE` and `HIGH_RES_TIMERS` in the defconfig.
- **Device tree:** `OF`, `OF_EARLY_FLATTREE`, and `OF_IRQ` are selected. The DT is generated in JS (`devicetree.ts`) and holds memory, reserved-memory, chosen (bootargs, rng-seed, ncpus, initrd, sections), and one `virtio,wasm` node per device.
- **Virtio:** `CONFIG_VIRTIO_WASM` (`drivers/virtio_wasm.c`), a custom transport that is neither MMIO nor PCI. The defconfig enables virtio blk/net/console/fs/vsock/hw_random plus EXT4, EROFS, SQUASHFS, OVERLAY, FUSE, NET/INET/UNIX, DEVTMPFS, and MAGIC_SYSRQ.
- Locks: the port uses its own ticket spinlocks and rwlocks (`asm/spinlock.h`). The waiter blocks with `memory.atomic.wait32` on `owner`, and unlock does `notify(INT_MAX)` (commit `31757f660` replaced the qspinlocks).

## 4. Boot & host runtime

**Instantiation** (`packages/kernel/src/index.ts`):
1. The host parses `vmlinux.wasm`'s memory import. It must be exactly one `env.memory`, shared and i32. The host reads the custom sections `.linux.sections` and `.linux.initramfs`.
2. It allocates a shared `WebAssembly.Memory` (initial = module minimum + initcpio pages, maximum = 4 GiB − 64 KiB), copies the initcpio in after the module's static data, and builds the DT.
3. It instantiates the kernel **on the main thread** and calls `exports.boot()`.

**`_start`** (export `boot`, `arch/wasm/kernel/setup.c`):
- Reserves memory `[0, __heap_base)`.
- Fetches the initramfs through the `boot.get_initramfs` import and the DT through `boot.get_devicetree`.
- Runs `early_init_dt_scan`, `__wasm_call_ctors()`, and `init_sections()`.
- Sets the main thread's `current_cpu=-2` and `current_task=NULL`.
- Calls `kernel.spawn_worker(do_start_kernel, ...)` to spawn Worker **"boot"**, which runs `start_kernel()` as `init_task` on CPU 0.

After this the main-thread instance only services devices:
- It runs `run_on_main` callbacks for virtio setup, notify, reset, and enable/disable vring.
- It calls `exports.trigger_irq(irq)` when a device completes.

**Memory layout** (single flat kernel memory, `PAGE_OFFSET = PHYS_OFFSET = 0`):
- `[0, __heap_base)` holds the wasm-ld layout: data segments, then a stack region (`__stack_low..__stack_high`, used only during the first boot phase).
- Then the initrd.
- Then memblock/buddy memory up to the DT `/memory` size.
- Physical memory is committed lazily. `arch_memblock_materialize()` and `arch_alloc_page()` (`mm/page.c`) call `memory.grow` up to the highest PFN in use, under a raw spinlock.
- Generic patches support this lazy commit:
  - `ARCH_WANTS_BUDDY_ALLOCATOR_BOTTOM_UP` changes `mm/page_alloc.c __rmqueue_smallest` to pick the lowest-PFN candidate across orders.
  - `page_group_by_mobility_disabled` is set, and `SHUFFLE_PAGE_ALLOCATOR` is disabled.

**Kernel imports** (`arch/wasm/include/asm/wasm_imports.h`, host side `src/wasm.ts kernel_imports`, `src/worker.ts`):
- `boot.{get_devicetree,get_initramfs}`
- `kernel.{breakpoint, halt_worker, terminate_machine, boot_console_write, boot_console_close, return_address, get_now_nsec, get_stacktrace, spawn_worker, run_on_main}`
- `user.{compile_begin/write/end/abort, instantiate, call, switch_entry, call_signal_handler, call_siginfo_handler, halt_signal_handler, read, write, write_zeroes, futex_atomic_op, futex_atomic_cmpxchg}`
- `virtio.{set_features, setup, reset, enable_vring, disable_vring, notify}`

**Kernel exports:**
- `boot`, `syscall`, `get_thread_area`, `copy_siginfo`, `clear_siginfo`, `trigger_irq`, `trigger_irq_for_cpu`
- `__indirect_function_table`, via `--export-table`. The host uses it to invoke the `fn` pointer given to `spawn_worker`.

**Per-Worker state:** each Worker instantiates the same `WebAssembly.Module` against the shared memory, so wasm globals are private to each Worker. `globals.c` defines the wasm globals `current_cpu`, `current_task`, and `thread_done`, and uses the linker's `__stack_pointer`. `current` is `global.get current_task`. `smp_processor_id` reads `current_cpu`.

**Console:**
- Early console: `early_printk.c` calls the `kernel.boot_console_write` import, which posts the text to the main thread.
- Normal console: virtio-console `hvc0` (`bootargs console=hvc0`). Recent commits hand the console off from boot_console to hvc and hold queued input until the guest opens the port.

**Timer** (`time.c`, `irq.c`):
- Clocksource `wasm` reads `get_now_nsec` (performance.now + timeOrigin, 5 µs granularity).
- Each CPU has a one-shot clockevent. `set_next_event` only stores the per-CPU `timer_deadline_ns`.
- Nothing asynchronous fires the timer. The deadline is checked in two places:
  - (a) `arch_cpu_idle()`, which does `memory.atomic.wait64` on the per-CPU IRQ summary word with timeout = deadline − now. On timeout it pends `TIMER_IRQ`.
  - (b) `wasm_timer_check()`, called on every syscall entry.
- `__delay` is `atomic.wait32` on a dummy word with a timeout.

**IRQs** (`irq.c`):
- Each CPU has a two-level `pending_irqs` bitmap (a summary word plus 4×64 words). hwirq 1 = IPI, 2 = TIMER, 3+ are external (virtio, from `wasm_alloc_irq()`).
- `trigger_irq_for_cpu()` sets bits and `atomic.notify`s the summary word. `irq_target[]` gives each IRQ's CPU affinity.
- IRQs are delivered **only when interrupts are re-enabled**: `arch_local_irq_restore(ENABLED)` runs `run_irqs()` → `generic_handle_domain_irq`. `irqflags` is a per-CPU software flag.
- So interrupt delivery is polled at `local_irq_enable`/`restore` points and on idle wake. Nothing interrupts running code.
- IPIs (`smp.c`) set per-CPU bits and raise IPI_IRQ, which handles reschedule, call-func, irq_work, and stop.

**Devices:** the JS implements virtio device models in `src/virtio/{block,console,net,fs(1255 LOC virtiofs),vsock,entropy}.ts`. Split virtqueues sit in kernel memory. The host reads and writes the rings directly through the shared buffer. `remote.ts`/`endpoint.ts` let a device run in another Worker (`workerDevice`/`serveDevice`).

## 5. Tasks, context switching & CPUs

**One Web Worker per task_struct**, including kernel threads. Idle threads are the exception.

- `copy_thread()` (`arch/wasm/kernel/process.c`):
  - Zeroes `task_pt_regs(p)` and sets `thread_info.running_cpu = WASM_CPU_PARKED (-1)`.
  - Returns immediately for `args->idle`, so no Worker is spawned for idle threads.
  - Otherwise calls the `kernel.spawn_worker(task_entry, bootstrap_args, "comm (pid)", user_memory)` import.
  - `user_memory` is NONE for kernel threads. For user clones (`fn == wasm_call_clone_fn`) it is SHARE if CLONE_VM, else COPY.
  - The host creates a new Worker, instantiates vmlinux against the same memory, and calls `table.get(fn)(arg)`.
  - Spawning is asynchronous for NONE/SHARE. It is synchronous for COPY: the parent `Atomics.wait`s until the child has snapshotted the user memory.
- `task_entry()` sets `__stack_pointer = task_pt_regs(task) - 1`, so the task's kernel stack is `task->stack`. It then calls `task_entry_inner()`, which:
  - **Parks:** loops `memory.atomic.wait32(&running_cpu, PARKED, 1s)` until `running_cpu >= 0`. It logs if the wait exceeds 1 s.
  - Then `set_current_cpu`, `set_current_task`, `schedule_tail(prev)`, `fn(fn_arg)`.
  - A kernel thread that execs userspace returns from `fn`. The code then calls `wasm_user_call()`, which runs the user module in this same Worker. If that returns, it runs `do_exit(SIGSEGV)`.
- **`__switch_to(from, to)`** is exactly a CPU-token handoff:
  1. `cpu = atomic_xchg(&from->running_cpu, PARKED)`.
  2. CAS `to->running_cpu` from its negative state (PARKED or REMOTE_WAKE) to `cpu`.
  3. `memory.atomic.notify(&to->running_cpu, 1)` wakes the target Worker.
  4. If `thread_done` is set (by `arch_task_dead()` → `wasm_set_thread_done(true)`, a new generic hook in `do_task_dead()`), it calls `kernel.halt_worker`. The JS throws `HALT_KERNEL` and the Worker exits.
  5. Otherwise it services any remote-memory request (`wasm_service_remote_request`).
  6. It then blocks in `atomic.wait32(&from->running_cpu, PARKED, ∞)`. When woken with REMOTE_WAKE (-2), it services the cross-Worker user-memory request and parks again.
  7. When handed a CPU, it sets `current_cpu`, reads `prev = current_tasks[cpu]`, sets `current_task`, and returns `prev`.

  There is no register or stack save. Each Worker keeps its own native wasm stack and suspends inside `atomic.wait`. The C call chain `schedule() → __switch_to` is literally frozen in the Worker.
- **CPUs:**
  - CPU 0's idle task (`init_task`) lives in the "boot" Worker.
  - `arch_cpuhp_kick_ap_alive()` spawns Worker "entryN" running `secondary_entry` → `cpu_startup_entry` as that CPU's idle task. So each CPU's idle task has a dedicated Worker that never parks except in `arch_cpu_idle`.
  - `current_tasks[NR_CPUS]` in shared memory records who holds each CPU.
  - Real SMP: N Workers truly run in parallel, with atomic ticket locks, `irq_set_affinity`, and IPIs.
- **Ticks and preemption:** cooperative only (`ARCH_NO_PREEMPT`). `TIF_NEED_RESCHED` is acted on at syscall exit (`syscall_exit_to_user_mode`) and at voluntary `schedule()`. The timer IRQ is noticed only in idle, at syscall entry, or when IRQs are re-enabled after the deadline was pended. The comment in `irq.c` says: "a task that never re-enters the kernel still cannot be preempted." A user busy-loop without syscalls holds its CPU indefinitely.
- **Spinning:** `cpu_relax()` = `atomic.wait64` on the IRQ summary word with a 100 µs timeout.

## 6. Userspace & syscalls

- **Program format:** `binfmt_wasm.c` accepts files starting with `\0asm\1\0\0\0`.
  - It streams the file in 256 KiB chunks through `user.compile_begin/write/end`.
  - The host validates that the module imports exactly one shared i32 `env.memory` and only `linux.syscall`, `linux.get_thread_area`, and `linux.copy_siginfo` (`user_module_imports_supported`).
  - The host compiles the module and allocates a new memory with maximum = min(module max, RLIMIT_AS/64K).
  - The kernel then runs `begin_new_exec` and `wasm_user_instantiate(true)`.
  - Args are not placed on a stack. The new `sys_wasm_get_args` (arch syscall +1) copies a relocated argv/envp/auxv/AT_RANDOM blob into a user buffer.
  - The entry point is the module's `_start`.
  - No ELF, no FDPIC, no dynamic linking.
- **Memory model:** **each process has its own `WebAssembly.Memory`**, separate from kernel memory. User pointers are offsets into that memory, so kernel and user address spaces are disjoint.
  - The user module is instantiated *inside the task's own Worker*, next to that Worker's kernel instance.
  - `__ARCH_WANT_NO_MEMORY_SYSCALLS` removes `brk`, `mmap`, `munmap`, and `mremap` from the table (new generic `#ifndef` in `asm-generic/unistd.h`). musl allocates with `memory.grow` in userspace [inferred from the missing syscalls and the tombl/musl port].
- **Syscall entry:** user code calls the import `linux.syscall(nr, a0..a5)`, which calls the kernel export `syscall` → `wasm_syscall()` (`kernel/syscall.c`). That function:
  - Fills `pt_regs` (`{syscall_nr, args[6], return, restart_nr, user_mode}`) and does `local_irq_disable`.
  - Calls `rcu_note_context_switch` (a user boundary counts as an RCU quiescent state), `syscall_enter_from_user_mode` (GENERIC_ENTRY), `wasm_timer_check()`, and `wasm_service_remote_request()`.
  - Dispatches through `syscall_table[nr](regs)` (pt_regs-style wrappers) and runs `syscall_exit_to_user_mode`, which is where signals and resched happen.
  - Loops for syscall restart.

  If exec replaced the user instance during the syscall, the JS throws `HALT_USER` and restarts at the new `_start`.
- **copy_to/from_user:** `raw_copy_from_user = wasm_user_read` and `raw_copy_to_user = wasm_user_write`, which are JS imports copying between the kernel `memory.buffer` and the current Worker's user memory. `__clear_user = write_zeroes`. Futex ops use JS `Atomics.*` on user memory. `__access_ok` is just a bounds check against 4 GiB.
- **Cross-process access** (`remote_vm.c`, used from patched `mm/nommu.c __access_remote_vm`): a user memory can only be touched from the Worker that owns it. The requester publishes a request in `mm->context.remote` (IDLE→PUBLISHED→CLAIMED→DONE). It then CASes the owner's `running_cpu` PARKED→REMOTE_WAKE and notifies. The owner Worker performs the copy even while parked, with timeout and cancellation handling.
- **fork/vfork/clone:** `sys_clone` is replaced (`kernel/fork.c`) with a `(fn, fn_arg, flags, ptid, ctid, tls)` signature. Userspace must supply an entry function, since a wasm stack cannot be duplicated.
  - CLONE_VM (threads, vfork-like): the child Worker gets the *same* user Memory and module, and `switch_entry` calls `__indirect_function_table.get(fn)(arg)`.
  - Without CLONE_VM (fork): the child Worker synchronously copies the whole user Memory into a new one (`WASM_USER_MEMORY_COPY`). This is refused with `-EOPNOTSUPP` if `mm_users > 1`, because other threads cannot be stopped during the snapshot.
  - The child starts at `fn`, not at the fork return point. The C library must emulate fork via clone(fn) [inferred: tombl/musl does this].
  - TLS: `set_thread_area` syscall and `get_thread_area` export (`tp_value` in thread_info).
- **Signals** (`signal.c`): delivered synchronously at syscall exit. `arch_do_signal_or_restart` → `call_signal_handler` does `exit_to_user_mode`, calls the user handler through the JS import (table lookup), then re-enters, restores `pt_regs` and the mask, and returns.
  - No `rt_sigreturn` (`__ARCH_WANT_NO_RT_SIGRETURN`).
  - `sigaltstack` returns -ENOSYS.
  - SA_SIGINFO goes through a user trampoline that pulls siginfo via the `copy_siginfo` export.
  - A task that never makes a syscall never receives signals.
- **ptrace:** `arch_ptrace` BUG(). `instruction_pointer` = -1. `user_stack_pointer` traps. `syscall_get_arch` returns 0 (TODO).

## 7. Changes outside arch/

`git diff 155b42bec HEAD` excluding arch/wasm and tools/wasm touches **61 files, +663/−105** (this includes README, flake, and selftest tweaks). The meaningful changes:

- **Build glue:** `Makefile` (ARCH/LLVM defaults, no build-id or noexecstack for wasm-ld), `scripts/Makefile.clang` (target), `scripts/Makefile.vmlinux_o` (full wasm-ld link instead of `-r`), `scripts/link-vmlinux.sh` (no `--script`, no groups, no System.map), `scripts/Makefile.dtbs` (wasm `.type`/`.size` and section flags), `scripts/mod/mk_elfconfig.c`, `usr/initramfs_data.S`.
- **Sections-as-pointers (no linker script):** `include/linux/init.h`, `init/main.c` (initcall level arrays of pointers, separate `*s` sync levels and rootfs level, ctors skipped because wasm-ld merges them into `__wasm_call_ctors`), `include/linux/moduleparam.h`, `kernel/params.c`, `kernel/ksysfs.c`, `lib/buildid.c`, `drivers/base/firmware_loader/builtin/main.c`, `init/initramfs.c` (`__initramfs_start` becomes a pointer), `include/linux/percpu-defs.h` (`arch_remap_percpu_ptr` hook, overridable `per_cpu_ptr`), `kernel/time/timer.c` (jiffies alias).
- **Scheduler classes:** `kernel/sched/{sched.h,core.c,fair,rt,deadline,idle,stop_task,ext}.c`. `DEFINE_SCHED_CLASS(name, rank)` adds an explicit `rank` field and a `sched_class_by_rank[]` table, replacing the linker-section ordering that `sched_class_above()` relied on. `arch_task_dead()` hook in `do_task_dead()`.
- **Memory:** `mm/Kconfig` (`ARCH_HAS_MEMBLOCK_MATERIALIZE`, `ARCH_WANTS_BUDDY_ALLOCATOR_BOTTOM_UP`), `mm/memblock.c`, `mm/mm_init.c`, `mm/page_alloc.c` (bottom-up selection), `mm/nommu.c` (remote VM access via the owner Worker, `wasm_remote_mm_shutdown` in `exit_mmap`), `include/asm-generic/access_ok.h`.
- **Syscalls:** `include/uapi/asm-generic/unistd.h` (`__ARCH_WANT_NO_RT_SIGRETURN`, `__ARCH_WANT_NO_MEMORY_SYSCALLS`), `kernel/sys_ni.c` (`sys_ni_syscall(const struct pt_regs *)`), `kernel/entry/syscall-common.c` (**syscall tracepoints `#ifdef CONFIG_HAVE_SYSCALL_TRACEPOINTS`, because wasm does not select it**).
- **Misc:** `drivers/of/{Kconfig,of_reserved_mem.c}` (stubs when RESERVED_MEM_DECLARE is off), `kernel/fork.c` (BUG in `idle_dummy`), `kernel/time/time.c` and `include/linux/semaphore.h` (rename a `jiffies` param that collides with the alias), `lib/vsprintf.c`, `fs/fuse/readdir.c`, `block/bdev.c`, `kernel/cpu.c`, `kernel/irq_work.c`.

Upstreamability: most changes are generic but `#ifdef CONFIG_WASM`-heavy. The sched-class rank change and the tracepoint guard are the ones kSEE would also need to carry.

## 8. Known limitations / bugs

- **No asynchronous preemption.** Timer and IRQ delivery depend on the running code re-enabling IRQs, entering a syscall, or going idle. A CPU-bound user task without syscalls starves its CPU. Signals are also only delivered at syscall exit.
- No MMU, mmap, brk, or munmap syscalls. No shared file mappings or MAP_SHARED between processes [inferred]. RLIMIT_AS is snapshotted at exec.
- fork is only supported for single-threaded mms. Children start at a user-supplied fn rather than returning from fork. Full-memory copy on fork is O(user memory).
- No ptrace, sigaltstack, rt_sigreturn, or coredump. `syscall_get_arch` is a TODO. `KSTK_EIP/ESP` are 0. `__get_wchan` and `thread_saved_pc` BUG(). `__builtin_return_address` is 0, so `%pS`/`kallsyms` caller info is useless. Stack traces come from the JS `Error().stack` (`get_stacktrace`).
- No dynamic linking or modules (`__DISABLE_EXPORTS`).
- Kernel memory is capped at 4 GiB (wasm32). Browsers may refuse large shared-memory maxima, and `allocate_shared_memory` halves the maximum until it succeeds.
- Each task costs a Worker plus a full kernel-module instance. Hundreds of tasks mean hundreds of Workers [inferred: scalability limit].
- The SMP kernel is always built. There is no `CONFIG_SMP=n` path, and ncpus=1 is SMP-with-one-CPU.
- Browser quirks are worked around in the host: Chromium stale buffer wrappers after grow (`memory.grow(0)`), and a WebKit shared-memory reclaim bug (forwarded_init via MessagePort).
- The section allowlist is hand-maintained in two places (`sections.pl` and `sections.c`). Any new linker-section-based feature (tracepoints, jump labels, BTF ids, kprobes blacklist, ...) silently gets no table until it is added to both.
- `HAVE_SYSCALL_TRACEPOINTS`, `HAVE_FUNCTION_TRACER`, and all `HAVE_*` tracing arch features are not selected. The defconfig has no tracing and sets `CONFIG_RCU_TRACE` off.

## 9. Relevance to kSEE

**Reusable, and closely aligned with our design:**
- The **Worker-per-task + shared kernel Memory + CPU-token handoff** is already implemented and debugged. `thread_info.running_cpu`, `__switch_to` (xchg/CAS/notify/wait32), and `task_entry` parking are almost exactly kSEE's planned "CPU token". It also handles REMOTE_WAKE, task death (`arch_task_dead` → `halt_worker`), and idle-per-CPU Workers.
- Per-Worker wasm globals for `current`, `smp_processor_id`, and the stack pointer. This is the clean solution.
- wasm-ld build integration without a linker script (`sections.pl` + `init_sections`), lazy `memory.grow` materialization, the IRQ/timer polling model, the virtio transport, and a working JS device library (blk/console/fs/net/vsock).
- GENERIC_ENTRY syscall path (`wasm_syscall` → `syscall_enter_from_user_mode`). Hooking `sys_enter`/`sys_exit` there is natural.
- Current kernel (7.1.5). PREEMPT_NONE (ARCH_NO_PREEMPT) matches our plan exactly. `ncpus=1` gives UP at runtime.
- The host is separable and MIT licensed (npm `@lowland/kernel`). It could be reused as-is or forked, and it keeps our UI independent of the kernel tree.

**Conflicts / gaps against kSEE's plan:**
- Kconfig forces `SMP=y` (NR_CPUS=64). UP-first would mean `ncpus=1` in the DT. A true `SMP=n` build needs Kconfig and `smp.c`/`irq.c` rework [inferred: `current_tasks[]`, per-CPU remap, and IPIs assume SMP]. This is moderate work and probably unnecessary.
- User processes use **separate per-process Memories** instead of living in the kernel Memory. That is fine for kSEE (arguably better), but it means user-side event instrumentation must go through JS imports, not shared memory. Source correlation for user code would need that module's own DWARF.
- **Tracing infrastructure is absent:**
  - `HAVE_SYSCALL_TRACEPOINTS` is not selected (syscall-common.c is patched to compile without it).
  - No ftrace, kprobes, or jump labels.
  - Tracepoint and trace-event sections (`__tracepoints_ptrs`, `_ftrace_events`, `__tracepoint_str`, `__bpf_raw_tp_map`, `.BTF_ids`, jump-table sections) are **not** in the `sections.pl`/`sections.c` allowlist.
  - Enabling `CONFIG_TRACEPOINTS`/`EVENT_TRACING` therefore means adding those sections, with the same pointer-variable conversion for `include/asm-generic/vmlinux.lds.h` users such as `__start___tracepoints_ptrs` and `__start_ftrace_events`.
  - Alternatively, kSEE's own lightweight hook ABI can bypass ftrace. Call a `kernel.trace_event` import or write a shared-memory ring from `__switch_to`, `try_to_wake_up` (`trace_sched_wakeup` sites), and `wasm_syscall`. This is **easy (about 100 LOC)**, because every context switch already passes through the arch `__switch_to` and every syscall through `wasm_syscall`.
- **BTF:** pahole/`resolve_btfids` on wasm objects is unproven [inferred: pahole reads ELF DWARF, and wasm DWARF lives in custom sections]. `link-vmlinux.sh` still has the BTF branch, but nothing exercises it. We should expect to generate type info offline from clang DWARF (`-g`, `llvm-dwarfdump`) instead. The same applies to LLVM location IDs: that needs our own clang pass or DWARF line tables, independent of which base we fork.
- The cooperative-only preemption means `sched_switch` events happen only at syscall and IRQ-enable boundaries. That is acceptable for PREEMPT_NONE semantics, but the UI must not imply timer preemption of user loops.
- Scale: one Worker plus one kernel instance per task. This is fine for teaching-size workloads.

**Difficulty of adding tracing hooks:** low for custom arch-level hooks (switch, wakeup, syscalls, IRQ entry in `run_irq`). Medium for real Linux tracepoints and trace events, which need the section plumbing plus `HAVE_SYSCALL_TRACEPOINTS` and `syscall_get_arch`. Medium-high for BTF.

**Fork-suitability verdict: 5 / 5.**
- Its core architecture *is* kSEE's architecture: Worker ↔ task_struct, one shared kernel Memory, a CPU token handed over through atomic wait/notify in `__switch_to`, and PREEMPT_NONE, nommu, wasm32.
- It is on a current kernel (7.1.5), builds with stock LLVM 19, is actively maintained with carefully documented concurrency, has a small arch (about 5k LOC) and a moderate generic diff (about 600 lines), and has a separable MIT host.
- The main work for kSEE is additive: trace-section plumbing or a custom event ABI, BTF/DWARF extraction, and a UI. We would not have to redesign anything.
- Risks:
  - Single-author upstream moving fast: pin a commit and rebase periodically.
  - Forced SMP.
  - The hand-maintained section allowlist.
  - The host now lives in a separate repo: vendor `tombl/distro/packages/kernel`.
