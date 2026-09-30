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
// The kernel, as the page makes its program's lines (web/kernel.js).
const kern = kernel(m);
// Calls finished (Machine.quiet's news), by id.
const fin = new Map();
// Its syscall summary.
const syscallOf = (id) => m.syscalls()[m.syscall_of(id)];
// A line of the program (Machine.run), made, and the kernel run until it
// is quiet: a call's { id, ret?, how? (none: it blocked), s (its syscall
// summary) }; a variable's { name, addr, ty, count, size }.
async function line(text) {
  const r = m.run(text);
  if (r.decl || r.assigned != null) return r.decl ?? r;
  for (const f of (await kern.quiet()).finished) fin.set(f.id, f);
  return { id: r.id, ...fin.get(r.id), s: syscallOf(r.id) };
}
const ran = (s, func) => s?.last != null && m.first_call(func, s.first, s.last, s.task.addr) != null;

check(`the program ready: a runner (${m.main()}), not /init`, m.main() > 1 && m.runners().includes(m.main()));
check("nothing recorded at first", (m.snapshot(), m.records() === 0));

// Calls, traced.
const w = (await line('write(1, "hello kSEE\\n", 11)')).s;
check(`a write, by the program (${w?.task.comm}, pid ${w?.task.pid}), shown on the console`, w?.ret === 11 && w.task.comm === "prog" && w.task.pid === m.main() && await until(() => out.includes("hello kSEE")));
check("write ran vfs_write", ran(w, "vfs_write"));
check("getpid ran __task_pid_nr_ns", ran((await line("getpid()")).s, "__task_pid_nr_ns"));
check("mkdirat ran vfs_mkdir", ran((await line('mkdirat(AT_FDCWD, "/tmp", 0755)')).s, "vfs_mkdir"));
// (The tasks of a stack chart's columns: see Machine.chart.)
const chartTasks = (flat) => {
  const out = [];
  for (let j = 0; j < flat.length; j += 3 + 3 * flat[j + 2]) out.push(flat[j]);
  return out;
};
const nap = (await line("clock_nanosleep(CLOCK_MONOTONIC, 0, {0, 30000000}, NULL)")).s;
check("sleeping switched to the idle task and back", ran(nap, "__schedule")
      && chartTasks(m.chart(nap.first, nap.last, 50)).some((a) => m.task(a, nap.first).comm === "swapper/0"));
// (A long sleep finishes too: time goes on while the call does, its runner waking.)
const long = await line("clock_nanosleep(CLOCK_MONOTONIC, 0, {2, 0}, NULL)");
check("a 2 s sleep returns (the clock skipped ahead)", long.how === "returned" && long.ret === 0);
// (And a timed wait: select's timeout.)
const sel = await line("pselect6(0, NULL, NULL, NULL, {1, 0}, NULL)");
check("a 1 s select() times out", sel.how === "returned" && sel.ret === 0);
m.snapshot();
const wi = m.syscalls().findIndex((s) => s.first === w.first);
const [vfs] = m.functions("vfs_write");
check("vfs_write: where it is", vfs?.file === "fs/read_write.c" && vfs.last > vfs.line);
{
  const mk = m.syscalls().find((s) => s.name === "mkdirat"), cs = mk ? m.objects(mk.first, mk.last) : [];
  check(`mkdirat's objects name the kernel's globals: ${cs.filter((c) => c.kind === "global").map((c) => c.what).join(", ")}; not the machine's`,
        cs.some((c) => c.kind === "global" && c.what === "nr_inodes") && !cs.some((c) => c.what === "wasm_irq_flags"));
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
for (let j = 0; j < chart.length; j += 3 + 3 * chart[j + 2]) for (let d = 0; d < chart[j + 2]; d++) if (chart[j + 5 + 3 * d]) inlinedRows++;
check("and a row of its own in the chart", inlinedRows > 0);
// (Each column is a moment, its record: the Stack there has every call
// it draws, the code inlined for a moment too: a click goes there.)
{
  let drawn = 0, missing = [];
  for (let j = 0; j < chart.length; j += 3 + 3 * chart[j + 2]) {
    const frames = m.call_stack(chart[j + 1]);
    for (let d = 0; d < chart[j + 2]; d++) {
      const [id, first, inl] = chart.slice(j + 3 + 3 * d, j + 6 + 3 * d), name = inl ? m.inline_name(inl - 1) : m.function(id)?.name;
      drawn++;
      if (!frames.some((f) => f.first === first && f.func === name && !!f.inlined === !!inl)) missing.push(name);
    }
  }
  check(`a chart's column is its record's stack: ${drawn} calls drawn, ${missing.length} not there (${missing.slice(0, 3)})`, drawn > 0 && !missing.length);
}
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
await line('int fd = openat(AT_FDCWD, "/tmp/note", O_CREAT | O_TRUNC | O_WRONLY, 0644)');
await line('write(fd, "hello from the page cache\\n", 26)');
await line("close(fd)");
await line('int rfd = openat(AT_FDCWD, "/tmp/note", O_RDONLY)');
const got = await line("char got[64]");
const { s: read } = await line("read(rfd, got, 64)");
check("read 26 bytes back", read?.ret === 26 && m.show(got.ty, got.addr, 64).startsWith('"hello from the page cache'));
const inRead = m.line_steps(read.first, read.last, read.task.addr).find((x) => x.func === "vfs_read").i;
{
  // (Its last line, the closing brace: `return ret;` compiles to no line of its own.)
  const last = m.line_steps(read.first, read.last, read.task.addr).findLast((x) => x.func === "vfs_read");
  check(`vfs_read's last line (${last?.line}) says what it returns: ${last?.ret}`, last?.ret === 26);
}
const file = m.call_stack(inRead)[0].args.find((a) => a.name === "file");
const fpos = (i) => (m.memory_at(i), m.object_by_id("file", file.target.ty, file.target.addr, 1)?.children?.find((c) => c.name === "f_pos"));
check("f_pos before the read, and after", fpos(read.first)?.value === "0" && fpos(read.last)?.value === "26");
// (The program's buffer is kernel memory: the read fills it, recorded.)
const buf = (i) => (m.memory_at(i), m.call_stack(inRead)[0].args.find((a) => a.name === "buf")?.value);
check("the user's buffer, before the read and after", !/hello/.test(buf(read.first)) && /"hello from the page cache/.test(buf(read.last)));
const who = (m.memory_at(read.last), m.origin(fpos(read.last).addr, 8)[0]);
check("who wrote f_pos: ksys_read", who?.at?.func === "ksys_read");
// What the read did (Objects): its file's f_pos, 0 to 26, by ksys_read.
{
  const obj = m.objects(read.first, read.last).find((n) => n.addr === file.target.addr && n.id === file.target.ty);
  const d = obj?.changes.find((x) => x.member === "f_pos");
  check(`the read wrote its file: ${obj?.changes.map((x) => x.member).join(", ")}; f_pos ${d?.before} → ${d?.after} by ${d?.runs.map((r) => r.at?.func).join(", ")}`,
        d?.before === "0" && d?.after === "26" && d.runs.length === 1 && d.runs[0].at?.func === "ksys_read");
  // The read's stores, named as the code names what they hit: a local of a call in it (at its frame base's offset), the kiocb's ki_pos, on its lines.
  const notes = m.notes(read.first, read.last, "fs/read_write.c", read.last);
  check(`the read's notes in fs/read_write.c: ${notes.map((n) => n.short).slice(0, 4).join("; ")}…`,
        notes.some((n) => /^ki_pos: /.test(n.short) && /\bkiocb(->|\.)ki_pos in \w+/.test(n.full)));
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
// A program's own variables: memory in the program's process, passed by
// address, read back as C writes them; a blocked read goes on as a later
// write wakes it.
const shown = (v) => m.show(v.ty, v.addr, v.count ?? undefined);
{
  const buf = await line("char buf[16]"), fds = await line("int fds[2]");
  await line("pipe2(fds, 0)");
  await line('write(fds[1], "hi", 2)');
  const n = (await line("read(fds[0], buf, 16)")).ret;
  check(`variables: fds = ${shown(fds)}, buf = ${shown(buf)}`, n === 2 && shown(buf) === '"hi"');
  // (A thread, a runner of its own, reads: it blocks, nothing written yet;
  // the program writes, and wakes it.)
  await line("void *mem = mmap2(NULL, 65536, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0)");
  await line("pid_t thread = clone(CLONE_VM | CLONE_FS | CLONE_FILES | CLONE_SIGHAND | CLONE_THREAD | CLONE_SYSVSEM, mem + 65536, NULL, NULL, 0)");
  const reading = await line("thread: read(fds[0], buf, 8)"), wrote = await line('write(fds[1], "ok", 2)');
  check(`a blocked read, woken: wrote ${wrote.ret}, read ${fin.get(reading.id)?.ret} (blocked first: ${reading.how == null}): buf = ${shown(buf)}`,
        reading.how == null && wrote.ret === 2 && fin.get(reading.id)?.ret === 2 && shown(buf) === '"ok"');
  // (prctl, with the constants of linux/prctl.h: the task's name set, read back, and set back.)
  const name = await line('char name[16] = "ksee"'), got = await line("char got16[16]");
  const set = await line("prctl(PR_SET_NAME, name, 0, 0, 0)"), get = await line("prctl(PR_GET_NAME, got16, 0, 0, 0)");
  await line('name = "prog"');
  await line("prctl(PR_SET_NAME, name, 0, 0, 0)");
  check(`prctl(PR_SET_NAME) = ${set.ret}, PR_GET_NAME: ${shown(got)}; name set again: ${shown(name)}`, set.ret === 0 && get.ret === 0 && shown(got) === '"ksee"' && shown(name) === '"prog"');
  // (A struct assigned whole.)
  const ts = await line("struct __kernel_timespec ts");
  await line("ts = {1, 5}");
  check(`a struct assigned: ts = ${shown(ts)}`, shown(ts) === "{tv_sec = 1, tv_nsec = 5}");
}
// A 64-bit argument, whole: llseek's offset (high 1, low 0) is vfs_llseek's loff_t 1 << 32.
{
  await line("long long res");
  const ls = (await line("llseek(rfd, 1, 0, &res, 0)")).s;
  const at = ls && m.first_call("vfs_llseek", ls.first, ls.last, ls.task.addr);
  const off = at != null && m.call_stack(at).find((f) => f.func === "vfs_llseek")?.args?.find((a) => a.name === "offset")?.value;
  check(`a 64-bit argument whole: vfs_llseek's offset ${off}`, off === "4294967296");
  // (And a 64-bit result: what vfs_llseek returned, the position.)
  const got = ls && m.line_steps(ls.first, ls.last, ls.task.addr).find((s) => s.called?.func === "vfs_llseek")?.called.value;
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
await line("close(rfd)");
// (A user pointer must be in RAM: not the runtime's memory, below it.)
check("a bad user pointer: EFAULT", (await line("write(1, 0x1000, 4)")).ret === -14 && (await line("getpid()")).ret === m.main());

// A char * shows its string: mkdir's struct filename, its name.
{
  const mk = (await line('mkdirat(AT_FDCWD, "/mnt/named", 0755)')).s, i = m.first_call("filename_create", mk.first, mk.last);
  const arg = m.call_stack(i).find((f) => f.func === "filename_create")?.args?.find((a) => a.ty === "struct filename *");
  m.memory_at(i); // (as it was: the name is freed after the syscall)
  // (7.2's struct filename: the name pointer is in an anonymous struct, whose members are its own.)
  const name = arg && m.object_by_id("name", arg.target.ty, arg.target.addr, 1)?.children?.find((c) => c.name === "name");
  m.memory_at(null);
  check(`a char * with its string: filename->name = ${name?.value}`, name?.value?.endsWith('"/mnt/named"'));
  await line('unlinkat(AT_FDCWD, "/mnt/named", AT_REMOVEDIR)');
}
check("definitions: a function, a type, a global", m.definition("vfs_write")?.file === "fs/read_write.c"
      && m.definition("task_struct")?.file === "include/linux/sched.h" && m.definition("runqueues")?.line > 0 && m.definition("no_such") == null
      && m.definition("list_add")?.file === "include/linux/list.h" && !m.functions("list_add").length);
check("a field's declaration: file.f_pos, by its struct (or a pointer to it), or of the one struct with it; not of many",
      m.definition("file.f_pos")?.file === "include/linux/fs.h" && m.member_definition(file.id, "f_pos")?.file === "include/linux/fs.h"
      && m.member_definition(undefined, "f_pos")?.file === "include/linux/fs.h" && m.member_definition(undefined, "next") == null);
{
  // (The syscalls this kernel builds, as its syscall table in the image says (ksee pack): setuid (CONFIG_MULTIUSER), not io_setup (no CONFIG_AIO).)
  const names = m.syscall_names();
  let refused = "";
  try { m.run("io_setup(1, NULL)"); } catch (e) { refused = String(e.message ?? e); }
  check(`the syscalls built: ${names.length}; read and setuid, not io_setup (${refused})`,
        names.includes("read") && names.includes("setuid") && !names.includes("io_setup") && /no syscall io_setup/.test(refused));
  // (And setuid runs: root may become root, and it is recorded, as any call.)
  const su = await line("setuid(0)");
  check(`setuid(0) = ${su.ret}, recorded (${su.s?.name})`, su.ret === 0 && su.s?.name === "setuid" && ran(su.s, "__sys_setuid"));
}
check("a syscall's parameters, typed, with the constants that fit", JSON.stringify(m.syscall("openat")?.params) ===
      JSON.stringify([{ name: "dfd", ty: "int", consts: ["AT_"] }, { name: "filename", ty: "char *", consts: [] },
                      { name: "flags", ty: "int", consts: ["O_"] }, { name: "mode", ty: "umode_t", consts: [] }]));
// The syscall entry: the frame says which syscall, its arguments typed as the syscall's, as constants; the wrapper is a frame of its own, with the same parameters (its SYSCALL_DEFINE line's).
{
  const op = m.syscalls().find((s) => s.name === "openat"), stack = m.call_stack(m.first_call("do_sys_openat2", op.first, op.last)), entry = stack.at(-1);
  // (Memory reached through a pointer local, or the task's pointer members, is named too: files->next_fd, through the task's files.)
  const ops = m.objects(op.first, op.last);
  check(`a store through the task's files is named: ${ops.filter((n) => n.changes.some((d) => d.member === "next_fd")).map((n) => n.what).join(", ")}`,
        ops.some((n) => n.what.endsWith("->files") && n.changes.some((d) => d.member === "next_fd")));
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
// (A line checked before it is made: with the names of the lines before it.)
const refusal = (text, line) => { try { m.check(text, line); return ""; } catch (e) { return String(e.message ?? e); } };
check("a line checked: a wrong member, a name not made yet, too many arguments",
      /no member/.test(refusal("clock_nanosleep(1, 0, {.tv_min = 1}, NULL)", 1e9)) && /rfd/.test(refusal("close(rfd)", 0))
      && refusal("close(rfd)", 1e9) === "" && /arguments/.test(refusal("getpid(1)", 1e9)));
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
    if (st.call && !fin.get(asked[j])) blocked.push(`${j + 1}. ${st.call}`);
  });
  // ([label](2:func:any): see examples.js; call 2, declarations not counted.)
  const calls = asked.filter((_, j) => ex.steps[j].call);
  for (const [, n, func, any] of (ex.text ?? []).join(" ").matchAll(/\]\((\d+)(?::([\w.]+))?(:any)?\)/g)) {
    const s = syscallOf(calls[Number(n) - 1]);
    if (!s || (func && m.first_call(func, s.first, s.last ?? m.records() - 1, any ? undefined : s.task.addr) == null)) missing.push(`${n}:${func ?? ""}`);
  }
  // (Its tries: each a program that runs; its calls finish, or block on purpose.)
  for (const [k, prog] of (ex.tries ?? []).entries()) {
    const steps = m.program_lines(prog).map((c) => m.statement(c));
    const ran = await runExample({ ...ex, steps });
    check(`example ${ex.name}, try ${k + 1}: ${steps.length} calls made (${prog.slice(0, 50)}…)`, ran.length === steps.length && steps.length > 0);
  }
  check(`example ${ex.name}: ${ex.steps.length} calls, all finished, the explanation's links found`,
        !missing.length && !blocked.length || console.log("  missing:", missing, "blocked:", blocked));
}

// A fresh kernel, in place: nothing of the calls before (/tmp is gone).
await line('mkdirat(AT_FDCWD, "/tmp", 0755)');
check("/tmp is there before", (await line('mkdirat(AT_FDCWD, "/tmp", 0755)')).ret === -17);
const before = out.length;
// (Restarted while a call is under way, its quiet not come: the calls after it still finish.)
m.run("getpid()");
kern.quiet().catch(() => {});
await m.restart();
const after = await Promise.race([line("getpid()"), new Promise((ok) => setTimeout(() => ok(null), 5000))]);
check(`a call after a restart cut one short finishes (${after?.ret})`, after?.ret === m.main());
check("restarted: the program ready again, its names gone", m.runners().includes(m.main()) && (await line("getpid()")).ret === m.main() && m.names_before(1e9).length === 0);
check("... on a fresh kernel: /tmp is not there", (await line('mkdirat(AT_FDCWD, "/tmp", 0755)')).ret === 0);

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
m.run("reboot(0xfee1dead, 672274793, 0x4321fedc, 0)");
check("powered off", (await until(() => m.halted() !== undefined && m.halted() + 1)) === 1);

check("no warning from the kernel (an execve has its argv)", !out.includes("NULL argv"));
if (failed) console.log(out);
console.log(failed ? "FAIL" : "PASS");
Deno.exit(failed ? 1 : 0);
