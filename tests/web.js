// Boot the kernel under Deno with the page's own module (web/ksee.js), have
// the program make syscalls (as the page's examples do), recorded, and check what
// the recordings show.
//   deno run --allow-read tests/web.js
import { start } from "../target/site/ksee.js";
import { kernel, runner } from "../target/site/kernel.js";

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
// The syscalls, from JavaScript (web/kernel.js), as the page makes them.
const kern = kernel(m);
// Calls finished (Machine.quiet's news), by id.
const fin = new Map();
// Ask runner `tid` to make syscall `name`, recorded, and wait until the
// kernel is quiet: the call's id (it may not have finished: blocked).
async function ask(name, args, tid) {
  const made = kern.call(name, args, tid);
  for (const f of (await made.quiet).finished) fin.set(f.id, f);
  return made.id;
}
// Its syscall summary.
const syscallOf = (id) => m.syscalls()[m.syscall_of(id)];
// Syscall `name`, made by runner `tid`: { ret, buffers, how, s (its syscall summary) }.
async function call(name, args, tid) {
  const id = await ask(name, args, tid);
  return { ...fin.get(id), s: syscallOf(id) };
}
const ran = (s, func) => s?.last != null && m.first_call(func, s.first, s.last, s.task.addr) != null;

check(`the program ready: a runner (${m.main()}), not /init`, m.main() > 1 && m.runners().includes(m.main()));
check("nothing recorded at first", (m.snapshot(), m.records() === 0));

// Calls, traced.
const w = (await call("write", [1, "hello kSEE\n", 11])).s;
check(`a write, by the program (${w?.task.comm}, pid ${w?.task.pid}), shown on the console`, w?.ret === 11 && w.task.comm === "prog" && w.task.pid === m.main() && await until(() => out.includes("hello kSEE")));
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
check("the task, as it was", task?.children?.find((c) => c.name === "comm")?.value === '"prog"');
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
const got = m.declare(undefined, "char got[64]", {});
const { s: read } = await call("read", [rfd, got.addr, 64]);
check("read 26 bytes back", read?.ret === 26 && m.show(got.ty, got.addr, 64).startsWith('"hello from the page cache'));
const inRead = m.line_steps(read.first, read.last, read.task.addr).find((x) => x.func === "vfs_read").i;
{
  // (Its last line, the closing brace: `return ret;` compiles to no line of its own.)
  const last = m.line_steps(read.first, read.last, read.task.addr).findLast((x) => x.func === "vfs_read");
  check(`vfs_read's last line (${last?.line}) says what it returns: ${last?.ret}`, last?.ret === 26);
}
const file = m.call_stack(inRead)[0].args.find((a) => a.name === "file");
const fpos = (i) => (m.memory_at(i), m.object_by_id("file", file.target, Number(file.value), 1)?.children?.find((c) => c.name === "f_pos"));
check("f_pos before the read, and after", fpos(read.first)?.value === "0" && fpos(read.last)?.value === "26");
// (The program's buffer is kernel memory: the read fills it, recorded.)
const buf = (i) => (m.memory_at(i), m.call_stack(inRead)[0].args.find((a) => a.name === "buf")?.value);
check("the user's buffer, before the read and after", !/hello/.test(buf(read.first)) && /"hello from the page cache/.test(buf(read.last)));
const who = (m.memory_at(read.last), m.origin(fpos(read.last).addr, 8)[0]);
check("who wrote f_pos: ksys_read", who?.at?.func === "ksys_read");
// What the read did (Objects): its file's f_pos, 0 to 26, by ksys_read.
{
  const obj = m.objects(read.first, read.last).find((n) => n.addr === Number(file.value) && n.id === file.target);
  const d = obj?.changes.find((x) => x.member === "f_pos");
  check(`the read wrote its file: ${obj?.changes.map((x) => x.member).join(", ")}; f_pos ${d?.before} → ${d?.after} by ${d?.runs.map((r) => r.at?.func).join(", ")}`,
        d?.before === "0" && d?.after === "26" && d.runs.length === 1 && d.runs[0].at?.func === "ksys_read");
  // The read's stores, named as the code names what they hit: a call's file, a local of a call in it (at its frame base's offset), a callee's frame; in order.
  const st = m.stores(read.first, read.last);
  check(`the read's stores: ${st.length}, ${[...new Set(st.map((s) => s.what))].slice(0, 5).join("; ")}…`,
        st.some((s) => /^\w+'s file$/.test(s.what) && s.kind === "object" && s.write.member === "f_pos") && st.some((s) => /'s (kiocb|iter)$/.test(s.what))
        && st.some((s) => s.what.endsWith("'s frame")) && st.every((s, k) => k === 0 || st[k - 1].write.i < s.write.i));
}
m.memory_at(null);
// What the read did: its file changed; not its kiocb (on the stack, gone
// when the read ends).
{
  const g = m.objects(read.first, read.last);
  check(`what the read changed: ${g.map((n) => n.what).join("; ")}`, g.some((n) => n.ty === "struct file") && !g.some((n) => n.ty === "struct kiocb"));
}
// Where the read's bytes came from: copied from the page cache, which the
// write before it filled, copying from its buffer.
{
  const buf = parseInt(m.call_stack(inRead)[0].args.find((a) => a.name === "buf").value, 16);
  m.memory_at(read.last);
  const way = m.origin(buf, 4);
  m.memory_at(null);
  check(`the read's bytes, back through their copies: ${way.map((w) => `${w.at?.func}(0x${w.addr.toString(16)})`).join(" ← ")}`,
        way.length >= 2 && way[0].from != null && way[1].addr === way[0].from);
}
// A program's own variables (Machine.declare): memory in the program's process,
// passed by address, read back as C writes them.
{
  const buf = m.declare(undefined, "char buf[16]", {}), fds = m.declare(undefined, "int fds[2]", {});
  await call("pipe2", [fds.addr, 0]);
  const [r, w] = m.show(fds.ty, fds.addr, 2).slice(1, -1).split(", ").map(Number);
  await call("write", [w, "hi", 2]);
  const n = (await call("read", [r, buf.addr, 16])).ret;
  check(`variables: fds = ${m.show(fds.ty, fds.addr, 2)}, buf = ${m.show(buf.ty, buf.addr, 16)}`, n === 2 && m.show(buf.ty, buf.addr, 16) === '"hi"');
}
// Syscalls from JavaScript (kernel.js): JS values as C's (a struct as an
// object, a variable as its address); a blocked read goes on as a later
// write wakes it.
{
  const buf = kern.declare("char buf[8]"), fds = kern.declare("int fds[2]");
  const slept = await kern.sys.clock_nanosleep_time64(kern.C.CLOCK_MONOTONIC, 0, { tv_sec: 0, tv_nsec: 1000000 }, null);
  await kern.sys.pipe2(fds, 0);
  const [r, w] = fds.get();
  // (A thread, a runner of its own, reads: it blocks, nothing written yet;
  // /init writes, and wakes it.)
  const C = kern.C, mem = await kern.sys.mmap2(null, 65536, C.PROT_READ | C.PROT_WRITE, C.MAP_PRIVATE | C.MAP_ANONYMOUS, -1, 0);
  const thread = await kern.sys.clone(C.CLONE_VM | C.CLONE_FS | C.CLONE_FILES | C.CLONE_SIGHAND | C.CLONE_THREAD | C.CLONE_SYSVSEM, mem + 65536, null, null, 0);
  const reading = kern.sys.on(thread).read(r, buf, 8);
  const wrote = await kern.sys.write(w, "ok", 2);
  check(`syscalls from JS: slept ${slept}, wrote ${wrote}, read ${await reading}: buf = ${buf.show()}`, slept === 0 && wrote === 2 && buf.get() === "ok");
  // (Its variables as JS values, both ways.)
  const ts = kern.declare("struct __kernel_timespec ts");
  ts.set({ tv_sec: 1, tv_nsec: 5 });
  check(`a variable as a JS value, and set from one: ${JSON.stringify(ts.get())}`, ts.get().tv_sec === 1 && ts.get().tv_nsec === 5);
}
// A 64-bit argument, whole: llseek's offset (high 1, low 0) is vfs_llseek's loff_t 1 << 32.
{
  const res = m.declare(undefined, "long long res", {});
  const ls = (await call("llseek", [rfd, 1, 0, res.addr, 0])).s;
  const at = ls && m.first_call("vfs_llseek", ls.first, ls.last, ls.task.addr);
  const off = at != null && m.call_stack(at).find((f) => f.func === "vfs_llseek")?.args?.find((a) => a.name === "offset")?.value;
  check(`a 64-bit argument whole: vfs_llseek's offset ${off}`, off === "4294967296");
  // (And a 64-bit result: what vfs_llseek returned, the position.)
  const got = ls && m.line_steps(ls.first, ls.last, ls.task.addr).find((s) => s.called?.[0] === "vfs_llseek")?.called[1];
  check(`a 64-bit result whole: vfs_llseek returned ${got}`, got === 4294967296);
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
// A variable the code keeps in a wasm local: in the Stack as it is by the
// cursor (ksys_read's ret, set to what vfs_read returned).
{
  const last = readSteps.findLast((x) => x.func === "ksys_read").i;
  const ret = m.call_stack(last).find((c) => c.func === "ksys_read")?.locals?.find((l) => l.name === "ret");
  check(`a local in a wasm local: ksys_read's ret = ${ret?.value}`, ret?.value === "26");
}
m.memory_at(null);
await call("close", [rfd]);
// (A user pointer must be in RAM: not the runtime's memory, below it.)
check("a bad user pointer: EFAULT", (await call("write", [1, 0x1000, 4])).ret === -14 && (await call("getpid", [])).ret === m.main());

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
  // (Memory reached through a pointer local, or the task's pointer members, is named too: files->next_fd, through the task's files.)
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
      && a("read", 0, "$2", { $2: 7 }) === 7 && a("read", 0, "fd + 1", { fd: 3 }) === 4
      && new TextDecoder().decode(a("openat", 1, '"a\\n"').bytes) === "a\n\0");
check("an array of strings as in C (argv)", JSON.stringify(a("execve", 1, '{"init", "-x", NULL}')) === '{"strings":["init","-x",null]}');
// A struct, laid out as the parameter's type says (a __kernel_timespec: two s64).
const spec = (text) => [...new BigInt64Array(a("clock_nanosleep_time64", 2, text).bytes.buffer)].join(",");
check("a struct as in C, positional or by name", spec("{0, 30000000}") === "0,30000000" && spec("{.tv_nsec = 30000000}") === "0,30000000");
check("a wrong member is an error", (() => { try { a("clock_nanosleep_time64", 2, "{.tv_min = 1}"); return false; } catch (e) { return /no member/.test(e); } })());
check("a runner by a result: value()", m.value("$2 + 0", { $2: 19 }) === 19);
const examples = JSON.parse(await Deno.readTextFile(new URL("target/site/examples.json", root)));
// (An example's program: its lines, as written, parsed as the page parses them.)
for (const ex of examples) ex.steps = ex.steps.map((line) => m.statement(line));
// (Each on a fresh kernel, as the page runs them; a call may block, and
// finish as a later one wakes it.)
// (A step's runner, `tid`: a thread id or a result's name; else the program.)
async function runExample(ex) {
  await m.restart();
  const p = runner(kern, m, ex.steps.map((s) => ({ ...s })));
  while (await p.next());
  // (Each call's id, as asked; null for a declaration; how each finished.)
  p.done.forEach((d) => d.id != null && d.how != null && fin.set(d.id, d));
  return p.done.map((d) => d.id ?? null);
}
for (const ex of examples) {
  const asked = await runExample(ex), missing = [], blocked = [];
  ex.steps.forEach((st, j) => {
    if (!st.decl && !fin.get(asked[j])) blocked.push(`${j + 1}. ${st.call}`);
  });
  // ([label](2:func:any): see examples.js; call 2, declarations not counted.)
  const calls = asked.filter((_, j) => !ex.steps[j].decl);
  for (const [, n, func, any] of (ex.text ?? []).join(" ").matchAll(/\]\((\d+)(?::([\w.]+))?(:any)?\)/g)) {
    const s = syscallOf(calls[Number(n) - 1]);
    if (!s || (func && m.first_call(func, s.first, s.last ?? m.records() - 1, any ? undefined : s.task.addr) == null)) missing.push(`${n}:${func ?? ""}`);
  }
  // (Its tries: each a program that runs; its calls finish, or block on purpose.)
  for (const [k, prog] of (ex.tries ?? []).entries()) {
    const steps = prog.split(";").map((c) => c.trim()).filter(Boolean).map((c) => m.statement(c));
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
// (Restarted while a call is under way, its quiet not come: the calls after it still finish.)
const cut = kern.call("getpid", []);
cut.quiet.catch(() => {});
await m.restart();
const after = await Promise.race([kern.call("getpid", []), new Promise((ok) => setTimeout(() => ok(null), 5000))]);
check(`a call after a restart cut one short finishes (${after?.ret})`, after?.ret === m.main());
check("restarted: the program ready again", m.runners().includes(m.main()) && (await call("getpid", [])).ret === m.main());
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

// An execve with its argv (a vfork child runs the program again): it
// finishes as an exec, and the kernel has no NULL argv to warn of.
{
  const steps = ['pid_t child = clone(CLONE_VM | CLONE_VFORK | SIGCHLD, 0, NULL, NULL, 0)', 'child: execve("/bin/prog", {"prog", "-x", NULL}, NULL)'].map((l) => m.statement(l));
  const ran = await runExample({ steps });
  check(`an execve with argv runs the program (${fin.get(ran[1])?.how})`, fin.get(ran[1])?.how === "exec" && !out.includes("NULL argv"));
}

// LINUX_REBOOT_CMD_POWER_OFF (no result: the machine halts)
await m.restart();
m.call("reboot", [0xfee1dead, 672274793, 0x4321fedc, 0]);
check("powered off", (await until(() => m.halted() !== undefined && m.halted() + 1)) === 1);

check("no warning from the kernel (an execve has its argv)", !out.includes("NULL argv"));
if (failed) console.log(out);
console.log(failed ? "FAIL" : "PASS");
Deno.exit(failed ? 1 : 0);
