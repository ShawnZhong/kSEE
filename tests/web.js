// Boot the kernel under Deno with the page's own module (web/ksee.js), have
// /init make syscalls (as the page's examples do), recorded, and check what
// the recordings show.
//   deno run --allow-read tests/web.js
import { start } from "../target/site/ksee.js";
import { parseCall } from "../target/site/args.js";

const root = new URL("..", import.meta.url);
const m = await start({
  kernel: new URL("target/site/vmlinux.gz", root),
  initrd: new URL("target/site/initramfs.cpio.gz", root),
});

let out = "", failed = false;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const check = (what, ok) => {
  console.log(`${ok ? "ok" : "FAIL"}: ${what}`);
  failed ||= !ok;
  return ok;
};
async function until(pred, ms = 10000) {
  for (const t0 = Date.now(); Date.now() - t0 < ms; await sleep(10)) {
    out += m.console().replaceAll("\r", "");
    const v = pred();
    if (v) return v;
  }
}
// Calls finished (Machine.quiet's news), by id.
const fin = new Map();
// Ask runner `tid` to make syscall `name`, recorded, and wait until the
// kernel is quiet: the call's id (it may not have finished: blocked).
async function ask(name, args, tid = 1) {
  const id = m.call(name, args, tid);
  for (const f of (await m.quiet()).finished) fin.set(f.id, f);
  m.snapshot();
  return id;
}
// Its syscall summary.
const syscallOf = (id) => m.syscalls()[m.syscall_of(id)];
// Syscall `name`, made by runner `tid`: { ret, buffers, how, s (its syscall summary) }.
async function call(name, args, tid = 1) {
  const id = await ask(name, args, tid);
  return { ...fin.get(id), s: syscallOf(id) };
}
const ran = (s, func) => s?.last != null && m.first_call(func, s.first, s.last, s.task.addr) != null;

check("init ready: a runner", m.runners().includes(1));
check("nothing recorded at first", (m.snapshot(), m.records() === 0));

// Calls, traced.
const w = (await call("write", [1, "hello kSEE\n", 11])).s;
check("a write, on /init (pid 1), shown on the console", w?.ret === 11 && w.task.comm === "init" && await until(() => out.includes("hello kSEE")));
check("write ran vfs_write", ran(w, "vfs_write"));
check("getpid ran __task_pid_nr_ns", ran((await call("getpid", [])).s, "__task_pid_nr_ns"));
check("mkdirat ran vfs_mkdir", ran((await call("mkdirat", [-100, "/tmp", 0o755])).s, "vfs_mkdir"));
// (The tasks of a stack chart's columns: see Machine.chart.)
const chartTasks = (flat) => {
  const out = [];
  for (let j = 0; j < flat.length; j += 2 + 3 * flat[j + 1]) out.push(flat[j]);
  return out;
};
const ts = { bytes: new Uint8Array(new BigInt64Array([0n, 30000000n]).buffer) };
const nap = (await call("clock_nanosleep_time64", [1, 0, ts, 0])).s;
check("sleeping switched to the idle task and back", ran(nap, "__schedule")
      && chartTasks(m.chart(nap.first, nap.last, 50)).some((a) => m.task(a).comm === "swapper/0"));
// (A long sleep finishes too: time goes on while the call does, its runner waking.)
const long = await call("clock_nanosleep_time64", [1, 0, { bytes: new Uint8Array(new BigInt64Array([2n, 0n]).buffer) }, 0]);
check("a 2 s sleep returns (the clock skipped ahead)", long.how === "returned" && long.ret === 0);
// (And a timed wait: select's timeout.)
const tv = { bytes: new Uint8Array(new BigInt64Array([1n, 0n]).buffer) };
const sel = await call("pselect6_time64", [0, 0, 0, 0, tv, 0]);
check("a 1 s select() times out", sel.how === "returned" && sel.ret === 0);
m.snapshot();
const wi = m.syscalls().findIndex((s) => s.first === w.first);
const [vfs] = m.functions("vfs_write");
check("vfs_write: where it is", vfs?.file === "fs/read_write.c" && vfs.last > vfs.line);
{
  const mk = m.syscalls().find((s) => s.name === "mkdirat"), cs = mk ? m.stores(mk.first, mk.last) : [];
  check(`mkdirat's stores name the kernel's globals: ${[...new Set(cs.filter((c) => c.kind === "global").map((c) => c.what))].join(", ")}; the machine's apart`,
        cs.some((c) => c.kind === "global" && c.what === "nr_inodes") && cs.some((c) => c.kind === "machine" && c.what === "wasm_irq_flags"));
}
check("fs/read_write.c coverage", m.file_coverage("fs/read_write.c", wi).some((l) => l.count > 0));
const chart = m.chart(w.first, w.last, 200);
check("a stack chart of write's records", chart.length > 600 && chart[0] === w.task.addr);
const steps = m.line_steps(w.first, w.last, w.task.addr);
check("write, line by line (inlined lines too)", steps.length > 50 && steps.some((x) => x.inlined));
const k = steps.findIndex((x) => x.inlined);
const inl = m.call_stack(steps[k].i, steps[k].nth);
check(`inlined code is a frame of its own (${steps[k].inlined} in ${steps[k].func})`,
      inl[0]?.inlined && inl[0].func === steps[k].inlined && inl[0].line === steps[k].line && inl.some((f) => f.func === steps[k].func && !f.inlined));
let inlinedRows = 0;
for (let j = 0; j < chart.length; j += 2 + 3 * chart[j + 1]) for (let d = 0; d < chart[j + 1]; d++) if (chart[j + 4 + 3 * d]) inlinedRows++;
check("and a row of its own in the chart", inlinedRows > 0);
const mid = steps.find((x) => x.func === "vfs_write").i;
const stack = m.call_stack(mid);
const kw = stack.find((f) => f.func === "ksys_write"), kwDef = m.functions("ksys_write")[0];
check("an outer call is where it calls the next one in", kw?.file === "fs/read_write.c" && kw.line > kwDef.line);
check("a past call stack, with arguments", stack[0]?.func === "vfs_write" && kw?.args?.find((a) => a.name === "count")?.value === "11");
check("memory as it was: writes undone", m.memory_at(w.first) > m.memory_at(w.last));
m.memory_at(inWrite(w));
const task = m.object("task_struct", w.task.addr, 1);
check("the task, as it was", task?.children?.find((c) => c.name === "comm")?.value === '"init"');
check("a global", m.global("runqueues", 1)?.children?.some((c) => c.name === "nr_running"));
m.memory_at(null);
function inWrite(s) {
  return m.first_call("vfs_write", s.first, s.last, s.task.addr);
}

// A read: f_pos over time, who wrote it, a local.
const fd = (await call("openat", [-100, "/tmp/note", 0o1101, 0o644])).ret; // O_CREAT | O_TRUNC | O_WRONLY
await call("write", [fd, "hello from the page cache\n", 26]);
await call("close", [fd]);
const rfd = (await call("openat", [-100, "/tmp/note", 0, 0])).ret;
const { s: read, buffers } = await call("read", [rfd, { size: 64 }, 64]);
check("read 26 bytes back", read?.ret === 26 && new TextDecoder().decode(buffers[1]).startsWith("hello from the page cache"));
const inRead = m.line_steps(read.first, read.last, read.task.addr).find((x) => x.func === "vfs_read").i;
{
  // (Its last line, the closing brace: `return ret;` compiles to no line of its own.)
  const last = m.line_steps(read.first, read.last, read.task.addr).findLast((x) => x.func === "vfs_read");
  check(`vfs_read's last line (${last?.line}) says what it returns: ${last?.ret}`, last?.ret === 26);
}
const file = m.call_stack(inRead)[0].args.find((a) => a.name === "file");
const fpos = (i) => (m.memory_at(i), m.object_by_id("file", file.target, Number(file.value), 1)?.children?.find((c) => c.name === "f_pos"));
check("f_pos before the read, and after", fpos(read.first)?.value === "0" && fpos(read.last)?.value === "26");
// (/init's buffer is kernel memory: the read fills it, recorded.)
const buf = (i) => (m.memory_at(i), m.call_stack(inRead)[0].args.find((a) => a.name === "buf")?.value);
check("the user's buffer, before the read and after", !/hello/.test(buf(read.first)) && /"hello from the page cache/.test(buf(read.last)));
const who = (m.memory_at(read.last), m.last_write(fpos(read.last).addr, 8));
check("who wrote f_pos: ksys_read", who?.at?.func === "ksys_read");
// An object's history: f_pos's writes, the read's among them (0 to 26, by ksys_read);
// the struct file's writes name their member.
{
  const p = fpos(read.last), h = m.writes(p.addr, p.id), w = h.find((x) => x.i >= read.first && x.i <= read.last);
  check("... and within the read alone, its write only", m.writes(p.addr, p.id, read.first, read.last).length === 1);
  check(`f_pos's history: ${h.length} writes, the read's ${w?.before} → ${w?.after} by ${w?.at?.func}`,
        w?.before === "0" && w?.after === "26" && w?.at?.func === "ksys_read" && h.every((x, k) => k === 0 || h[k - 1].i < x.i));
  const f = m.writes(Number(file.value), file.target, read.first, read.last);
  check(`the struct file's writes in the read: ${f.map((x) => x.member).join(", ")}`, f.some((x) => x.member === "f_pos"));
  // The read's stores, named as the code names what they hit: a call's file, a local of a call in it (at its frame base's offset), a callee's frame; in order.
  const st = m.stores(read.first, read.last);
  check(`the read's stores: ${st.length}, ${[...new Set(st.map((s) => s.what))].slice(0, 5).join("; ")}…`,
        st.some((s) => /^\w+'s file$/.test(s.what) && s.kind === "object" && s.write.member === "f_pos") && st.some((s) => /'s (kiocb|iter)$/.test(s.what))
        && st.some((s) => s.what.endsWith("'s frame")) && st.every((s, k) => k === 0 || st[k - 1].write.i < s.write.i));
}
m.memory_at(null);
// The objects the read wrote, as a graph: its kiocb (on the stack, gone
// when the read ends) points to its file (ki_filp).
{
  const g = m.objects(read.first, read.last, read.last), iocb = g.find((n) => n.ty === "struct kiocb");
  const filp = iocb?.links.find((l) => l.member === "ki_filp");
  check(`the read's objects: ${g.length}, its kiocb's ki_filp → ${filp && g[filp.to].what}`, filp && g[filp.to].ty === "struct file" && iocb.ended != null);
}
// A 64-bit argument, whole: llseek's offset (high 1, low 0) is vfs_llseek's loff_t 1 << 32.
{
  const ls = (await call("llseek", [rfd, 1, 0, { size: 8 }, 0])).s;
  const at = ls && m.first_call("vfs_llseek", ls.first, ls.last, ls.task.addr);
  const off = at != null && m.call_stack(at).find((f) => f.func === "vfs_llseek")?.args?.find((a) => a.name === "offset")?.value;
  check(`a 64-bit argument whole: vfs_llseek's offset ${off}`, off === "4294967296");
}
const readSteps = m.line_steps(read.first, read.last, read.task.addr);
check("line steps sit at their first store: some line is at a record past its block's, none before it, and a store is on its line",
      readSteps.some((s) => s.at > s.i) && readSteps.every((s) => s.at >= s.i) && readSteps.some((s) => s.at > s.i && m.at(s.at)?.line === s.line));
// Run to a line: the next time it runs, and back.
{
  const s5 = readSteps.find((x) => x.func === "ksys_read" && m.at(x.i)?.kind === "block");
  const next = m.run_of(s5.file, s5.line, s5.i - 1, 1), back = m.run_of(s5.file, s5.line, s5.i + 1, -1);
  check(`run to ${s5.file}:${s5.line}, forward and back: its run`, next === s5.i && back === s5.i);
}
const pos = (i) => (m.memory_at(i), m.call_stack(i).find((c) => c.func === "ksys_read")?.locals?.find((l) => l.name === "pos")?.value);
check("a local on the stack, as it was: ksys_read's pos, 0 then 26",
      pos(inRead) === "0" && pos(readSteps.findLast((x) => x.func === "ksys_read").i) === "26");
m.memory_at(null);
await call("close", [rfd]);
// (A user pointer must be in RAM: not the runtime's memory, below it.)
check("a bad user pointer: EFAULT", (await call("write", [1, 0x1000, 4])).ret === -14 && (await call("getpid", [])).ret === 1);

// A char * shows its string: mkdir's struct filename, its name.
{
  const mk = (await call("mkdirat", [-100, "/mnt/named", 0o755])).s, i = m.first_call("filename_create", mk.first, mk.last);
  const arg = m.call_stack(i).find((f) => f.func === "filename_create")?.args?.find((a) => a.ty === "struct filename *");
  m.memory_at(i); // (as it was: the name is freed after the syscall)
  // (7.2's struct filename: the name pointer is in an anonymous struct, whose members are its own.)
  const name = arg && m.object_by_id("name", arg.target, Number(arg.value.split(" ")[0]), 1)?.children?.find((c) => c.name === "name");
  m.memory_at(null);
  check(`a char * with its string: filename->name = ${name?.value}`, name?.value?.endsWith('"/mnt/named"'));
  await call("unlinkat", [-100, "/mnt/named", 0x200]);
}
check("definitions: a function, a type, a global", m.definition("vfs_write")?.file === "fs/read_write.c"
      && m.definition("task_struct")?.file === "include/linux/sched.h" && m.definition("runqueues")?.line > 0 && m.definition("no_such") == null
      && m.definition("list_add")?.file === "include/linux/list.h" && !m.functions("list_add").length);
check("a field's declaration: file.f_pos, or .f_pos of any struct", m.definition("file.f_pos")?.file === "include/linux/fs.h" && m.definition(".f_pos")?.line > 0);
check("a syscall's parameters, typed, with the constants that fit", JSON.stringify(m.syscall("openat")?.params) ===
      JSON.stringify([{ name: "dfd", ty: "int", consts: ["AT_"] }, { name: "filename", ty: "char *", consts: [] },
                      { name: "flags", ty: "int", consts: ["O_"] }, { name: "mode", ty: "umode_t", consts: [] }]));
// The syscall entry: the frame says which syscall, its arguments typed as the syscall's, as constants; the wrapper is a frame of its own, with the same parameters (its SYSCALL_DEFINE line's).
{
  const op = m.syscalls().find((s) => s.name === "openat"), stack = m.call_stack(m.first_call("do_sys_openat2", op.first, op.last)), entry = stack.at(-1);
  // (Memory reached through a pointer local, or the task's pointer members, is named too: files->next_fd, through current->files.)
  const ops = m.stores(op.first, op.last);
  check(`a store through the task's files is named: ${[...new Set(ops.filter((s) => s.write.member === "next_fd").map((s) => s.what))].join(", ")}`,
        ops.some((s) => s.write.member === "next_fd" && s.kind === "object"));
  // (A call begins at its function's signature line: the callee's frame there, before its first statement.)
  const dso = m.at(m.first_call("do_sys_openat2", op.first, op.last)), dsoDef = m.definition("do_sys_openat2");
  check(`a call begins at its signature: do_sys_openat2 at ${dso?.file}:${dso?.line}, defined at ${dsoDef?.line}`, dso?.line === dsoDef?.line && dso?.file === dsoDef?.file);
  check(`the entry frame: ${entry?.func}, the syscall ${entry?.syscall}(${entry?.args?.map((a) => `${a.name} = ${a.value}`).join(", ")})`,
        entry?.func === "wasm_syscall" && entry.syscall === "openat" && entry.args?.[0]?.value === "AT_FDCWD" && entry.args[2]?.value === "O_WRONLY | O_CREAT | O_TRUNC"
        && stack.some((f) => f.func === "__wasm_sys_openat" && !f.inlined && f.args?.[0]?.name === "dfd" && f.args[0].value === "AT_FDCWD"));
}

// The examples (target/site/examples.json): their calls, and each of the explanation's links found in its call's recording.
const consts = m.constants();
check("the kernel's constants: O_CREAT, AT_FDCWD, AT_REMOVEDIR, ENOTEMPTY",
      consts.O_CREAT === 64 && consts.AT_FDCWD === -100 && consts.AT_REMOVEDIR === 0x200 && consts.ENOTEMPTY === 39);
const a = (call, k, text, known = {}) => m.arg(call, k, text, known);
check("arguments as in C", a("openat", 2, "O_CREAT | O_WRONLY | O_TRUNC") === 577 && a("openat", 3, "0644") === 420
      && a("read", 0, "$2", { $2: 7 }) === 7 && a("read", 0, "fd + 1", { fd: 3 }) === 4 && a("read", 1, "[64]").size === 64
      && new TextDecoder().decode(a("openat", 1, '"a\\n"').bytes) === "a\n\0");
check("an array of strings as in C (argv)", JSON.stringify(a("execve", 1, '{"init", "-x", NULL}')) === '{"strings":["init","-x",null]}');
// A struct, laid out as the parameter's type says (a __kernel_timespec: two s64).
const spec = (text) => [...new BigInt64Array(a("clock_nanosleep_time64", 2, text).bytes.buffer)].join(",");
check("a struct as in C, positional or by name", spec("{0, 30000000}") === "0,30000000" && spec("{.tv_nsec = 30000000}") === "0,30000000");
check("a wrong member is an error", (() => { try { a("clock_nanosleep_time64", 2, "{.tv_min = 1}"); return false; } catch (e) { return /no member/.test(e); } })());
check("a runner by a result: value()", m.value("$2 + 0", { $2: 19 }) === 19);
const examples = JSON.parse(await Deno.readTextFile(new URL("target/site/examples.json", root)));
// (Each on a fresh kernel, as the page runs them; a call may block, and
// finish as a later one wakes it.)
// (A step's runner, `as`: the one call `as` started; else /init.)
async function runExample(ex) {
  await m.restart();
  const rets = [], asked = [], started = [], seen = new Set(m.runners());
  // (The results so far, by $n and by name, as the page has them; for a
  // runner, a call not returned yet is the runner it started: vfork's child.)
  const known = (runners) => Object.fromEntries(ex.steps.slice(0, asked.length).flatMap((st, i) => {
    const v = rets[i] ?? (runners ? started[i] : null);
    return v == null ? [] : [[`$${i + 1}`, v], ...(st.name ? [[st.name, v]] : [])];
  }));
  for (const st of ex.steps) {
    const args = st.args.map((x, k) => m.arg(st.call, k, x, known(false)));
    const tid = st.tid == null ? (st.as ? started[st.as - 1] : 1) : typeof st.tid === "number" ? st.tid : m.value(String(st.tid), known(true));
    const q = await ask(st.call, args, tid);
    asked.push(q);
    started.push(m.runners().find((t) => !seen.has(t)));
    m.runners().forEach((t) => seen.add(t));
    asked.forEach((a, j) => (rets[j] = fin.get(a)?.ret));
  }
  return asked;
}
for (const ex of examples) {
  const asked = await runExample(ex), missing = [], blocked = [];
  ex.steps.forEach((st, j) => {
    const q = asked[j], s = syscallOf(q), f = fin.get(q);
    if (!f) blocked.push(`${j + 1}. ${st.call}`);
  });
  // ([label](2:func:any): see examples.js.)
  for (const [, n, func, any] of (ex.text ?? []).join(" ").matchAll(/\]\((\d+)(?::([\w.]+))?(:any)?\)/g)) {
    const s = syscallOf(asked[Number(n) - 1]);
    if (!s || (func && m.first_call(func, s.first, s.last ?? m.records() - 1, any ? undefined : s.task.addr) == null)) missing.push(`${n}:${func ?? ""}`);
  }
  // (Its tries: each a program that runs; its calls finish, or block on purpose.)
  for (const [k, prog] of (ex.tries ?? []).entries()) {
    const steps = prog.split(";").map((c) => parseCall(c.trim())).filter(Boolean);
    const ran = await runExample({ ...ex, steps });
    check(`example ${ex.name}, try ${k + 1}: ${steps.length} calls made (${prog.slice(0, 50)}…)`, ran.length === steps.length && steps.length > 0);
  }
  check(`example ${ex.name}: ${ex.steps.length} calls, all finished, the explanation's links found`,
        !missing.length && !blocked.length || console.log("  missing:", missing, "blocked:", blocked));
}

// A fresh kernel, in place: nothing of the calls before (/tmp is gone).
await call("mkdirat", [-100, "/tmp", 0o755]);
check("/tmp is there before", (await call("mkdirat", [-100, "/tmp", 0o755])).ret === -17);
const before = out.length;
await m.restart();
check("restarted: init ready again", m.runners().includes(1) && (await call("getpid", [])).ret === 1);
check("... on a fresh kernel: /tmp is not there", (await call("mkdirat", [-100, "/tmp", 0o755])).ret === 0);

// Deterministic: the same calls on a fresh kernel make the same recording,
// record for record (the kernel's clock is virtual; see the runtime's sched.rs).
async function runOf(ex) {
  const seen = (await runExample(ex)).map((q) => {
    const s = syscallOf(q);
    return [s?.id, s?.first, s?.last, fin.get(q)?.ret];
  });
  return JSON.stringify([m.records(), seen]);
}
for (const name of ["create", "futex"]) {
  const file = examples.find((x) => x.name === name);
  const one = await runOf(file), two = await runOf(file);
  check(`deterministic: the same calls, the same records (${one.slice(0, 60)}…)`, one === two || console.log(one, "\n", two));
}

// LINUX_REBOOT_CMD_POWER_OFF (no result: the machine halts)
await m.restart();
m.call("reboot", [0xfee1dead, 672274793, 0x4321fedc, 0]);
check("powered off", (await until(() => m.halted() !== undefined && m.halted() + 1)) === 1);

check("no warning from the kernel (an execve has its argv)", !out.includes("NULL argv"));
if (failed) console.log(out);
console.log(failed ? "FAIL" : "PASS");
Deno.exit(failed ? 1 : 0);
