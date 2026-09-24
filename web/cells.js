// The notebook panel: lines of web/notebook.js, each run in /init, with its
// result under it. A syscall's result links to what it ran (the history).
//   Enter        run the line, then go to the next (a new one at the end)
//   Shift+Enter  a new line below
//   ↑ ↓          move between lines; Backspace on an empty line removes it
import { $, el } from "./dom.js";
import { Notebook, EXAMPLES } from "./notebook.js";

// A line that calls syscall `s`, with a setup line for each buffer it
// needs: [setup..., call].
function template(m, s, taken) {
  const setup = [], args = [];
  let lastLen = "64";
  const fresh = (base) => {
    let name = base, k = 2;
    while (taken.has(name)) name = base + k++;
    taken.add(name);
    return name;
  };
  for (const p of s.params ?? []) {
    const n = p.name;
    if (p.kind === "out" || (p.kind === "in" && p.aggregate)) {
      const name = fresh(p.aggregate ? n : "buf");
      if (p.aggregate && p.target) {
        setup.push(`${name} = new ${m.type_info(p.target).name}`);
        args.push(`&${name}`);
        lastLen = `sizeof(${m.type_info(p.target).name})`;
      } else {
        setup.push(`${name} = alloc(256)`);
        args.push(name);
        lastLen = `len(${name})`;
      }
    } else if (p.kind === "in") {
      const text = /name|path|file/.test(n) ? "/" : "hello kSEE\\n";
      args.push(`"${text}"`);
      lastLen = String(text.replace("\\n", "\n").length);
    } else if (n === "dfd") args.push("AT_FDCWD");
    else if (/mode/.test(n)) args.push("0644");
    else if (/^(count|len|size|nbytes|bufsiz)$/.test(n)) args.push(lastLen);
    else if (/^fd$/.test(n)) args.push(taken.has("fd") ? "fd" : "1");
    else args.push("0");
  }
  return [...setup, `${s.name}(${args.join(", ")})`];
}

export function cells(app) {
  const { m, proc } = app;
  const nb = new Notebook(m, proc);
  const table = m.syscall_table().filter((s) => s.built && s.params);
  const byName = new Map(table.map((s) => [s.name, s]));
  const list = []; // { text, node, input, out }
  let busy = Promise.resolve();

  function add(text = "", after = list.length - 1) {
    const c = { text };
    c.input = el("input", { className: "line", value: text, spellcheck: false, placeholder: "fd = openat(AT_FDCWD, \"/\", O_RDONLY)" });
    c.out = el("div", { className: "out" });
    c.node = el("div", { className: "cell" },
      el("button", { className: "run", textContent: "▶", title: "Run this line (Enter)", onclick: () => run(c) }), c.input, c.out);
    c.input.oninput = () => (c.text = c.input.value);
    c.input.onkeydown = (e) => key(c, e);
    list.splice(after + 1, 0, c);
    const before = list[after + 2]?.node ?? null;
    $("cells").insertBefore(c.node, before);
    return c;
  }
  function remove(c) {
    list.splice(list.indexOf(c), 1);
    c.node.remove();
  }
  const focus = (c) => c && (c.input.focus(), c.node.scrollIntoView({ block: "nearest" }));

  function key(c, e) {
    const i = list.indexOf(c);
    if (e.key === "Enter" && e.shiftKey) {
      e.preventDefault();
      focus(add("", i));
    } else if (e.key === "Enter") {
      e.preventDefault();
      run(c);
      focus(list[i + 1] ?? add());
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      focus(list[i - 1]);
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      focus(list[i + 1]);
    } else if (e.key === "Backspace" && !c.input.value && list.length > 1) {
      e.preventDefault();
      remove(c);
      focus(list[Math.max(0, i - 1)]);
    }
  }

  // Run line `c` (after whatever is running): its result, and a link to
  // the syscall it made.
  function run(c) {
    c.node.className = "cell running";
    c.out.replaceChildren(el("span", { className: "dim", textContent: "…" }));
    return (busy = busy.then(async () => {
      let r;
      try {
        r = await nb.run(c.text);
      } catch (e) {
        c.node.className = "cell err";
        c.out.replaceChildren(el("span", { className: "err", textContent: e.message }));
        return false;
      }
      c.node.className = "cell " + (r.ok ? "ok" : "err");
      const head = el("div", {}, el("span", { className: r.ok ? "" : "err", textContent: r.text }));
      c.key = null;
      if (r.syscall) {
        const s = app.madeSyscall(r.syscall);
        if (s) {
          c.key = app.key(s);
          head.append(" ", el("a", { className: "trace", textContent: `ran ${s.blocks} blocks in ${s.functions} functions`,
            title: "show what it ran", onclick: () => app.selectKey(c.key) }));
        }
      }
      c.out.replaceChildren(head, ...r.shown.map((t) => el("div", { className: "shown", textContent: t })));
      return r.ok;
    }));
  }

  function load(text) {
    list.splice(0).forEach((c) => c.node.remove());
    for (const line of text.split("\n")) add(line);
    add();
  }
  async function runAll() {
    await reset();
    for (const c of [...list]) if (c.text.trim()) await run(c);
  }
  async function reset() {
    await (busy = busy.then(() => nb.reset()));
    for (const c of list) {
      c.node.className = "cell";
      c.out.replaceChildren();
    }
  }

  $("examples").append(...Object.keys(EXAMPLES).map((k) => el("option", { value: k, textContent: k })));
  $("examples").onchange = () => {
    if ($("examples").value) load(EXAMPLES[$("examples").value]);
    $("examples").value = "";
    runAll();
  };
  $("syscall-list").append(...table.map((s) => el("option", { value: s.name,
    label: `${s.name}(${s.params.map((p) => `${p.type} ${p.name}`).join(", ")})` })));
  $("add").onchange = () => {
    const s = byName.get($("add").value.trim());
    if (!s) return;
    $("add").value = "";
    const taken = new Set([...list.map((c) => c.text.match(/^\s*(\w+)\s*=/)?.[1]).filter(Boolean)]);
    let last = list.findLastIndex((c) => c.text.trim());
    let c;
    for (const line of template(m, s, taken)) c = add(line, last++);
    focus(c);
  };
  $("runall").onclick = runAll;
  $("reset").onclick = reset;

  // Highlight the lines that made syscall `key`.
  const mark = (key) => list.forEach((c) => c.node.classList.toggle("on", key != null && c.key === key));
  load(EXAMPLES.files);
  return { mark, run, runAll, load };
}
