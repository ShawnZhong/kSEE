# The host

`ksee-host` is the browser host for the kernel. It owns the shared memory,
boots a Linux WebAssembly instance, schedules its suspended task stacks, drives
lesson syscall runners, and exposes snapshots to the page.

For the Linux side of this contract, see [kernel.md](kernel.md). For replay data
structures, see [replay.md](replay.md).

## One thread, one memory

The runtime is Rust compiled into the outer WebAssembly module on the page's
thread. Linux is a second WebAssembly instance that imports the outer module's
memory.

The layout is:

```text
runtime static data and stack
kernel image and 64 MiB RAM
runtime heap above kernel RAM
```

`CONFIG_WASM_HOST_MEMORY` reserves the lower runtime region before the kernel
image. The runtime can read kernel objects and the trace ring directly. There
is no worker, message channel, copied snapshot memory, or shared-memory
cross-origin-isolation requirement.

Host code never runs concurrently with the kernel. It runs only when a kernel
stack is suspended. The required browser capability is JSPI,
`WebAssembly.Suspending`; browser version numbers are not part of the runtime
contract.

## Host modules

```text
machine.rs    boot, restart, console, syscall requests, quiet, snapshots
sched.rs      kernel task stacks, switches, idle, interrupts, virtual clock
imports.rs    imports called by Linux, including host-side trace stores
programs.rs   user modules, per-task user stack pointers, syscall runners
replay.rs     Machine::view, Machine::at_moment, and snapshot lookups
state.rs      machine state shared by those modules
js.rs         small Web API reflection helpers
test/         the kernel's test suite (`ksee test`'s `kernel`) and its harness
```

The rules of a program run (its runners, heaps, calls, names, each line's
outcome) are `ksee_core::program::Session`, which the state holds; the
kernel's timer layout is `ksee_core::arch::sleeping`, and a module's custom
sections `ksee_core::wasm`: all tested natively.

Page modules and Dioxus state are documented in
[development.md#page-modules-and-state](development.md#page-modules-and-state).

## Boot and restart

JavaScript fetches and decompresses the packed kernel and initramfs, then
`Machine::start` receives their bytes. It grows the outer memory to the
product's fixed 64 MiB of kernel RAM, compiles the kernel, places the initramfs
on the built-in command line, and parses the packed `ksee.probes` and
`ksee.types` custom sections once.

Each boot then:

1. zeros the kernel memory range;
2. copies the initramfs to the top of RAM and adds its location to the command
   line;
3. instantiates Linux with the runtime imports;
4. discovers exported state such as the trace, current task, pending interrupt,
   timer deadline, stack pointer, and function table;
5. starts the boot CPU through the suspending `boot` export; and
6. waits until `/bin/prog` registers its main runner and the kernel becomes
   quiet.

Restart stops the scheduler, abandons suspended stacks from the old instance,
clears runtime state and the last snapshot, wipes kernel memory, and boots a
new instance. A stack belonging to a stopped machine is never resumed.

## JSPI task scheduling

Each Linux task is a call stack of the kernel instance. At
`__switch_to(prev, next)`, Linux calls the suspending `switch_task` import. The
runtime saves the outgoing kernel and user stack pointers, suspends that
promise-backed stack, and resumes the next task's stack. A new task starts at
`task_entry`; a dead task's suspended stack is dropped.

The idle task suspends through the `idle` import. The runtime resumes a task
when a pending interrupt or task switch makes work available. Because one
WebAssembly stack runs at a time, the kernel remains uniprocessor and host code
cannot observe it halfway through ordinary instructions.

The runtime raises interrupts by setting `wasm_irq_pending`. Linux takes them
when it next enables interrupts. The doorbell interrupt wakes a runner whose
request changed. The timer interrupt follows a virtual clock: reads advance it
slightly, and idle can skip to an eligible sleeping runner's timer deadline.

## User programs and runners

Linux loads freestanding position-independent WebAssembly user programs into
kernel-managed memory. The runtime compiles and instantiates each module where
Linux mapped its data, places its functions in the kernel table, and preserves
a user `__stack_pointer` value per task.

`/init` starts `/bin/prog`. A prog instance registers a request structure and
program heap through `ksee.ready`. The main instance becomes the default
runner. A clone child or exec'd prog can register another runner.

`Machine::make` handles one chronological lesson step. A declaration allocates
the task's program memory. A syscall writes the number and arguments into the
runner's request, records those host
stores, and raises the doorbell. The runner wakes from its futex and calls the
ordinary kernel syscall export.

A secondary lesson step names the result-bearing call that created its task.
The runtime resolves that structured task name to the active runner; task
selection is not part of the program expression language. See
[lesson-authoring.md](lesson-authoring.md) for the JSON and syntax.

## Recording boundaries

The runner brackets its syscall with `ksee.begin` and `ksee.end(ret)`.
While a call the page asked for is under way (a runner between its `begin`
and its `end`), the kernel records everything (its `ksee_record(1)`
export): blocks, calls, returns, variables, comparisons, and stores, from a
`Task` record naming the running `task_struct`, because no context switch
in the recording necessarily identifies it. Between calls it records stores
only (`ksee_record(2)`), so memory as it was stays exact while the runners'
waits and the kernel's own work then are not shown. The boot, before the
program's first line, is not recorded. Host writes to kernel memory are logged by the kernel's
`ksee_bulk` export before they are made, as its own bulk writes are, so
historical memory stays coherent across request setup and program
variables.

A requested syscall can remain blocked when the kernel becomes quiet. A later
step on another runner may wake it. Runtime call state records whether each
request returned, ended through `execve`, or disappeared with an exiting task.

## Quiet and snapshots

`Machine::quiet` resolves when the scheduler is idle and no immediately
eligible kernel work remains. It returns every requested call completed since
the previous wait. Quiet does not mean every requested call has returned; a
pipe read or futex wait can be blocked pending a later lesson step.

`Machine::snapshot` decodes the current trace ring into a new `Recording`. The
snapshot owns decoded records, stable record IDs, call arguments, inferred task
identity, calls, runs, line-step indexes, and lazy store history. The live
kernel and trace ring can continue after that snapshot is taken.

`Machine::run_of_call_id` reconnects a runtime request ID to its syscall in the
latest snapshot. Stable trace record IDs make that lookup resilient when ring
indexes move as older data is overwritten.

## Page boundary

`ksee-page` is compiled into the same outer Rust WebAssembly module and receives
the booting `Machine` as a promise. It calls the runtime directly, Rust to Rust.
JavaScript loads the module, starts the machine, and provides the static frame;
it does not maintain a parallel replay model.

The current replay boundary is closure-based:

- `Machine::view` lends a `ksee_core::view::View` over the last snapshot and
  current end-state memory.
- `Machine::at_moment` additionally reconstructs memory after a selected record
  and lends both the view and that historical memory.

The page uses these methods for syscalls, reconstructed lines, the timeline,
stacks, tasks, objects, notes, origins, source definitions, and run-to-line
lookups. Historical memory is available only through `Machine::at_moment`.

## Determinism

Host input enters only as explicit page requests while Linux is suspended. The
clock is virtual, one CPU runs one task at a time, and no external device or
console input changes the run. A fresh machine given the same ordered lesson
steps reaches the same trace and state record for record.

Going backward therefore means inspecting reconstructed history or restarting
and replaying the same earlier steps before choosing a different next step. It
does not restore browser-held task stacks in place.

## Imports

Frequent Linux imports such as `console_write`, `time_ns` and `spawn_task`
are exports of the outer Rust WebAssembly module, so Linux calls
them directly. Suspending scheduler operations and browser-facing work use
closures and promises. The complete kernel-side import/export list is in
[kernel.md#the-host-interface](kernel.md#the-host-interface).
