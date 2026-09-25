// The syscall cards: a form per call, built from the kernel's signature
// (web/calls.js), made in /init. After a run a card reads like strace, and
// links to what the call ran; "Step into" stops the kernel at its first
// line, to play or step through it.
import { $, el } from "./dom.js";
import { Session, SCENARIOS, retText } from "./calls.js";

export function cards(app) {
  const { m, proc } = app;
  const session = new Session(m, proc);
  const list = []; // { s, values, node, ... }
  let busy = Promise.resolve();

  // The fds offered to fd fields: the named ones, then the standard ones.
  function refreshFds() {
    const opts = [["latest", "the newest fd"], ...session.fds.map((f) => [f.label, `fd ${f.fd}`]),
                  ["0", "stdin"], ["1", "stdout"], ["2", "stderr"], ["AT_FDCWD", "the working directory"]];
    $("fds").replaceChildren(...opts.map(([value, label]) => el("option", { value, label })));
  }

  // One field's input; `get`/`set` its value in `values`.
  function widget(f, get, set) {
    const input = ({ list, ...props } = {}) => {
      const i = el("input", { value: get() ?? "", spellcheck: false, ...props });
      if (list) i.setAttribute("list", list);
      i.oninput = () => set(i.value);
      return i;
    };
    if (f.kind === "fd") return input({ className: "fd", list: "fds", title: "a number, latest, or a file's name" });
    if (f.kind === "in") return input({ className: "wide", placeholder: f.path ? "a path" : "text (\\n for a newline); empty: NULL" });
    if (f.kind === "out")
      return el("span", { className: "inline" }, input({ className: "num", placeholder: "size" }),
                el("span", { className: "dim", textContent: get() === "" ? " out (its size)" : " bytes out" }));
    if (f.flags) {
      const i = input({ className: "wide" });
      const chips = f.flags.map((name) => {
        const c = el("button", { className: "chip", textContent: name, type: "button" });
        const on = () => i.value.split("|").map((x) => x.trim()).includes(name);
        c.classList.toggle("on", on());
        c.onclick = () => {
          const parts = i.value.split("|").map((x) => x.trim()).filter((x) => x && x !== "0" && x !== name);
          if (!on()) parts.push(name);
          i.value = parts.join("|") || "0";
          set(i.value);
          chips.forEach((c, k) => c.classList.toggle("on", i.value.split("|").includes(f.flags[k])));
        };
        return c;
      });
      i.addEventListener("input", () => chips.forEach((c, k) => c.classList.toggle("on", i.value.split("|").includes(f.flags[k]))));
      return el("span", { className: "flags" }, i, ...chips);
    }
    if (f.choices) {
      const id = `choices-${f.name}`;
      if (!$(id)) document.body.append(el("datalist", { id }, ...f.choices.map((c) => el("option", { value: c }))));
      return input({ className: "num", list: id });
    }
    return input({ className: "num", placeholder: "0, 0x10, 0644, auto" });
  }

  // A struct parameter: a row of fields per element, and + to add one.
  function structWidget(f, values) {
    const box = el("div", { className: "struct" });
    const draw = () => {
      const rows = values[f.name].map((e, k) => el("div", { className: "elem" },
        el("span", { className: "dim", textContent: values[f.name].length > 1 ? `[${k}]` : "" }),
        ...f.fields.map((x) => el("label", {}, el("span", { className: "dim", textContent: x.name }),
          widget({ ...x, kind: x.text ? "in" : "value" }, () => e[x.name], (v) => (e[x.name] = v)))),
        values[f.name].length > 1 ? el("a", { textContent: "✕", title: "remove", onclick: () => {
          values[f.name].splice(k, 1);
          draw();
        } }) : null));
      box.replaceChildren(...rows, el("a", { className: "dim", textContent: `+ another ${f.struct}`, onclick: () => {
        values[f.name].push({ ...values[f.name].at(-1) });
        draw();
      } }));
    };
    draw();
    return box;
  }

  function add(name, given = {}) {
    const s = session.byName.get(name);
    if (!s) return;
    const fields = session.fields(s);
    const values = Object.fromEntries(fields.map((f) => [f.name, structuredClone(given[f.name] ?? f.value)]));
    const c = { s, values };
    const form = el("div", { className: "form" }, ...fields.map((f) =>
      el("div", { className: "f" }, el("span", { className: "pname", title: f.type, textContent: f.name }),
        f.kind === "struct" ? structWidget(f, values) : widget(f, () => values[f.name], (v) => (values[f.name] = v)),
        el("span", { className: "ptype", textContent: f.type }))));
    c.line = el("span", { className: "line", textContent: `${s.name}(${s.params.map((p) => p.name).join(", ")})` });
    c.out = el("div", { className: "result" });
    const button = (text, title, onclick) => el("button", { textContent: text, title, onclick: (e) => (e.stopPropagation(), onclick()) });
    c.node = el("div", { className: "card open" },
      el("div", { className: "head", onclick: () => c.node.classList.toggle("open") },
        el("b", { textContent: s.name }), c.line,
        el("span", { className: "acts" },
          button("▶", "Run it", () => run(c)),
          button("⤵", "Step into: stop the kernel where the syscall starts", () => run(c, true)),
          button("✕", "Remove", () => {
            list.splice(list.indexOf(c), 1);
            c.node.remove();
          }))),
      form, c.out);
    list.push(c);
    $("cards").append(c.node);
    c.node.scrollIntoView({ block: "nearest" });
    return c;
  }

  // Run card `c` (after whatever runs now); `into`: stop at its start.
  function run(c, into = false) {
    c.node.classList.remove("ok", "err");
    c.node.classList.add("running");
    c.out.replaceChildren(el("span", { className: "dim", textContent: into ? "stepping into…" : "…" }));
    return (busy = busy.then(async () => {
      let r;
      try {
        if (into) app.stepInto(c.s.name);
        r = await session.call(c.s, c.values);
      } catch (e) {
        c.node.classList.replace("running", "err");
        c.out.replaceChildren(el("span", { className: "err", textContent: e.message }));
        return false;
      }
      c.node.classList.replace("running", r.ret < 0 ? "err" : "ok");
      c.node.classList.remove("open");
      c.line.textContent = r.line;
      c.key = null;
      const head = el("div", {}, el("span", { className: r.ret < 0 ? "err" : "ret", textContent: `= ${retText(r.ret)}` }));
      const made = app.madeSyscall(c.s.name);
      if (made) {
        c.key = app.key(made);
        head.append(" ", el("a", { textContent: `ran ${made.blocks} blocks in ${made.functions} functions`,
          title: "show what it ran", onclick: () => app.selectKey(c.key) }));
      }
      c.out.replaceChildren(head, ...r.outputs.map((o) => el("div", { className: "shown" },
        el("span", { className: "dim", textContent: `${o.name}: ` }),
        o.object ? el("a", { textContent: "decoded ›", onclick: () => app.showBytes(o.name, o.object) })
          : o.list ? o.list.join("  ·  ") : JSON.stringify(o.text))));
      refreshFds();
      return r.ret >= 0;
    }));
  }

  async function runAll() {
    for (const c of [...list]) await run(c);
  }
  async function reset() {
    await (busy = busy.then(() => session.reset()));
    refreshFds();
  }

  $("scenarios").append(...Object.keys(SCENARIOS).map((k) => el("option", { value: k, textContent: k })));
  $("scenarios").onchange = async () => {
    const calls = SCENARIOS[$("scenarios").value];
    $("scenarios").value = "";
    if (!calls) return;
    const made = calls.map(([name, values]) => add(name, values));
    for (const c of made) if (c && !(await run(c))) break;
  };
  $("syscall-list").append(...session.table.map((s) => el("option", { value: s.name,
    label: `${s.name}(${s.params.map((p) => `${p.type} ${p.name}`).join(", ")})` })));
  $("add").onchange = () => {
    const c = add($("add").value.trim());
    if (c) $("add").value = "";
  };
  $("runall").onclick = runAll;
  $("clear").onclick = async () => {
    list.splice(0).forEach((c) => c.node.remove());
    await reset();
  };
  refreshFds();

  const mark = (key) => list.forEach((c) => c.node.classList.toggle("on", key != null && c.key === key));
  return { add, run, runAll, mark, session };
}
