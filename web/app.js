// The kSEE page: wires the panels to a booted Machine (crates/ksee-runtime).
//
// An example's program makes calls, each recorded; explore the recording
// at one cursor: the timeline shows it among the tasks and their calls;
// the source and the Stack show the code and the values there (memory as
// it was). ◀ ▶| and Play move it line by line.
import { $, el, taskName } from "./dom.js";
import { source } from "./source.js";
import { timeline } from "./timeline.js";
import { callstack } from "./callstack.js";
import { examples } from "./examples.js";
import { trace } from "./trace.js";
import { objects } from "./objects.js";
import { tasks } from "./tasks.js";
import { kernel } from "./kernel.js";
import { next } from "./steps.js";
import { layout } from "./layout.js";


// 1st, 2nd, 3rd, 4th, …, 11th, 12th, 13th, 21st.
const ordinal = (n) => `${n}${n % 100 >= 11 && n % 100 <= 13 ? "th" : ["th", "st", "nd", "rd"][n % 10] ?? "th"}`;

export function app(m) {
  const a = { m };
  // The kernel, for the program's lines (kernel.js): each call's
  // recording on the page as the kernel is quiet after it.
  a.kernel = kernel(m, { snapshot: () => a.refresh() });
  const src = source(a), time = timeline(a), stack = callstack(a);
  // (The panels, for each other and for tests.)
  Object.assign(a, { src, time, stack });

  // ---- The state: a recording to explore, or none yet ----
  let halted = null;
  const state = () => (halted != null ? "halted" : time.records() ? "replay" : "idle");
  // (The status, in the transport: only what the panels do not say.)
  function badge() {
    const st = state();
    $("status").classList.toggle("halted", st === "halted");
    $("status").textContent = {
      idle: "Nothing recorded yet: an example's calls are, as it opens.",
      replay: "",
      halted: `the kernel halted (${halted})`,
    }[st];
  }

  // A new snapshot of the recording, everywhere.
  let calls = [];
  const refresh = () => {
    m.snapshot();
    calls = m.syscalls();
    time.refresh(calls);
    a.outlined();
    return calls;
  };
  a.refresh = refresh;

  // ---- Memory: as it was at the cursor ----
  let memAt;
  function memoryAt(i) {
    if (i === memAt) return;
    memAt = i;
    m.memory_at(i ?? undefined);
  }

  // ---- Selecting, and moving the cursor ----

  // The place shown, in the transport: `task` · `name` `file`:`line` · in `sc`.
  // (`ret`: a step that lands after a return says what returned: "vfs_read returned 26".)
  const where = ({ task, name, file, line, sc, ret }) => ($("where").textContent =
    [task && taskName(task), `${name} ${file ?? ""}:${line ?? ""}`, ret, sc && `in ${sc}`].filter(Boolean).join(" · "));

  // Select syscall `s`: its records; the cursor where its own code starts
  // (its handler's first line, past the syscall entry).
  a.select = (s) => {
    setLevel(null);
    time.select(s);
    time.go(m.next_block(s.first, true));
    a.outlined();
  };
  // Select call `f` (a Stack frame: see Machine.call_stack) at the cursor:
  // the Stack marks it, the source shows where it is, the timeline outlines it.
  // The view's position has two coordinates: the cursor (a record: the
  // timeline's x) and the call chosen on the stack there (`chosen`: its y;
  // null: the innermost). A click on a bar, or ▲ ▼, sets the call; a step
  // resets it to the innermost; the pointer over the timeline shows other
  // positions without touching it, and leaving comes back to both.
  let chosen = null;
  // The level ← → step at (calls deep, then inlined deep, as line steps
  // count them: Machine.line_steps; said as its row on the Stack, and the
  // call it was set at): set by selecting a call (a click on a bar or on
  // the Stack, ↑ ↓), and kept as the steps go (a call's siblings are at it
  // too); null, all: every line, at any depth (at first; ↑ past the
  // innermost).
  let level = null;
  const showLevel = () => $("level").replaceChildren(...(level
    ? [el("span", { textContent: `level ${level.row}`, title: `◀ ▶ stop at the lines of the Stack's ${level.row} outermost rows (set at ${level.func}): what the calls there make is stepped over` }),
       " · ", el("a", { textContent: "all", title: "every line, at any depth (↑ past the innermost)", onclick: () => setLevel(null) })]
    : [el("span", { textContent: "level: all", title: "◀ ▶ stop at every line, at any depth: ↓ steps at the level of the call selected (the innermost unless another is)" })]));
  const setLevel = (l) => ((level = l), showLevel());
  a.level = () => level;
  // (Only selected: the level ◀ ▶ step at is ▲ ▼'s, see climb.)
  a.selectFrame = (f) => {
    chosen = { first: f.first, inlined: f.inlined ? f.func : null };
    stack.select(f.first, f.inlined ? f.func : null);
    src.view.cursor = { file: f.file, line: f.line };
    const at = time.cursor() != null ? m.at(time.cursor()) : null, s = calls[at?.syscall] ?? null;
    // (Inlined code: in the function it is inlined into, at its own line.)
    const owner = f.inlined ? stack.owner(f) : f;
    src.showFunction(owner?.func ?? f.func, at?.syscall ?? undefined, f.line, f.file);
    time.selected(f.first);
    where({ task: at?.task, name: f.func, file: f.file, line: f.line, sc: s?.name });
  };
  // Select the call on the stack at the cursor whose first record is
  // `first` (or code inlined into it, from function `inlined`).
  a.selectCall = (first, inlined) => {
    const f = stack.select(first, inlined);
    if (f) a.selectFrame(f);
  };
  a.syscalls = () => calls;
  // The selection (time.home(): a syscall's records and task: the line
  // steps go through them), with a key that changes with it or with the
  // snapshot (the newest record's id: indexes move): what a panel made of
  // it holds until the key changes.
  const keyed = (w) => w && { ...w, key: `${w.a}:${w.b}:${w.task}:${m.record_id(time.records() - 1)}` };
  a.selection = () => keyed(time.home());
  // Show the moment `at` (see Machine.at): the source there, the Stack,
  // memory as it was, where it is. `frame` ({ first, inlined }, a call on
  // the stack then): that call selected on the Stack, the source at the
  // line it was at (the one calling what is above it), as a click on it
  // shows.
  function view(at, frame = null) {
    memoryAt(at.i);
    const s = calls[at.syscall] ?? null;
    stack.show(at.i, null, at.task);
    const f = frame && stack.select(frame.first, frame.inlined);
    const here = f ? { func: f.func, file: f.file, line: f.line, inlined: null, owner: (f.inlined ? stack.owner(f) : f)?.func } : at;
    src.view.cursor = here;
    if (here.file) src.showFunction(here.owner ?? here.func, s?.index, here.line, here.file);
    where({ task: at.task, name: here.inlined ?? here.func, file: here.file, line: here.line, sc: s?.name });
  }
  // The cursor moved to `at`.
  a.showAt = (at) => {
    L.pos = null;
    chosen = null;
    view(at);
    marks();
    badge();
  };
  // The view follows the pointer over the timeline: record `i` shown as if
  // the cursor were there, as call `frame` saw it if over one (once a
  // frame, and only as the moment or the call changes); null: back to the
  // cursor's. A click fixes the cursor (time.go).
  let peek = { raf: 0, key: null };
  a.peek = (i, frame = null) => {
    cancelAnimationFrame(peek.raf);
    peek.raf = requestAnimationFrame(() => {
      const to = i ?? time.cursor(), key = to == null ? null : `${to}:${frame?.first ?? ""}:${frame?.inlined ?? ""}`;
      if (i != null && key === peek.key) return; // (back to the cursor: always)
      peek.key = i == null ? null : key;
      const at = to != null ? m.at(to) : null;
      // (Back to the cursor at a line step, no call chosen: that step, its note too.)
      const step = i == null && !chosen && L.pos != null ? steps()[L.pos] : null;
      if (step) showLine(step);
      else if (at) view(at, i == null ? chosen : frame);
      // (The tabs follow too, back to the cursor's with null.)
      marks(to);
    });
  };
  // The right column's tabs but the Stack, by name: each draws for the
  // selection (`draw`; a tab not shown waits until it is looked at) and
  // follows the cursor (`mark(i)`: record `i`, or the cursor's).
  const tabs = { tasks: tasks(a), objects: objects(a), trace: trace(a) };
  a.tasks = tabs.tasks;
  const marks = (i) => Object.values(tabs).forEach((x) => x.mark(i));
  a.outlined = () => Object.values(tabs).forEach((x) => x.draw());
  // Show tab `name` (its pane and its link marked; #right's data-tab says
  // which, for the panels), drawn.
  a.tab = (name) => {
    $("right").dataset.tab = name;
    for (const x of document.querySelectorAll("#right [data-tab]")) x.classList.toggle("on", x.dataset.tab === name);
    tabs[name]?.draw();
  };
  for (const x of document.querySelectorAll("h2.tabs a")) x.onclick = () => a.tab(x.dataset.tab);
  a.tab("stack");
  examples(a);

  // ---- Lines: the line steps of the selection (a syscall) ----
  // (Machine.line_steps: every source line it ran, within blocks too.)
  const L = { key: null, steps: [], pos: null };
  function steps() {
    const w = a.selection();
    if (!w) return (L.steps = []);
    // (A selection of any task, as the boot: the cursor's task's lines.)
    const task = w.task ?? m.at(time.cursor() ?? w.a)?.task.addr ?? 0;
    const key = `${w.key}:${task}`;
    if (key !== L.key) {
      // (A new snapshot of the same selection: the step it was at, found
      // again by its record and which of its lines.)
      const was = L.pos != null ? L.steps[L.pos] : null;
      L.key = key;
      L.steps = m.line_steps(w.a, w.b, task);
      const k = was ? L.steps.findIndex((s) => s.i === was.i && s.nth === was.nth) : -1;
      L.pos = k >= 0 ? k : null;
    }
    return L.steps;
  }
  a.lineSteps = () => steps();
  // (The line step the cursor is at, if at one: for tests.)
  a.stepNow = () => (L.pos != null ? steps()[L.pos] : null);
  // Where in the steps the cursor is: at step `k` (the one shown last), or
  // `between` two, just before step `k` (the first at or after the cursor;
  // after a click in the timeline, say).
  function position() {
    const st = steps();
    if (L.pos != null && L.pos < st.length) return { k: L.pos, between: false };
    const cur = time.cursor() ?? 0, k = st.findIndex((x) => x.at >= cur);
    return k < 0 ? { k: st.length - 1, between: false } : { k, between: st[k].at > cur };
  }
  // Show step `k`: the cursor at its record, its line in the source.
  function showStep(k) {
    const st = steps(), step = st[k];
    if (!step) return false;
    L.pos = k;
    showLine(step);
    return true;
  }
  // Go to the call from record `first`: in it, at its first line, the
  // function a link names the one selected on the Stack (the innermost
  // there), with its arguments; the source there. (Inlined code: its first
  // record.)
  a.goCall = (first) => {
    setLevel(null);
    time.go(first);
  };
  // Show line `step` (see Machine.line_steps): the cursor at its record.
  // (The cursor, and memory, just before it runs, `at`; the Stack at its
  // record, where its block's lines are.)
  function showLine(step) {
    chosen = null;
    time.mark(step.at);
    memoryAt(step.at);
    marks();
    const at = m.at(step.i);
    // (Its note: which turn of a loop it is, the call running it again;
    // and at the call's last line, what it returns, the optimizer's
    // `return x;` being no line of its own.)
    // (And the comparisons it made: where it went, and why.)
    // (Not what a call it makes returns: it has not run yet; once it has,
    // the line says it, as the source's other lines do.)
    const note = [step.run > 1 && `${ordinal(step.run)} time here`,
                  step.tests.length && step.tests.join(", "),
                  step.ret != null && `returns ${m.result(step.ret)}`];
    src.view.cursor = { file: step.file, line: step.line, note: note.filter(Boolean).join("   ") || null };
    src.showFunction(step.func, at?.syscall ?? undefined, step.line, step.file); // (the lines that ran: in its syscall, as elsewhere)
    // (The stack as it runs: see Machine.call_stack's nth.)
    stack.show(step.i, step, at?.task);
    // (A line after a return: what returned, as a debugger's finish says it.)
    const ret = at?.kind === "return" && at.value != null ? `${at.func} returned ${m.result(at.value)}` : null;
    where({ task: at?.task, name: step.inlined ?? step.func, file: step.file, line: step.line, ret });
  }
  // Step, `dir` 1 forward, -1 back: to the next line at the level (see
  // steps.js); past the syscall, its task's next one (across).
  function step(dir) {
    const st = steps();
    if (!st.length) return false;
    const { k, between } = position(), j = next(st, k, dir, level, between);
    return j >= 0 ? showStep(j) : across(dir);
  }
  // Past the selected syscall's end (or before its start): its task's next
  // syscall (or the one before), at its first line (or its last), as a
  // debugger stepping out of one call goes on to the program's next.
  function across(dir) {
    const w = a.selection();
    if (w?.task == null) return false;
    const mine = (c) => c.task.addr === w.task;
    const s = dir > 0 ? calls.find((c) => mine(c) && c.first > w.a) : calls.findLast((c) => mine(c) && c.first < w.a);
    if (!s) return ($("where").textContent = `no syscall of ${taskName(calls.find(mine)?.task ?? { addr: w.task })} ${dir > 0 ? "after" : "before"} this one in the recording`), false;
    time.select(s);
    a.outlined();
    return showStep(dir > 0 ? 0 : steps().length - 1);
  }
  // Run to line `line` of `file`: the next (dir 1) or the last (-1) time it
  // runs, from the cursor; its syscall selected (line steps go on from it).
  a.runTo = (file, line, dir) => {
    const i = m.run_of(file, line, time.cursor() ?? 0, dir);
    if (i == null) return ($("where").textContent = `${file}:${line} does not run ${dir > 0 ? "again" : "before"} in the recording`);
    const at = m.at(i), s = calls[at?.syscall] ?? null;
    if (s) time.select(s);
    time.mark(i);
    // (Its step: at or after the block that runs it, which may run it after a call returns into it.)
    const st = steps(), k = st.findIndex((x) => x.i >= i && x.line === line && x.file === file);
    return k >= 0 ? showStep(k) : time.go(i);
  };
  // (The four buttons are the four keys: ◀ ▶ in time, at the level; ▲ ▼
  // in depth, setting it.)
  $("step-back").onclick = () => step(-1);
  $("step-fwd").onclick = () => step(1);
  // The level, one row: ↓ the one below, as deep as the Stack is now at
  // most (from all: the call selected, the innermost unless another is);
  // ↑ the one above, past the innermost all, every line. (Only these set
  // it: a click selects a call, and ◀ ▶ step as they did.)
  const climb = (dir) => {
    const frames = stack.frames(), rows = frames.length, sel = stack.level();
    if (!rows || (!level && dir < 0)) return;
    const row = (level?.row ?? (sel?.row ?? rows) + 1) - dir;
    if (row > rows) return setLevel(null);
    const f = frames[rows - Math.max(1, row)];
    a.selectFrame(f);
    setLevel({ ...stack.level(), func: f.func });
  };
  $("step-up").onclick = () => climb(-1);
  $("step-down").onclick = () => climb(1);
  // Two axes, a pair of keys each: ← → in time, the next line at the level
  // (that deep or shallower: a call's siblings too; every line, with no
  // level; past its syscall, into its task's next or last one); ↑ ↓ in
  // depth, the row above or below on the Stack (the callee, the caller:
  // the innermost is on top), setting the level, ↑ past the innermost: all,
  // every line. Home: the first line; /: find a function.
  // Anywhere on the page but where text is typed (a program's call, the
  // find box), in the source too.
  addEventListener("keydown", (e) => {
    if (e.target.closest?.("input:not([type=checkbox]), select, textarea") || e.altKey || e.ctrlKey || e.metaKey) return;
    const arrow = { ArrowRight: 1, ArrowLeft: -1 }[e.key], updown = { ArrowUp: -1, ArrowDown: 1 }[e.key];
    if (arrow) step(arrow);
    else if (updown) climb(updown);
    else if (e.key === "Home") steps().length && showStep(0);
    else if (e.key === "/") time.focusFind();
    else return;
    e.preventDefault();
  });

  // ---- The page's layout: the columns, the timeline's height, the console ----
  const page = layout();

  // ---- Starting again, and watching the kernel (its console, a halt) ----
  // Start the kernel again (a fresh one: the page stays); done once /init
  // is ready for calls.
  a.restart = async () => {
    halted = null;
    memAt = undefined; // (a fresh kernel's memory: asked again)
    // (Nothing of the old recording: its selection, its step, its call chosen.)
    time.clear();
    (chosen = null), (L.key = null), (L.pos = null), (peek.key = null);
    setLevel(null);
    page.clear();
    await m.restart();
    refresh();
    stack.show(undefined);
    badge();
  };
  function tick() {
    const out = m.console();
    if (out) page.add(out.replaceAll("\r", ""));
    const code = m.halted();
    if (code !== undefined && halted == null) {
      halted = code;
    }
    badge();
    setTimeout(tick, 50);
  }
  refresh();
  stack.show(undefined);
  showLevel();
  tick();
  return a;
}
