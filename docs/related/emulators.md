# CPU-emulator approaches to Linux in the browser

Surveyed 2026-09-24. In every system below, **the emulator is what gets
compiled to (or generates) WebAssembly**. The guest kernel is an ordinary
native binary for x86, RISC-V or arm64, and wasm never sees it as code, only as
bytes in emulated RAM. Facts come from project READMEs and technical pages
fetched today, plus the kSTEP playground README. Inferences are marked.

The playground README the brief pointed to at `/home/ubuntu/web/README.md`
does not exist. The playground lives at `/home/ubuntu/kstep/website/README.md`,
and all kSTEP numbers below come from that file.

## Overview

| System | What is wasm | Guest CPU | Guest kernel | Execution | License |
|---|---|---|---|---|---|
| v86 | emulator core (Rust→wasm) + **runtime-generated wasm per hot guest page** | x86 32-bit (≈Pentium 4, SSE3), UP | any unmodified x86-32 OS (Linux, BSDs, Windows 9x/2000, …) | interpreter + x86→wasm JIT | BSD-2-Clause |
| JSLinux (Bellard) | TinyEMU-derived emulator compiled with Emscripten | x86_64 (since 2026-01), x86, riscv64/32 | unmodified Linux (Alpine, Buildroot, Fedora), Windows 2000, FreeDOS | interpreter (no JIT is documented) | TinyEMU MIT; x86 core not published (inference) |
| WebVM / CheerpX | CheerpX engine (proprietary) + x86→wasm JIT | x86 32-bit **user mode** | **none**: CheerpX emulates the Linux syscall ABI itself | two-tier interp + JIT | WebVM frontend Apache-2.0; CheerpX proprietary, free for individual/tech exploration |
| container2wasm | Bochs / TinyEMU / QEMU compiled with wasi-sdk or Emscripten | x86_64 (Bochs), riscv64 (TinyEMU), any QEMU target (`--to-js`) | real Linux (c2w-built, e.g. `6.1.0`) + runc running the container | interpreter (Bochs/TinyEMU) or QEMU TCG JIT | Apache-2.0 |
| QEMU-wasm | QEMU system emulator via Emscripten + **TCG backend emitting wasm modules** | any QEMU target (x86_64, aarch64, riscv64, …); MTTCG SMP | unmodified Linux (or anything QEMU boots) | TCG JIT to wasm (ktock) or TCI interpreter (upstream) | GPL-2.0 (QEMU) |

## v86

- **Links:** https://github.com/copy/v86 (Fabian Hemmer, `copy`; about 23.5k
  stars, active, pushed 2026-09). Demos: https://copy.sh/v86/. How it works:
  `docs/how-it-works.md`.
- **Architecture:**
  - The CPU and most devices are in Rust compiled to wasm, with JS glue for
    devices and the UI.
  - The interpreter records entry points and per-page hotness. When a page gets
    hot, v86 generates **one wasm module for that page plus up to `MAX_PAGES`
    directly reachable pages**. Each module is a single function with a
    `br_table` switch over entry points and a "stackifier" for structured
    control flow.
  - Modules are not linked to each other. Execution leaves a module, and the
    main loop picks the next one.
  - Paging is fully emulated: memory accesses go through a 4 MB software TLB
    with a fast path, and a slow path that walks page tables and raises faults.
  - Lazy flags. The x87 FPU is emulated with SoftFloat, which is slow.
- **CPU:** x86 around Pentium 4 level with SSE3. It has no 64-bit mode, no
  multicore, no single-step, no trap flag and no debug registers. Partial APIC.
- **Devices:** IDE, floppy, PS/2, VGA/SVGA, NE2000, SB16, virtio
  9p/net/balloon. Emulator state can be saved and restored.
- **Guest kernel:** stock i386 Linux (Buildroot, Arch 32, DSL) and many other
  OSes.
- **Performance:** no official figures. The design notes say browsers handle
  deep `br_table` modules poorly and that module generation is slow and costs
  memory per module. Expect a large factor below native (inference: roughly one
  to two orders of magnitude on CPU-bound code, based on how interpreter-plus-JIT
  emulators usually compare).
- **Debug/introspection:** there is no gdbstub, and single-step and debug
  registers are unsupported. Guest state is reachable from JS through the
  emulator API (registers, memory). Symbolising it would require the host to
  parse `vmlinux` DWARF itself (inference).

## JSLinux (Fabrice Bellard)

- **Links:** https://bellard.org/jslinux/ (VM list),
  https://bellard.org/jslinux/tech.html, https://bellard.org/jslinux/news.html,
  and TinyEMU at https://bellard.org/tinyemu/.
- **History:**
  - 2011: hand-written JS x86 emulator.
  - 2015: rewritten in asm.js.
  - 2016: TinyEMU (RISC-V) ported through Emscripten, adding the VirtIO 9P
    filesystem.
  - The x86 emulator was later rewritten in C and compiled back to JS/wasm with
    Emscripten, and turned out faster than the hand-written asm.js.
  - WASM enabled 2018-08.
  - 64-bit x86 on 2026-01-12.
  - AVX2, AVX-512 and APX on 2026-03-09.
- **CPU:**
  - The x86_64 core has x87 with bit-exact 80-bit floats, PAE+NX, AMD SVM with
    nested paging, RDPMC and SSE through AVX-512/APX. It has no segment
    limit/rights checks and no debug registers.
  - RISC-V is 32/64-bit with FPU and compressed instructions.
- **Devices:** 8259, 8254, UART, RTC, PCI, VirtIO
  console/9P/net/block/input, simple framebuffer, and optional IDE/PS-2/VGA.
- **Guest kernel:** unmodified Linux (Alpine 3.23.2 x86_64, Alpine 3.12 x86,
  Buildroot and Fedora 33 riscv64), Windows 2000, FreeDOS. Networking goes
  through a websocket VPN capped at 40 kB/s.
- **Performance:** "about 100 MIPS" for the x86 emulator on a 2017 desktop
  under Firefox. The guest has a `vmtime` benchmark tool. No JIT is documented,
  so it is inferred to be an interpreter.
- **Source:** TinyEMU is open (MIT), but its x86 support targets KVM. The
  JSLinux x86 core itself does not appear to be published (inference: no source
  link exists on the site).
- **Debug/introspection:** none exposed.

## WebVM / CheerpX (Leaning Technologies)

- **Links:** https://github.com/leaningtech/webvm (about 17.4k stars, active,
  pushed 2026-09), https://webvm.io, https://cheerpx.io/ (npm
  `@leaningtech/cheerpx`), and the JSNation 2022 talk "Crafting the Impossible:
  X86 Virtualization in the Browser with WebAssembly".
- **Architecture.** CheerpX provides four pieces:
  - an x86→wasm JIT;
  - a "virtual block-based file system" (ext2 images streamed over HTTP, with a
    local overlay);
  - a **Linux syscall emulator**;
  - a sandbox.

  WebVM runs unmodified 32-bit x86 Debian or Alpine userlands, including
  toolchains and Xorg in WebVM 2.x. Networking goes through Tailscale (or
  headscale) over WebSocket.
- **Guest kernel: none.** This is user-mode emulation, like `qemu-user`, with a
  proprietary reimplementation of Linux semantics (processes, fork, signals,
  files) inside CheerpX. Linux kernel code never runs.
- **Performance:** a two-tier interpreter plus JIT. Leaning Tech's blog posts
  claim near-usable speeds for compilers and Python, but no neutral numbers
  exist (inference). Among these systems it is probably the fastest at
  userspace-heavy work, because no guest kernel or device emulation sits in the
  path (inference).
- **License:** the WebVM repo is Apache-2.0. CheerpX is proprietary: free for
  individuals and technological exploration, a license is required for
  organisations (academia included), and self-hosting a CheerpX build needs a
  commercial license.
- **Relevance:** close to zero for kSEE, since there is no Linux kernel to
  observe. It is useful only as a UX reference (disk streaming, networking).

## container2wasm (Kohei Tokunaga, `ktock`)

- **Links:** https://github.com/container2wasm/container2wasm (moved from
  `ktock/container2wasm`; about 2.8k stars, active, pushed 2026-09). Demo:
  https://ktock.github.io/container2wasm-demo/.
- **What:** `c2w <image> out.wasm` converts an OCI container image into a wasm
  image. It runs on WASI runtimes (wasmtime, wamr, wasmer, wasmedge, wazero) or
  in the browser.
- **Stack:**
  - An emulator compiled to wasm: Bochs for x86_64, or TinyEMU for riscv64,
    built with wasi-sdk (the patched forks are `ktock/Bochs` and
    `ktock/tinyemu-c2w`). With `--to-js` it uses QEMU-wasm built by Emscripten,
    **with the TCG JIT**.
  - A real Linux guest kernel (the example prints `Linux localhost 6.1.0 …
    x86_64`), with runc starting the container.
  - Other architectures run through binfmt QEMU inside the guest, which adds
    another layer of emulation.
  - Packaging uses wasi-vfs or the Emscripten FS. For WASI, the kernel is
    pre-booted at build time with **wizer**, which snapshots the initialised
    instance to cut startup time.
  - Networking uses `c2w-net` / `c2w-net-proxy` (gvisor-tap-vsock), either via
    browser Fetch or a WebSocket delegate.
- **Guest kernel:** real, stock-configured Linux built by c2w.
- **Performance:** the Bochs and TinyEMU paths are interpreters (slow). The
  QEMU TCG path is much faster (see below). The README warns that containers of
  other architectures are "slow because of additional emulation".
- **Relevance:** it shows that snapshotting a pre-booted kernel (wizer, or the
  QEMU migration stream in kSTEP) removes boot latency. kSEE can do the same
  with `vmlinux.wasm` memory snapshots (inference).

## QEMU-wasm (ktock) and the wasm64 TCG JIT branch

- **Links:**
  - https://github.com/ktock/qemu-wasm (about 400 stars): the original patched
    QEMU, 32-bit wasm host, with a TCG backend that emits wasm, MTTCG, and
    Asyncify plus fibers for coroutines. Demo:
    https://ktock.github.io/qemu-wasm-demo/ (x86_64, aarch64 including
    Raspberry Pi, and riscv64 guests).
  - https://github.com/ktock/qemu branches `wasm64-tcg-a`, **`wasm64-tcg-b`**,
    `wasm64-tcg-b-v3-bench`, `wasm64-tci-a`, … (checked with `git ls-remote`
    today). These are the rework against the upstream Emscripten port.
- **Upstream status.** Upstream QEMU builds for a wasm64 host under Emscripten
  **with TCI only** (the TCG interpreter). This is from the kSTEP README;
  inference: merged in the QEMU 10.x cycle. ktock's `wasm64-tcg-b` adds a real
  TCG backend: each translation block (TB) is compiled to a wasm module and
  instantiated at run time.
- **How kSTEP uses it** (`/home/ubuntu/kstep/website/README.md`, measured by the
  user):
  - **Build.** `kstep-dev/qemu` branch `kstep` is upstream **v11.1.1** plus
    `wasm64-tcg-b` squashed into one commit, plus one tuning commit: 1 GB wasm
    heap, JIT threshold 300, 4 virtio-mmio slots, and only the `virt` machine
    and virtio console. It is built with
    `--enable-wasm64-32bit-address-limit` (64-bit C pointers, wasm32 output) so
    it runs without Memory64 (Safari, older Chrome/Firefox). The site ships a
    **13 MB** QEMU build. emsdk is pinned to 4.0.23 with one JS patch (a leaked
    `setTimeout` per `poll()`, which grew to 1.6 GB of JS heap after 90 s);
    emsdk 6.0.10 booted 25% slower and answered commands 30–45% slower.
  - **Guest.** arm64 `virt` with the 6 MB `Image`, 64 MB guest RAM, and a
    64 MB translation cache. The run peaks at about 0.6 GB of process memory.
    arm64 was chosen over x86 `pc`: it reaches the kmod about 1 s sooner, with
    no firmware, ACPI/PCI enumeration or TSC calibration.
  - **Threads.** MTTCG runs one Web Worker per vCPU. That needs
    SharedArrayBuffer, so COOP/COEP headers are supplied by a service worker on
    GitHub Pages.
  - **Timings:**

    | Measurement | Value |
    |---|---|
    | Cold boot | ~4 s (3.6 s MTTCG) |
    | Resume from a migration-stream snapshot at the driver's ready line (`snap-5.bin.gz`, ~3 MB) | **~0.3 s** |
    | Scheduler `tick` command | ~2.3 ms |
    | Full scripted run in Node | **8–14 s**, vs ~1 s for native QEMU TCG (**≈8–14× slower than native TCG**, which is itself slower than KVM) |
    | Bench noise, run to run | ±30% |

  - **Single-threaded TCG** (`thread=single`) is worse: boot 7.4–9.5 s against
    3.6 s, and `tick` 4.8 ms against 2.3 ms. It also exposed a backend bug: the
    TB prologue caches `env` in a wasm global only when that global is zero.
  - **TCI** is "about 4× slower" than the wasm TCG JIT.
  - **Profile.** The vCPU running the driver is 80% busy. Of that busy time:

    | Where | Share |
    |---|---|
    | JIT'd guest code | 46% |
    | TB dispatch (`cpu_tb_exec`, `helper_lookup_tb_ptr`) | 9% |
    | JS glue (Asyncify save/rewind, BigInt pointer conversion, `ffi_call_js` for helpers, JS longjmp) | 10% |
    | softmmu and helpers | the rest |

    `-O3` and `-sSUPPORT_LONGJMP=wasm` made no measurable difference. The
    levers left are JIT code quality and wasm-to-wasm TB chaining (tail calls).
  - **Fidelity.** "Traces match native QEMU of the same version byte for byte."
  - **Instrumentation path.** A guest kmod and driver emit JSON records over a
    virtio console port. There is one virtqueue kick per write; the 16550 cost
    one port-I/O exit per byte. Emscripten device-node callbacks receive the
    bytes with no polling. A decoder (`crates/core` built for wasm32) turns
    machine state into UI records.
- **Guest kernel:** unmodified upstream Linux (kSTEP builds v5.15 through
  v6.18 for arm64), plus the kSTEP kmod.

## Why kSEE does not take this route

1. **Guest execution is opaque.** An emulator executes guest *machine code*.
   Its wasm is either the emulator itself or JIT'd TBs of translated
   aarch64/x86 instructions. Nothing in that wasm corresponds to a kernel C
   function, line or variable. The browser's devtools, wasm DWARF and source
   maps describe QEMU, not Linux.
2. **Source/object correlation needs guest-side introspection.** Tying events
   to `kernel/sched/fair.c:NNNN` and live `struct task_struct` fields takes one
   of these:
   - **(a) a gdbstub-style debugger.** QEMU has one, but in the browser it
     needs a custom chardev bridge (inference: TCP is unavailable). kSEE's JS
     would also need a DWARF reader for `vmlinux`, page-table walks for kernel
     VAs, and breakpoints or single-step through TCG. Each stop exits the JIT,
     and per-line stepping is orders of magnitude slower still (inference).
   - **(b) an in-guest agent.** This is kSTEP's kmod-plus-virtio-JSON design.
     It gives semantic events but only at the points the agent instruments, and
     every event crosses a device boundary and needs a decoder that tracks the
     kernel's struct layouts.
   - **(c) a host-side memory scraper.** Guest RAM sits in the emulator's
     linear memory, so the host can read it and decode it with `vmlinux`
     DWARF/BTF. This is possible, but it has to do virt→phys translation and
     know the struct layout for each kernel version (inference).

   With a direct `arch/wasm` build, kernel C *is* the wasm. Function-level
   correlation is inherent (wasm function names and DWARF come straight from
   Clang), kernel objects live at known addresses in a `WebAssembly.Memory` the
   UI can read directly, and instrumentation hooks are ordinary wasm imports
   with no device round-trip.
3. **It is slower, and the cost is structural.** Every guest instruction goes
   through translation, softmmu (a TLB lookup on every load and store) and TB
   dispatch, plus the emulated platform: firmware, interrupt controller, timer
   device and virtio. kSTEP measured ≈8–14× slower than native TCG on a
   kernel-heavy workload. Compiled-to-wasm kernel C should run at roughly
   native-wasm speed (inference; typically 1.5–3× slower than native for C,
   unmeasured for kSEE). That speed matters for kSEE's always-on event
   streaming.
4. **Scheduling is not visible as Workers.** Under MTTCG a Worker is a *vCPU*.
   Guest task switches happen inside JIT'd code and are invisible to the host
   unless the guest reports them. kSEE's one-Worker-per-task model makes
   `schedule()` a host-visible event (the CPU-token handoff) by construction.
5. **What the emulator route does win:**
   - **zero kernel changes:** any arch, any version, including kSTEP's
     v5.15–v6.18;
   - full MMU, fork/COW, page faults and SMP semantics, which NOMMU `arch/wasm`
     cannot show;
   - bit-exact fidelity to a native QEMU run;
   - no porting or maintenance of an arch tree.

   kSEE gives these up deliberately, in exchange for correlation and speed.

## What it is still good for

- **Ground-truth baseline.** Run the *same* kernel source and config (the arm64
  build of the version kSEE's `arch/wasm` targets) under QEMU-wasm or native
  QEMU, then compare what the two kernels do:
  - syscall traces, the order of scheduling decisions (the `sched_switch`
    sequence for a given workload), wakeup chains, and VFS/page-cache paths;
  - the output of `/proc` and `tracefs`.

  Differences point at `arch/wasm` bugs, or at artifacts of NOMMU, UP or the
  virtual timer. kSTEP's result that traces "match native QEMU byte for byte"
  means the emulator path counts as real-hardware-equivalent ground truth for
  arch-independent code.
- **Semantic-event oracle.** The same kSEE event schema could be emitted from
  a guest kmod (the kSTEP pattern) or from tracepoints. That lets kSEE check
  that its `arch/wasm` instrumentation reports the same events, in the same
  causal order, as a stock kernel.
- **Performance and boot baseline.** It gives a yardstick for `vmlinux.wasm`
  boot time and per-event overhead:
  - QEMU-wasm boot ~3.6–4 s cold, 0.3 s from a snapshot;
  - scheduler ticks at ~2.3 ms.

  The snapshot trick (a QEMU migration stream, or container2wasm's wizer
  pre-boot) is worth copying for `vmlinux.wasm`: snapshot the memory after
  boot, then instantiate from it.
- **Features NOMMU `arch/wasm` cannot show.** fork/COW, demand paging, `mmap`,
  TLB shootdowns and SMP load balancing can be shown by an emulator-backed "full
  machine" mode, or in kSTEP (inference: a possible second backend, not a v1
  goal).
- **Reuse of kSTEP's browser plumbing.** The service-worker COOP/COEP shim,
  Worker threading, device-node byte streams and the headless Node test harness
  (`run.mjs --check`, `pagetest.mjs`) all carry over to kSEE's runtime.
