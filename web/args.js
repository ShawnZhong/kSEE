// A program's line, as the page shows it: a call as written, or made
// (`made`: with the results it names as their values); a result (`result`:
// with the kernel's errno name). The lines themselves are parsed in the
// core (Machine.statement: crates/ksee-core/src/program.rs).

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
