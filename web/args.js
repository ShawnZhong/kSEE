// A call's result, as the page shows it: with the kernel's errno name.
// (The program's lines themselves are parsed in the core: Machine.statement,
// crates/ksee-core/src/program.rs.)

// A result: a number, or an error's name (from the kernel's errno names).
export function result(r, consts) {
  if (r > 0xffff) return `0x${r.toString(16)}`; // (a pointer, a flag word)
  if (r >= 0) return String(r);
  const name = Object.keys(consts).find((c) => /^E[A-Z0-9]+$/.test(c) && consts[c] === -r);
  return `${r} (${name ?? "an error"})`;
}
