# LinuxOnTab 2.0: a downstream repackaging of tombl/linux (wasm-6.1) with fork() added through Asyncify

Legend used throughout:

- **[V]**: verified from artifacts: the repo clone at `refs/linuxontab`, or files downloaded from the live site into `refs/linuxontab-2.0-site/` (the kernel `.wasm` was parsed, and the host JS is unminified and was read).
- **[W]**: claimed on the website (`linuxontab.com/2.0/`) and not independently verified.
- **[I]**: my inference.

Investigated 2026-09-24.

---

## 1. Summary

| | |
|---|---|
| Landing page | https://linuxontab.com/2.0/ (a static marketing page, the same file as `refs/linuxontab/2.0/index.html`) |
| Running app | https://next.linuxontab.com/ (terminal) and https://next.linuxontab.com/console.html (image manager) |
| Repo | https://github.com/kilian-ai/linuxontab (cloned at `refs/linuxontab`, HEAD `0ec067c`, 2026-09-10) |
| Author | "kilian-ai" (Kilian, kiliannc@gmail.com). A single developer, working with heavy AI assistance (see `.github/agents/linuxontab.agent.md`). The kernel banner reads `kilian@mbair24.local`. |
| Kernel base | **Linux 6.1.132+** [V], from the string `Linux version 6.1.132+ (kilian@mbair24.local) (Homebrew clang version 19.1.7, Homebrew LLD 19.1.7) # SMP` in `vmlinux.wasm`. |
| Actual upstream | **tombl/linux, branch `wasm-6.1`** (Thomas Stokes) [V]. Evidence: the arch/wasm file paths and symbols (`task_entry`, `wasm_call_clone_fn`, `virtio_wasm`, `binfmt_wasm`), identical import namespaces (`boot/kernel/user/virtio`), the JS `Machine`/`BlockDevice`/... API, and the package metadata `built_with=tombl/distro`. Compare `docs/related/tombl-linux.md`. |
| License | Kernel: GPL-2.0 (inherited). Repo: no top-level LICENSE; the README only lists upstream component licenses (v86 BSD-2, xterm MIT). |
| Source availability | **The kernel source for 2.0 is not published.** [V] Neither the repo nor any kilian-ai GitHub repo holds the modified arch/wasm tree. Only binaries are served: `vmlinux.wasm` and the host JS (which is readable source). `refs/linuxontab/wasm-cmds` shows the real development tree is private, at `/Users/kilian/.ai/LinuxOnTab-kernel`. The public `kilian-ai/linux-wasm` is an unrelated, older fork (April 2026) of **joelseverin/linux-wasm**, not of the 2.0 kernel. |
| 1.x vs 2.0 | **1.x** (what the repo mostly contains): the v86 x86 emulator booting an Alpine ISO (`shell/`, `libv86.js`, `v86.wasm`, `*.iso`). **2.0**: native wasm32 Linux. The kernel is a wasm module, and user programs are wasm32-linux-musl modules, one Worker per task. The repo does contain the 2.0 **userspace package tree** (`packages/*.tar.gz`, `packages/recipes/*.sh`, `packages/build-package.sh`). |
| Maturity | Labelled a "Technical Preview" [W]. It runs a large, impressive userland (mc, tmux, nano, CPython+pip, clang/wasm-ld in the guest, X11 via Xvfb/x11vnc, Rust through a WALI bridge) [W, and the package list is V]. The code quality is "hotfix-driven". Examples: a binary-patch of Xvfb keyed on file size 3490984 inside `user.compile` [V], and a heuristic scan of linear memory for a wasm module to work around wasm-ld's mmap output [V]. |

## 2. Toolchain

- **Kernel** [V]: Homebrew clang 19.1.7 and LLD 19.1.7, according to the banner and the `producers` section. The target features enabled in the kernel module are atomics, bulk-memory, **exception-handling**, extended-const, multivalue, mutable-globals, nontrapping-fptoint, reference-types, sign-ext and simd128.
- **Userspace** (`packages/build-package.sh`) [V]:
  - Compiler and link line: `clang -target wasm32 --sysroot=<musl sysroot>` (clang 19 from Nix), linked with `-nostdlib -static -Wl,--import-memory,--export-memory,--export-table,--shared-memory,--max-memory=268435456`.
  - Compile flags: `CFLAGS=-O2 -matomics -mbulk-memory`.
  - Asyncify: **every binary is post-processed with `wasm-opt --enable-exception-handling --asyncify -O1`** (a whole-program transform with no allow-list), unless the build passes `--no-asyncify`.
  - setjmp/longjmp: `-mllvm --enable-emscripten-sjlj`. The host provides the `invoke_*` and `emscripten_longjmp` wrappers in `worker.js`.
  - libc: a patched musl sysroot ("mallocng alloc_meta fix", at `toolchain/musl-sysroot-fixed`). That toolchain directory is not in the public repo.
  - Nix builds come from tombl/distro (`make-packages.sh`).
- **Userspace ABI** [V]: modules import `env.memory` (shared), `linux.syscall(nr,a0..a5)`, `linux.get_args`, `linux.get_args_length` and `linux.get_thread_area`. Syscall numbers are asm-generic 32-bit (for example 220 = clone, 98 = futex, 403 = clock_gettime64).
- **Rust** [V]: `wali-bridge.js` translates `wali.SYS_<x86-64 name>` imports (arjunr2/WALI ABI) onto the asm-generic `linux.syscall`. It also emulates mmap/brk host-side with `memory.grow`.
- **In-guest toolchain** [V, as packages only]: `clang-19.1.7`, `wasm-ld-19.1.7`, `wasm-opt-129(-wali)`, `rustc-1.99.0` and `cargo-0.100.0` tarballs. Whether they run correctly is [W].

## 3. Kernel port structure & config (as far as determinable)

No source is available, so everything here comes from `vmlinux.wasm` (5.36 MB, 16,433 functions, a full `name` section) [V].

- **arch/wasm files referenced** [V]: `kernel/{process,setup,time,ptrace,irq,smp}.c`, `include/asm/{syscall,processor}.h`, `include/uapi/asm/ptrace.h` and `drivers/virtio_wasm`. This matches tombl `wasm-6.1`'s `arch/wasm/kernel/*` (`binfmt_wasm.c`, `fork.c`, `smp.c`, `irq.c`, ...).
- **Config signals** [V, from symbols and strings]:
  - **NOMMU**: `add_nommu_region`, `ramfs_nommu_mmap` and `proc_nommu_init` are present, and the upstream Kconfig forces `!MMU`.
  - **SMP=y**: the banner says `# SMP`, and `secondary_entry` and `smp_send_reschedule` are present. It is booted with `nr_cpus=1`.
  - **ARCH_NO_PREEMPT**: this comes from the upstream Kconfig. `__cond_resched` is present.
  - Page size is 64 KiB, as in upstream.
  - `binfmt_wasm` (`load_wasm_binary`) plus `binfmt_script`, with no ELF.
  - Filesystems: ext4 and ext2; no squashfs, erofs, 9p, fuse or overlayfs.
  - Terminals: devpts/ptmx/unix98 pty and hvc.
  - Syscall facilities: futex (the JS comment says `CONFIG_FUTEX=y`), epoll, eventfd, signalfd and timerfd.
  - Networking: inet/tcp plus unix sockets.
  - Virtio drivers: virtio-blk and virtio-net.
  - `CONFIG_POSIX_TIMERS` and `CONFIG_KALLSYMS` appear to be off, judging by their warning strings.
- **LinuxOnTab-specific kernel additions** [V symbol; I that they are downstream]:
  - `wasm_get_kdiag`, `wasm_kdiag_switch` and `wasm_ktrace` (hang diagnostics; `@K@` lines are routed to an HTTP log).
  - `wasm_get_clock_origins` and `wasm_publish_clock_origins`, which let user Workers compute `clock_gettime` in JS without entering the kernel.
  - The `get_monotonic_ns` and `get_real_ns` exports.
  - A patched devicetree buffer size.

  None of these appear in upstream `wasm-6.1` `time.c`, `irq.c`, `setup.c` or `smp.c`. `wasm_get/set_timer_deadline` is upstream.
- **Snapshot age** [V/I]: the imports are `user.compile(buf,size)` (single-shot) and `kernel.spawn_worker(fn,arg,comm,len,share_user_memory)`. Current upstream `wasm-6.1` uses `compile_begin/compile_write/compile_end/compile_abort` and a `wasm_user_memory` enum (NONE/SHARE/COPY), and adds `copy_siginfo`. So LinuxOnTab forked an **older** `wasm-6.1` snapshot and has diverged. Upstream `wasm` has since moved on to 7.1.5.

## 4. Boot & host runtime

Host code: `refs/linuxontab-2.0-site/linux-dist/dist/{index,wasm,worker,virtio,devicetree,bytes,util,wali-bridge}.js` [V].

**Kernel wasm imports (25)** [V]:

```
env.memory                (shared, min 86 pages, max 65536 — kernel imports the single shared Memory)
boot.get_devicetree  boot.get_initramfs
kernel.breakpoint  kernel.boot_console_write  kernel.boot_console_close  kernel.get_now_nsec
kernel.halt_worker  kernel.spawn_worker  kernel.get_stacktrace  kernel.return_address  kernel.run_on_main
user.compile  user.instantiate  user.call  user.switch_entry  user.call_signal_handler
user.read  user.write  user.write_zeroes            (copy_{from,to}_user / clear_user across Memories)
virtio.setup  virtio.set_features  virtio.enable_vring  virtio.disable_vring  virtio.notify
```

**Exports (13)** [V]: `boot`, `syscall`, `trigger_irq_for_cpu`, `get_thread_area`, `get_args`, `get_args_length`, `__indirect_function_table`, plus the LinuxOnTab additions `get_kdiag`, `get_irq_pending_ptr`, `get_timer_deadline_ptr`, `get_monotonic_ns`, `get_real_ns` and `get_clock_origins`.

**Custom sections** [V]: `.linux.sections` holds a JSON map of initcall, percpu, `__param` and similar ranges, which the loader passes in via the devicetree `chosen.sections`. `.linux.initramfs` holds a 529-byte built-in initramfs.

**Boot sequence** [V] (`index.js` `Machine`):

1. Create a `WebAssembly.Memory({shared:true, initial=maximum=memoryMib})`. The default is 2048 MiB, or 384 MiB on iOS.
2. Copy an external `initramfs.cpio` near the top of memory.
3. Build an FDT in JS that contains memory, `chosen.bootargs`, `rng-seed`, `ncpus`, and one `virtio,wasm` node per device.
4. Instantiate `vmlinux` on the **main thread** and call `exports.boot()`.

`spawn_worker` then creates one module Worker per kernel task. Each Worker re-instantiates the same compiled `vmlinux` module against the same shared Memory, then calls `__indirect_function_table.get(fn)(arg)`, starting at `task_entry`.

Virtio is serviced on the main thread (`virtio.*` imports are "unavailable" in Workers). Interrupts go in through `trigger_irq_for_cpu`. `run_on_main` lets a Worker ask the main thread to call a kernel function pointer.

**Page configuration** [V]:

- Command line: `console=hvc0 loglevel=4 nr_cpus=1 rcupdate.rcu_cpu_stall_suppress=1 rdinit=/init ip=192.168.86.x::192.168.86.1:255.255.255.0::eth0:off lot_epoch=… lot_tty=RxC …`.
- Machine options: `cpus: 1`.
- Devices: `[console, entropy, rootfs-blk, pkgdrop-blk, net]`.
- The comment on `rcu_cpu_stall_suppress` notes that `show_regs()` is unimplemented and that an RCU stall report BUGs, which halts the kernel.

## 5. Tasks, context switching & CPU token

This is the model kSEE plans, already implemented upstream by tombl. Verified from the disassembly of `__switch_to` and `task_entry_inner` in `vmlinux.wasm`, which match the upstream `arch/wasm/kernel/process.c`.

- **One Worker per `task_struct`** [V]. `copy_thread()` calls `wasm_kernel_spawn_worker(&task_entry, bootstrap_args, "comm (pid)", …)`. Idle threads get no Worker.
- **The CPU token is `thread_info->running_cpu`**, an atomic int in shared kernel memory [V]; -1 means the task holds no CPU. `__switch_to(from,to)` works like this:
  1. `xchg(from->running_cpu, -1)`.
  2. `cmpxchg(to->running_cpu, -1, cpu)`.
  3. `memory.atomic.notify(&to->running_cpu, 1)`.
  4. If `wasm_get_thread_done()` is set, `halt_worker`.
  5. Otherwise loop on `memory.atomic.wait32(&from->running_cpu, -1, ∞)` until it is ≥ 0, then `set_current_cpu` and `set_current_task`.

  New tasks wait in the same way in `task_entry_inner`, then call `schedule_tail(prev)`, then `fn(arg)`. For a user task, that is followed by `wasm_user_call()`.
- **The "single CPU token" is `nr_cpus=1`** [V] on a kernel built SMP. The kernel supports N CPUs (`ncpus` defaults to `navigator.hardwareConcurrency` in `Machine`). LinuxOnTab pins it to 1, and the JS comments cite deadlocks and stall issues.
- **Preemption** [V/I]: `ARCH_NO_PREEMPT`. A CPU-bound user program keeps the token until it makes a syscall. The page comments say rust-lld "holds the single CPU for minutes".
- **Pitfalls that LinuxOnTab's JS comments document** [V], all useful for kSEE:
  - A JS `Atomics.wait` futex shim parked the token and deadlocked. It was reverted so the kernel's futex is used.
  - pthread clones that ran user code outside a kernel task context left "phantom runnable" tasks and a lost token. Pthreads are now real kernel tasks entered via `task_entry` and `wasm_call_clone_fn`.
  - A child that traps while holding the token freezes the whole guest.
  - Calling kernel code from a non-task Worker corrupts CPU 0's idle stack. That led to the `clock_origins` export design.

## 6. Userspace, syscalls, fork/vfork via Asyncify thunk

**Process model** [V]:

- Each user process has **its own** `WebAssembly.Memory`. It is shared-flagged so it can be passed to thread Workers, uses up to 4096 pages, and is separate from kernel memory.
- The user module is instantiated **inside the task's Worker**, next to that Worker's kernel instance.
- `linux.syscall` is a JS closure that calls `kernel_instance.exports.syscall(nr,…)` directly, so a syscall is just a function call into the kernel instance in the same Worker.
- The kernel reaches user memory only through the `user.read`, `user.write` and `user.write_zeroes` imports.
- `execve` goes through `binfmt_wasm`, which calls `user.compile(buf,size)` (bytes in kernel memory) and then `user.instantiate(fresh)`.
- Threads (`CLONE_VM`) share the parent's Memory and Module in a new Worker. They enter through `switch_entry(fn,arg)`, and upstream's `clone` syscall takes `fn`/`arg` directly.

**The JS worker shims a number of syscalls** [V]: `clock_gettime64` (computed from the kernel-published origins), `getrandom`, the sigprocmask variant for thread children, `ftruncate`/`renameat` (the wasm-ld output hack) and `settimeofday`. The rest go straight to the kernel.

**fork()/vfork() via the "asyncify thunk"** [V, `worker.js`]. This is **host-side JS, not kernel code**, and it is the main thing LinuxOnTab adds on top of tombl:

- **Triggers:**
  - The pseudo-syscalls **9999 (fork)** and **10000 (vfork)**, issued by a `wasm_fork.c` in libc.
  - `clone(SIGCHLD, 0)`, from `wasm-stubs.c` `fork()`.
  - `clone` with `CLONE_VFORK`, for NOMMU busybox/hush.

  All of them require the binary to export `asyncify_*`, and return ENOSYS otherwise.
- **Parent, phase 1:**
  1. The closure calls `asyncify_start_unwind(buf)` and returns.
  2. The unwind propagates out to the `user.call` loop.
  3. The loop runs `asyncify_stop_unwind()`, then **copies the whole user Memory** (`new Memory({initial: pages}) .set(parent)`).
  4. It sets a "fork override" so that `get_user_memory()` returns the copy.
  5. It calls the kernel's `syscall(SYS_clone, 0, 0, SIGCHLD)`. `copy_thread` then triggers `spawn_worker`, and the child Worker receives `{user_module, childMem, fork_bufPtr, fork_retPtr}`.
  6. It writes the child pid to `retPtr` and calls `asyncify_start_rewind(buf)` on the parent, which re-enters `_start` and rewinds.
- **The child** starts at `task_entry` (a real kernel task). When it reaches `switch_entry`, it sees `forkRewindState`, instantiates the module on the copied memory, and calls `asyncify_start_rewind`. The re-issued fork syscall observes state 2, runs `asyncify_stop_rewind`, and returns 0 (read from `retPtr`).
- **The asyncify stack buffer** is either caller-malloc'd (dynamic ABI, magic `'FORK'`) or a per-Worker 4 MiB region obtained with `memory.grow`, a fix for the collision with mmap'd top-of-memory data.
- **Kernel view:** from the kernel's perspective, fork is `clone(fn=0)` from a task whose user memory the host has already snapshotted. The kernel stays NOMMU, and `vfork` is also handled by copying ("copy for safety").
- **Caveats:** the snapshot is not coherent if other threads share the memory; upstream now refuses `!CLONE_VM` clone when `mm_users > 1`. Every user binary pays the asyncify size and speed cost. Rewind always re-enters from `_start`, or from the WALI thread entry.

## 7. Devices (pts, networking, block overlay)

All virtio devices use tombl's `virtio,wasm` transport, with the host devices in `virtio.js` [V].

- **Console:** virtio-console appears as `hvc0` and feeds xterm.js. `/dev/pts` comes from the kernel's devpts/unix98 pty (symbols verified), which is what makes tmux, mc and ncurses programs work [W, since only the kernel side was verified]. `lot_tty=RxC` is passed on the command line.
- **Entropy:** virtio-rng.
- **Networking ("WispSlirp")** [V]:
  - The guest runs a normal virtio-net `eth0` (192.168.86.100/24).
  - On the main thread, a user-mode SLIRP in JS answers ARP for the 192.168.86.1 gateway.
  - It terminates guest TCP and bridges each connection into a **WISP v1** stream over one WebSocket to `wss://linuxontab-net.fly.dev/wisp`, a Fly.io TCP egress server.
  - UDP/53 is answered through DoH via `relay.linuxontab.com`.
  - Inbound port tunnels go through `tunnel.linuxontab.com`.
  - Sources for the relay and tunnel workers are in `refs/linuxontab/services/`.
- **Block and persistence** [V]:
  - The rootfs is an ext4 image with no journal (`rootfs.ext4`, 600 MiB; or `rootfs-lean.data`, a sparse extent image of a 512 MiB disk with a sha manifest), held entirely in a JS `Uint8Array`.
  - `BlockDevice.write` marks dirty 4 KiB blocks. The "block-delta overlay" stores those dirty blocks in IndexedDB (`lot-rootfs-overlay-v1`), keyed to the base image hash, and reapplies them on boot. `lotCommit(name)` exports the dirty extents as a named image.
  - A second virtio-blk device, "pkgDrop", is a mailbox used to deliver packages into the guest: a write to offset 0 triggers a host fetch.
  - All of this is host-side JS. The kernel just sees ordinary virtio-blk.

## 8. Known limitations

- **No kernel source**, from which it follows that the modified kernel cannot be rebuilt or audited, and GPL source availability is unclear [V].
- **One CPU in practice** (`nr_cpus=1`) and no preemption, so a CPU-bound process stalls everything, including the RCU stall that has to be suppressed [V].
- `show_regs()` is unimplemented, so any stall or oops report halts the kernel [V, per comment].
- **Fork is a JS heuristic** [V]:
  - It needs whole-program asyncify of each binary.
  - It copies the whole memory.
  - It is incoherent with live threads.
  - Rewind resumes only from `_start` or a thread entry.
  - There are several legacy and dynamic fork-ABI variants.
- **Many hotfixes in the runtime** [V]: an Xvfb bytecode patch, a scan for wasm-ld output, clock sanity clamps, per-binary memory maxima, and the SJLJ runtime living in the worker. The code is fragile, with a large debugging surface (`?debuglog`, `/log` POSTs, `__lotKernel` handles).
- **External infrastructure dependencies** [V]: networking needs the Fly.io WISP server and the Cloudflare DoH relay.
- **Diverged from upstream** [V/I]: it is based on an older `wasm-6.1` snapshot with a different user ABI (`compile` vs `compile_begin…end`). Upstream `tombl/linux` has since gained EROFS, ticket locks, atomic SMP barriers, network devices in-tree and moved to 7.1.5, and none of that is in LinuxOnTab.
- The kernel mmap is NOMMU. User mmap/brk are mostly `memory.grow` inside musl or the WALI bridge, and `MAP_SHARED` is broken (hence the wasm-ld hack) [V].

## 9. Relevance to kSEE and fork-suitability verdict

**Architecture match: near-exact, because it *is* tombl's architecture.** kSEE's plan is arch/wasm with wasm32 NOMMU, task_struct ↔ Worker, one shared kernel Memory, a CPU token handed off on context switch, UP-first with no preemption, and optionally fork via Asyncify of userspace only. That is precisely tombl/linux, which LinuxOnTab runs with `nr_cpus=1`, plus LinuxOnTab's own userspace-asyncify fork. Everything LinuxOnTab contributes on top of tombl lives in **host JS**, which is readable, and in the userspace build recipes, which are in the repo.

**What kSEE should take from LinuxOnTab (as reference, not as a base):**

1. **Proof that userspace-only Asyncify fork works** at scale (busybox/hush `$()`, tmux, Python, clang) on this kernel model. The protocol is a useful blueprint, provided the kernel `clone` gains a "fork-from-snapshot" mode. See `worker.js` around the 9999/10000 and `clone(SIGCHLD)` intercepts.
2. Lessons about the CPU token, documented in comments:
   - Never block a Worker in JS while the kernel thinks the task is running.
   - Make every thread a real kernel task.
   - A trap in a token-holding Worker must release the token or the guest dies.
   - Do not call kernel code from Workers that are not in a task context. Publish data (for example, clock origins) instead.
3. Host device patterns: WISP-backed SLIRP over virtio-net, and a block-delta overlay in IndexedDB. Both are independent of the kernel.

**Why not fork LinuxOnTab itself:**

- There is no kernel source.
- It is based on a stale `wasm-6.1` snapshot.
- The runtime is heavily hotfixed and single-author.
- Its value-add is host-side and can be ported onto a fresh tombl base.

The right fork base is **tombl/linux upstream** (see `tombl-linux.md`, rated 5/5). LinuxOnTab should be treated as a field report and a design reference for fork, networking and persistence.

**Fork-suitability verdict: 2 / 5.** It scores 5 on architecture fit, but the missing kernel source, the divergence from upstream and the hotfix-laden runtime make it unsuitable as the codebase to fork. Use tombl/linux as the base, and mine LinuxOnTab's `worker.js` (Asyncify fork), WispSlirp and the overlay code for ideas.

---

### Artifacts saved

The following are under `refs/linuxontab-2.0-site/`:

- `next.linuxontab.com_.html` (the app page, 258 KB, with WispSlirp, the overlay and the Machine config inline)
- `next.linuxontab.com_console.html.html`
- `linuxontab.com_2.0_.html`
- `linux-dist/vmlinux.wasm` (the kernel actually loaded, 6.1.132+)
- `linux-dist/rootfs-lean.manifest.json`
- `linux-dist/dist/*.js` (the host runtime)
- `linux-dist/dist/vmlinux-DKGM2TYZ.wasm` and `worker-Q7XWPLKR.js` (an older hashed build, 6.1.132 built by Nix, with 7 exports and no LinuxOnTab kdiag or clock additions; not loaded by the current `index.js`)

The rootfs images (the 600 MiB `rootfs.ext4` and the lean `.data`) and the initramfs were not downloaded.
