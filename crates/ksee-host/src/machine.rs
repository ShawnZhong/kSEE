//! The page's API: boot a kernel (one instance, its tasks on this thread:
//! see sched.rs), drive it (console, syscalls, recording), and take
//! snapshots of its recording (see replay.rs).

use std::cell::RefCell;

use js_sys::{Promise, Uint8Array, WebAssembly};
use ksee_core::recording::Recording;
use ksee_core::tables::Tables;
use ksee_core::program::{lower, Arg, Lowered, Outcome, ProgramVar, Request, Runner, Sym};
use ksee_core::{trace, Memory};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

use crate::imports;
use crate::js;
use crate::sched;
use crate::state::*;

/// A booted machine. Its views of the recording are in replay.rs.
#[wasm_bindgen]
pub struct Machine {
    /// The kernel, compiled (a new instance of it each boot); and the
    /// initramfs (at the top of RAM).
    module: WebAssembly::Module,
    initrd: Vec<u8>,
    /// The disk as built (each boot's starts as it).
    disk: Disk,
    /// The last snapshot of the trace.
    pub(crate) rec: RefCell<Recording>,
}

/// Where the built initramfs goes: at the top of kernel RAM, page-aligned.
fn initrd_at(initrd: &[u8]) -> Result<u32, JsValue> {
    let len = u32::try_from(initrd.len()).map_err(|_| JsValue::from_str("initramfs is too large"))?;
    RAM_BYTES.checked_sub(len).map(|at| at & !0xfff).filter(|&at| at >= KERNEL_BASE)
        .ok_or_else(|| JsValue::from_str("initramfs does not fit in RAM"))
}

/// Where this module's static data is, and its stack (a local's, here: not
/// an async fn's, whose locals are its future's, on the heap).
#[inline(never)]
fn layout() -> (u32, u32) {
    static DATA: u8 = 0;
    let local = 0u8;
    (core::hint::black_box(&DATA) as *const u8 as u32, core::hint::black_box(&local) as *const u8 as u32)
}

#[wasm_bindgen]
impl Machine {
    /// Boot the built kernel and initramfs bytes in its RAM, with the disk
    /// image; the kernel's tables, the pack's.
    pub async fn start(kernel: Uint8Array, tables: Uint8Array, initrd: Uint8Array, disk: Uint8Array) -> Result<Machine, JsValue> {
        if js::webassembly("Suspending").is_undefined() {
            return Err("this browser has no JSPI (WebAssembly.Suspending): kSEE needs Chrome 137, Firefox 153, Safari 27 or later".into());
        }
        // The kernel's RAM: this module's memory between its stack and its
        // static data (its heap after the data).
        let (data, stack) = layout();
        if data < RAM_BYTES || stack >= KERNEL_BASE {
            return Err(format!("the runtime's static data ({data:#x}) or stack ({stack:#x}) is in the kernel's RAM: link it with --global-base=RAM_BYTES (.cargo/config.toml)").into());
        }

        let module: WebAssembly::Module =
            JsFuture::from(WebAssembly::compile(&kernel)).await?.unchecked_into();
        // (Read where they are: their bytes kept, as the tables are.)
        let bytes = ksee_core::flat::kept(tables.length() as usize, |b| tables.copy_to(b));
        let tables = Tables::from_bytes(bytes).ok_or("the kernel's tables are not this build's")?;
        TABLES.set(tables).map_err(|_| "one machine per page")?;

        // The initramfs goes at the top of RAM; the kernel reserves it.
        let m = Machine { module, initrd: initrd.to_vec(), disk: Disk::new(disk.length() as usize, |at, buf| disk.subarray(at as u32, (at + buf.len()) as u32).copy_to(buf)),
                         rec: Default::default() };
        m.boot().await?;
        Ok(m)
    }

    /// Boot: kernel memory with the initramfs (zeros else: as it began, or
    /// zeroed by restart), a new kernel instance (instantiating initializes
    /// its data), and its boot CPU; done once the program (user/prog.c) is
    /// ready for calls.
    async fn boot(&self) -> Result<(), JsValue> {
        let initrd = initrd_at(&self.initrd)?;
        unsafe { core::ptr::copy_nonoverlapping(self.initrd.as_ptr(), initrd as *mut u8, self.initrd.len()) };
        let inst: WebAssembly::Instance =
            JsFuture::from(WebAssembly::instantiate_module(&self.module, &imports::kernel())).await?.unchecked_into();
        let kernel = inst.exports();
        let global = |name: &str| js::get(&js::get(&kernel, name), "value").as_f64().unwrap_or(0.0) as u32;
        // (Where the kernel's memory starts, its CONFIG_WASM_HOST_MEMORY: as
        // the host has it, KERNEL_BASE.)
        if global("__global_base") != KERNEL_BASE {
            return Err(format!("the kernel starts at {:#x}, not KERNEL_BASE ({KERNEL_BASE:#x}): build both alike", global("__global_base")).into());
        }
        // (The machine, as the kernel reads it as it boots: its struct
        // wasm_machine, asm/wasm.h's, its fields where its types say.)
        let at = global("wasm_machine");
        let field = |f: &str| tables().types.field("wasm_machine", f).map(|(off, size)| (at + off, size))
            .ok_or_else(|| JsValue::from(format!("the kernel has no wasm_machine.{f}")));
        let ((end, _), (sectors, _), (line, room)) = (field("memory_end")?, field("disk_sectors")?, field("cmdline")?);
        // (The command line: where the initramfs is, the kernel reserving it.)
        let cmdline: Vec<u8> = format!("initrd={initrd:#x},{}", self.initrd.len()).bytes().take(room as usize - 1).chain([0]).collect();
        unsafe {
            *(end as *mut u32) = RAM_BYTES;
            *(sectors as *mut u64) = self.disk.len as u64 / 512;
            core::ptr::copy_nonoverlapping(cmdline.as_ptr(), line as *mut u8, cmdline.len());
        }
        // (The trace's ring, its struct ksee_ring's fields, where its types say.)
        let ksee_ring = global("ksee_trace");
        let ring_field = |f: &str| tables().types.field("ksee_ring", f).map(|(off, _)| ksee_ring + off)
            .ok_or_else(|| JsValue::from(format!("the kernel has no ksee_ring.{f}")));
        let ring = trace::Ring { pos: ring_field("pos")?, words: ring_field("words")?, ring: ring_field("ring")? };
        let trace = ring.pos;
        let (current, pending, deadline) = (global("__current_task"), field("irq_pending")?.0, field("timer_deadline")?.0);
        // (Nothing of a machine before it but the ids of the calls asked:
        // they are not used again.)
        state_mut(|s| *s = State { trace, ring, current, pending, deadline, waiting: Some(0), booting: true,
                               session: s.session.again(), disk: self.disk.clone(),
                               quiet_waiters: std::mem::take(&mut s.quiet_waiters), ..Default::default() });
        crate::programs::instantiated(&kernel);
        imports::instantiated(&kernel);
        sched::boot(&kernel)?;
        // (Quiet, with the program ready.)
        while state(|s| s.session.main).is_none() {
            self.quiet().await?;
        }
        Ok(())
    }

}

impl Machine {
    /// Start the kernel again, from scratch: the machine stopped (its tasks
    /// dropped, and what it had under way let end: it only suspends, for
    /// ever), the runtime's state emptied, and a new boot.
    pub async fn restart(&self) -> Result<(), JsValue> {
        sched::stop();
        // (Once what was under way has had its turn of the event loop.)
        js::sleep(0).await;
        sched::stop();
        // (What waits for the old machine gives up.)
        settle(Some("the kernel started again".into()));
        *self.rec.borrow_mut() = Default::default();
        // (Kernel memory as new. Not at the first boot: zeros as it began,
        // not one page of it touched until the kernel does.)
        unsafe { core::ptr::write_bytes(KERNEL_BASE as *mut u8, 0, (RAM_BYTES - KERNEL_BASE) as usize) };
        self.boot().await
    }

    /// Console output since the last call.
    pub fn console(&self) -> String {
        let out = state_mut(|s| std::mem::take(&mut s.console_out));
        String::from_utf8_lossy(&out).into_owned()
    }

    /// Decode the trace ring (again). The views use the last snapshot.
    pub fn snapshot(&self) {
        // (The last one dropped first: never two at once, the memory's high
        // water a recording's, not two's.)
        drop(std::mem::take(&mut *self.rec.borrow_mut()));
        let (records, ids, args) = trace::ring(&Direct, state(|s| s.ring));
        *self.rec.borrow_mut() = Recording::new(tables(), records, ids, args);
    }

    // ---- The page's program, a line at a time ----

    /// Make line `text` of the program (ksee_core::program::statement),
    /// the next: a variable declared, in its runner's process's own memory
    /// (its name then its address); memory written; or a syscall asked of
    /// its runner (it finishes when it returns, or, when it blocks, later,
    /// as another call wakes it: see quiet). The host's writes are recorded,
    /// as a call's are; a call from its runner's begin to its end
    /// (user/prog.c): the syscall, entry to return, and whatever the kernel
    /// does meanwhile.
    pub fn make(&self, task: Option<&str>, text: &str) -> Result<Made, String> {
        let made = self.lower_and_make(task, text);
        let outcome = match &made {
            Ok(Made::Decl(v)) => {
                let shown = self.show(v.ty, v.addr, v.count);
                state_mut(|s| s.session.vars.push((v.clone(), shown.clone())));
                Outcome::Declared(shown)
            }
            Ok(Made::Call(id)) => Outcome::Call { id: *id, done: false, ret: None, during: None, changed: Vec::new() },
            Err(e) => Outcome::Failed(e.clone()),
        };
        state_mut(|s| s.session.outcomes.push(outcome));
        made
    }

    fn lower_and_make(&self, task: Option<&str>, text: &str) -> Result<Made, String> {
        let line = state(|s| s.session.lines);
        let (tid, plan) = lowered(task, text, line)?;
        let made = match plan {
            Lowered::Decl(d) => {
                let addr = state_mut(|s| s.session.alloc(tid, d.bytes.len() as u32)).map_err(|e| format!("{}: {e}", d.name))?;
                write_logged(addr, &d.bytes);
                state_mut(|s| s.session.names.set(&d.name, line, Sym::Var { addr, ty: d.ty, count: d.count }));
                Made::Decl(ProgramVar { name: d.name, addr, ty: d.ty, count: d.count, bytes: d.bytes.len() as u32 })
            }
            // (What it names: the variables it may change, see quiet.)
            Lowered::Call { nr, args, result, said } => Made::Call(call(tid, nr, &args, line, result, said)?),
        };
        state_mut(|s| s.session.lines = line + 1);
        Ok(made)
    }

    /// How each line made since the boot went.
    pub fn outcomes(&self) -> Vec<Outcome> {
        state(|s| s.session.outcomes.clone())
    }

    /// Whether line `text` can be made as line `line` (with the names of
    /// the lines before it): why not, if not; nothing made.
    pub fn can_make(&self, task: Option<&str>, text: &str, line: usize) -> Result<(), String> {
        lowered(task, text, line).map(|_| ())
    }

    /// Active secondary task names visible after the rows before `line`, in
    /// source-row order. The main task is represented by `None` at the API
    /// boundary and therefore is not included.
    pub fn task_columns(&self, line: usize) -> Vec<String> {
        state(|s| s.session.task_columns(line))
    }

    /// What fits at the cursor `at` of `text`, line `line` of the program
    /// (ksee_core::program::completions; `common`: the syscalls first).
    pub fn complete(&self, text: &str, at: usize, line: usize, common: &[String]) -> Vec<ksee_core::program::Completion> {
        let c = ksee_core::program::context(text, at);
        state(|s| ksee_core::program::completions(&tables().types, &c, &s.session.names.before(line), common))
    }

    /// The kernel quiet (now, or once it is): the calls finished meanwhile
    /// done in the outcomes, and the variables the last line's call changed,
    /// by now (one passed as records of a variable length, as its
    /// parameter's type reads them: a directory's entries); an error if it
    /// started again meanwhile.
    pub async fn quiet(&self) -> Result<(), JsValue> {
        JsFuture::from(Promise::new(&mut |ok, fail| {
            state_mut(|s| s.quiet_waiters.push((ok, fail)));
            if sched::quiet() {
                settle(None);
            }
        })).await?;
        let mut p = state_mut(|s| std::mem::take(&mut s.session));
        p.changed(|v| self.show(v.ty, v.addr, v.count), |call, k, v, len| self.as_records(call, k, v.addr, len));
        state_mut(|s| s.session = p);
        Ok(())
    }

    /// A program's variable (of type `ty`, an array of `count` if given, at
    /// `addr`: see make) as it is now, as C would write it: `"hello"`,
    /// `{3, 4}`, `{tv_sec = 0, tv_nsec = 30000000}`.
    pub fn show(&self, ty: u32, addr: u32, count: Option<u32>) -> String {
        tables().types.show(&Direct, addr, ty, count)
    }

    /// The `len` bytes at `addr`, passed as argument `k` of syscall `call`,
    /// as the records its parameter's type says they are, if they are of
    /// variable length (Types::records: getdents64's entries).
    pub(crate) fn as_records(&self, call: &str, k: usize, addr: u32, len: u32) -> Option<String> {
        use ksee_core::types::Kind;
        let types = &tables().types;
        let &(_, ty) = types.syscall_params(call)?.get(k)?;
        let Kind::Pointer(to) = types.get(types.resolve(ty)).kind else { return None };
        types.records(&Direct, addr, len, to)
    }
}

/// Write `bytes` at `addr` into kernel memory, as the host: recorded (as a
/// call's writes into its request are), so memory as it was stays exact.
pub(crate) fn write_logged(addr: u32, bytes: &[u8]) {
    // (Recorded from the first write of the host's on, a variable declared
    // before any call: memory as it was exact from there.)
    crate::programs::record();
    imports::logged(addr, bytes.len() as u32);
    unsafe { core::ptr::copy_nonoverlapping(bytes.as_ptr(), addr as *mut u8, bytes.len()) };
}

/// What a line made (Machine::make): a variable, or a call's id.
pub enum Made {
    Decl(ProgramVar),
    Call(u32),
}

/// Have runner `tid` make syscall `nr` with `args`, for line `line` (its
/// result named `result`): the call's id.
fn call(tid: u32, nr: u32, args: &[Arg], line: usize, result: Option<(String, Option<u32>)>, said: (String, Vec<Option<String>>)) -> Result<u32, String> {
    let (id, Runner { task, .. }) = state(|s| s.session.next_call(tid))?;
    // (A call's words as the buffers they point to are put in its program's
    // memory, recorded, as its variables are.)
    let mut put = |b: &[u8]| -> Result<u32, String> {
        let at = state_mut(|s| s.session.alloc(tid, b.len() as u32))?;
        write_logged(at, b);
        Ok(at)
    };
    let words = core::iter::once(nr).chain(ksee_core::program::words(args, &mut put)?).collect();
    let c = Request { id, tid, nr, task, line, result, said, words, ..Default::default() };
    state_mut(|s| {
        s.session.ask(c);
        // (The page waits for the kernel now: see State::waiting.)
        s.waiting = Some(s.clock);
    });
    // (The runner reads it from the console: see imports.rs's console_read.)
    crate::programs::record();
    sched::interrupt(IRQ_CONSOLE);
    Ok(id)
}

/// The page's program's names, as line `line` sees them (see
/// ksee_core::program::Names), and the kernel's constants; memory as it is
/// now.
struct Seen(usize);

impl ksee_core::program::Env for Seen {
    fn sym(&self, n: &str) -> Option<Sym> {
        if let Some(&v) = tables().types.constants.get(n) {
            return Some(Sym::Value { v, to: None });
        }
        state(|s| s.session.names.get(n, self.0).cloned())
    }

    fn memory(&self) -> Option<&dyn Memory> {
        Some(&Direct)
    }
}

/// The kernel is quiet (or, `err`, gone): settle what waits for it (see
/// Machine::quiet).
pub(crate) fn settle(err: Option<String>) {
    for (ok, fail) in state_mut(|s| std::mem::take(&mut s.quiet_waiters)) {
        let _ = match &err {
            None => ok.call0(&JsValue::NULL),
            Some(e) => fail.call1(&JsValue::NULL, &e.into()),
        };
    }
}

/// Line `text`, as line `line` of task `task` (none: the program's own):
/// its runner's tid, and what it lowers to (with the names of the lines
/// before it).
fn lowered(task: Option<&str>, text: &str, line: usize) -> Result<(u32, Lowered), String> {
    let tid = state(|s| s.session.task_tid(task, line))?;
    Ok((tid, lower(text, &tables().types, &Seen(line))?))
}
