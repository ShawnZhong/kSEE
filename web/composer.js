// "Make a syscall": a form for any of the kernel's syscalls, built from its
// parameters (names and C types from DWARF), and presets. /init, the agent,
// makes the call (web/ksee.js: call).
import { call } from "./ksee.js";
import { $, el, retText } from "./dom.js";

// Named constants for arguments ("O_WRONLY|O_CREAT"); octal as 0644; $N is
// the result of step N of a preset.
const CONSTANTS = {
  AT_FDCWD: -100, AT_REMOVEDIR: 0x200, AT_SYMLINK_NOFOLLOW: 0x100, AT_EMPTY_PATH: 0x1000,
  O_RDONLY: 0, O_WRONLY: 1, O_RDWR: 2, O_CREAT: 0o100, O_EXCL: 0o200, O_NOCTTY: 0o400, O_TRUNC: 0o1000,
  O_APPEND: 0o2000, O_NONBLOCK: 0o4000, O_DIRECTORY: 0o200000, O_NOFOLLOW: 0o400000, O_CLOEXEC: 0o2000000,
  SEEK_SET: 0, SEEK_CUR: 1, SEEK_END: 2, CLOCK_REALTIME: 0, CLOCK_MONOTONIC: 1, CLOCK_BOOTTIME: 7,
};
function number(text, results) {
  let v = 0;
  for (const part of String(text).split("|").map((s) => s.trim()).filter(Boolean)) {
    let n;
    if (part in CONSTANTS) n = CONSTANTS[part];
    else if (/^\$\d+$/.test(part)) n = results[Number(part.slice(1)) - 1] ?? 0;
    else if (/^-?0[0-7]+$/.test(part)) n = parseInt(part, 8);
    else n = Number(part);
    if (!Number.isFinite(n)) throw new Error(`not a number: ${part}`);
    v = v === 0 ? n : v | n;
  }
  return v;
}
const unescape = (s) => s.replace(/\\n/g, "\n").replace(/\\t/g, "\t").replace(/\\0/g, "\0");

const PRESETS = [
  ["getpid", [["getpid", {}]]],
  ["uname", [["uname", {}]]],
  ["ls /", [["openat", { dfd: "AT_FDCWD", filename: { text: "/", nul: true }, flags: "O_RDONLY|O_DIRECTORY" }],
            ["getdents64", { fd: "$1", dirent: { len: 1024 }, count: "1024" }], ["close", { fd: "$1" }]]],
  ["write a file", [["mkdirat", { dfd: "AT_FDCWD", pathname: { text: "/tmp", nul: true }, mode: "0755" }],
                    ["openat", { dfd: "AT_FDCWD", filename: { text: "/tmp/hello", nul: true }, flags: "O_WRONLY|O_CREAT|O_TRUNC", mode: "0644" }],
                    ["write", { fd: "$2", buf: "hello kSEE\\n", count: "auto" }], ["close", { fd: "$2" }]]],
  ["read it back", [["openat", { dfd: "AT_FDCWD", filename: { text: "/tmp/hello", nul: true }, flags: "O_RDONLY" }],
                    ["read", { fd: "$1", buf: { len: 64 }, count: "64" }], ["close", { fd: "$1" }]]],
  ["sleep 20 ms", [["clock_nanosleep_time64", { which_clock: "CLOCK_MONOTONIC", flags: "0", rqtp: { tv_sec: "0", tv_nsec: "20000000" } }]]],
];

export function composer(app) {
  const { m } = app;
  const table = m.syscall_table();
  const byName = new Map(table.map((s) => [s.name, s]));
  const signature = (s) => `${s.name}(${(s.params ?? []).map((p) => `${p.type} ${p.name}`).join(", ")})`;

  // The agent's request for syscall `s` with argument texts keyed by
  // parameter name (input strings: text or { text, nul }; input structs:
  // { field: text }; outputs: { len }).
  function request(s, given, results = []) {
    const args = [0, 0, 0, 0, 0, 0], buffers = [], outs = [];
    let lastLen = 0;
    (s.params ?? []).forEach((p, i) => {
      const v = given[p.name];
      if (p.kind === "in" && p.aggregate) {
        const bytes = new Uint8Array(p.size), view = new DataView(bytes.buffer);
        for (const f of m.layout(p.target)) {
          const x = BigInt(number(v?.[f.name] || "0", results));
          if (f.size === 8) view.setBigInt64(f.offset, x, true);
          else if (f.size === 4) view.setInt32(f.offset, Number(x), true);
          else if (f.size === 2) view.setInt16(f.offset, Number(x), true);
          else if (f.size === 1) view.setInt8(f.offset, Number(x));
        }
        buffers.push({ arg: i, data: [...bytes] });
        lastLen = p.size;
      } else if (p.kind === "in") {
        if (v === undefined || v === "") return; // NULL
        const { text, nul } = typeof v === "string" ? { text: v, nul: false } : v;
        const bytes = [...new TextEncoder().encode(unescape(text))];
        lastLen = bytes.length;
        if (nul) bytes.push(0);
        buffers.push({ arg: i, data: bytes });
      } else if (p.kind === "out") {
        const len = v?.len === undefined ? (p.aggregate ? p.size : 256) : number(v.len, results);
        if (!len) return; // NULL
        buffers.push({ arg: i, len });
        outs.push(p);
        lastLen = len;
      } else {
        args[i] = v === undefined || v === "" ? 0 : v === "auto" ? lastLen : number(v, results);
      }
    });
    return { req: { nr: s.nr, args, buffers }, outs };
  }

  // Make a call; log it with its result and outputs; return the result.
  async function run(s, given, results = []) {
    const { req, outs } = request(s, given, results);
    const shown = (s.params ?? []).map((p, i) => {
      const b = req.buffers.find((b) => b.arg === i);
      if (!b) return String(req.args[i]);
      if (b.len !== undefined) return `<${b.len} bytes out>`;
      if (p.aggregate) return "{…}";
      const text = new TextDecoder().decode(new Uint8Array(b.data)).replace(/\0$/, "");
      return JSON.stringify(text.length > 24 ? text.slice(0, 24) + "…" : text);
    });
    const line = el("div", { className: "logline" }, el("span", { className: "mono", textContent: `${s.name}(${shown.join(", ")})` }));
    $("log").append(line);
    const r = await call(m, req);
    line.append(el("span", { className: r.ret < 0 ? "mono err" : "mono ok", textContent: ` = ${retText(r.ret)}` }));
    if (r.ret >= 0)
      r.outputs.forEach((bytes, i) => {
        const p = outs[i];
        if (p.aggregate) {
          const o = m.decode_bytes(p.name, p.target, bytes);
          const parts = (o.children ?? []).slice(0, 8).map((c) => `${c.name} = ${c.value ?? "{…}"}`);
          line.append(el("div", { className: "out mono", textContent: `${p.name}: { ${parts.join(", ")} }` }));
        } else {
          const n = r.ret > 0 && r.ret <= bytes.length ? r.ret : bytes.length;
          const text = new TextDecoder().decode(bytes.slice(0, n)).replace(/[\0-\x08\x0e-\x1f]+/g, " ").trim();
          line.append(el("div", { className: "out mono", textContent: `${p.name}: ${JSON.stringify(text)}` }));
        }
      });
    $("log").scrollTop = $("log").scrollHeight;
    app.afterCall(s.name);
    return r.ret;
  }

  let chosen = null;
  function compose(s) {
    chosen = s;
    $("sysname").value = signature(s);
    const fields = (s.params ?? []).map((p, i) => {
      let input;
      if (p.kind === "in" && p.aggregate) {
        input = el("span", { className: "struct" }, ...m.layout(p.target).map((f) =>
          el("label", {}, `${f.name} `, el("input", { dataset: { param: p.name, field: f.name }, size: 8, value: "0" }))));
      } else if (p.kind === "in") {
        const path = /name|path|file/.test(p.name);
        input = el("span", { className: "inline" },
          el("input", { dataset: { param: p.name }, value: path ? "/" : "hello kSEE\\n",
                        placeholder: "text (\\n: newline); empty: NULL" }),
          el("label", { className: "dim" }, el("input", { type: "checkbox", checked: path, dataset: { nul: p.name } }), " NUL"));
      } else if (p.kind === "out") {
        input = el("span", { className: "inline" }, el("input", { dataset: { param: p.name, len: "1" }, value: String(p.aggregate ? p.size : 256) }),
                   el("span", { className: "dim", textContent: " bytes out" }));
      } else {
        const def = p.name === "dfd" ? "AT_FDCWD" : /^(count|len|size|nbytes)$/.test(p.name) && i > 0 ? "auto"
          : /mode/.test(p.name) ? "0644" : /^fd$/.test(p.name) && s.name === "write" ? "1" : "0";
        input = el("input", { dataset: { param: p.name }, value: def, placeholder: "number, 0x.., 0644, O_CREAT|O_WRONLY, auto" });
      }
      return el("div", { className: "field" }, el("span", { className: "pname mono", textContent: `${p.type} ${p.name}` }), input);
    });
    const note = s.params ? null : el("div", { className: "dim", textContent: "Parameters unknown: raw arguments." });
    $("fields").replaceChildren(...fields, note ?? "", el("button", { className: "go", textContent: `Run ${s.name}`, onclick: runChosen }));
  }
  function given() {
    const g = {};
    for (const i of $("fields").querySelectorAll("input[data-param]")) {
      const { param, field, len } = i.dataset;
      if (field) (g[param] ??= {})[field] = i.value;
      else if (len) g[param] = { len: i.value };
      else {
        const nul = $("fields").querySelector(`input[data-nul="${param}"]`);
        g[param] = nul ? { text: i.value, nul: nul.checked } : i.value;
      }
    }
    return g;
  }
  const failed = (e) => $("log").append(el("div", { className: "logline err", textContent: e.message }));
  async function runChosen() {
    try {
      await run(chosen, given());
    } catch (e) {
      failed(e);
    }
  }

  $("syscall-list").append(...table.filter((s) => s.built && s.params).map((s) => el("option", { value: signature(s) })));
  $("sysname").onchange = () => {
    const name = $("sysname").value.split("(")[0].trim();
    if (byName.has(name)) compose(byName.get(name));
  };
  $("presets").append(...PRESETS.map(([label, steps]) => el("button", { className: "chip", textContent: label, onclick: async () => {
    const results = [];
    for (const [name, args] of steps) {
      try {
        results.push(await run(byName.get(name), args, results));
      } catch (e) {
        failed(e);
        break;
      }
    }
    compose(byName.get(steps.at(-1)[0]));
  } })));
  compose(byName.get("write"));
}
