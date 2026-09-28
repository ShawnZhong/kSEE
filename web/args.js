// A call, written as in C, in a program's line (see calledit.js and
// examples.js): `name(arg, arg, …)`; `int fd = name(…)` (or `fd = name(…)`)
// names its result, for the calls after (`read(fd, buf, 64)`; $2 is call
// 2's result, always), its type as C would declare it (for the reader:
// the name is its value); `19: name(…)`, or `child: name(…)` (a name or
// $n: a clone's result), has another runner make it (a thread id). The
// arguments' language is the kernel's (crates/ksee-core/src/program.rs:
// Machine.arg).

// A call as written: { tid?: as written, type?, name?, call, args (their
// texts) }, or null if it is not one (a ; at the end is fine).
export function parseCall(text) {
  const m = String(text).trim().replace(/;$/, "")
    .match(/^(?:([$A-Za-z_]\w*)\s*:\s*)?(?:((?:(?:unsigned|signed|const|struct)\s+)?[A-Za-z_]\w*(?:\s+|\s*\*+\s*))(?=[A-Za-z_]\w*\s*=(?!=)))?(?:([A-Za-z_]\w*)\s*=(?!=)\s*)?([A-Za-z_]\w*)\s*\(([\s\S]*)\)$/);
  if (!m) return null;
  const [, tid, typed, name, call, inner] = m;
  const type = typed?.replace(/\s+/g, " ").replace(/ \*/, " *").trim();
  // (The arguments: split at the commas outside strings, () and {}.)
  const args = [];
  let cur = "", depth = 0, quote = false;
  for (let i = 0; i < inner.length; i++) {
    const c = inner[i];
    if (quote) {
      cur += c;
      if (c === "\\") cur += inner[++i] ?? "";
      else if (c === '"') quote = false;
    } else if (c === "," && depth === 0) {
      args.push(cur.trim());
      cur = "";
    } else {
      if (c === '"') quote = true;
      else if (c === "(" || c === "{") depth++;
      else if (c === ")" || c === "}") depth--;
      cur += c;
    }
  }
  if (cur.trim()) args.push(cur.trim());
  return { ...(tid ? { tid: /^\d+$/.test(tid) ? Number(tid) : tid } : {}), ...(type && name ? { type } : {}), ...(name ? { name } : {}), call, args };
}

// Argument `text` as the call was made: the results it names ($n, fd:
// `values`), and a variable's elements (fds[1]), replaced by their values
// (not in strings).
export const made = (text, values) => String(text).split(/("(?:[^"\\]|\\.)*")/).map((part, i) =>
  i % 2 ? part : part.replace(/\$?\b[A-Za-z_]\w*(?:\[\d+\])?|\$\d+/g, (w) => (w in values ? String(values[w]) : w))).join("");

// A result: a number, or an error's name (from the kernel's errno names).
export function result(r, consts) {
  if (r > 0xffff) return `0x${r.toString(16)}`; // (a pointer, a flag word)
  if (r >= 0) return String(r);
  const name = Object.keys(consts).find((c) => /^E[A-Z0-9]+$/.test(c) && consts[c] === -r);
  return `${r} (${name ?? "an error"})`;
}
