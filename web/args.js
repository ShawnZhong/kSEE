// A syscall's arguments, written as in C (in the examples, and the form):
//   "a string"         a buffer with the string, NUL-terminated
//   0644, 0x41, -100   numbers (0: octal, 0x: hex)
//   O_CREAT | O_WRONLY constants, with | and <<: the kernel's own, from its
//                      UAPI headers (Machine.constants); NULL
//   [64]               a buffer of 64 bytes, for the kernel to fill
//   {0, 30000000}      a buffer of 64-bit values (a struct timespec, say)
//   $2                 call 2's result (an fd, say)

// The value of argument `text` for Machine.call: a number, a string,
// { size } or { bytes }. `rets`: the results of the calls so far.
export function arg(text, consts, rets = []) {
  const s = String(text).trim();
  if (/^".*"$/s.test(s)) return JSON.parse(s);
  if (/^\[\d+\]$/.test(s)) return { size: Number(s.slice(1, -1)) };
  if (/^\{.*\}$/.test(s)) {
    const values = s.slice(1, -1).split(",").map((x) => BigInt(value(x, consts, rets)));
    return { bytes: new Uint8Array(new BigInt64Array(values).buffer) };
  }
  return value(s, consts, rets);
}

// A C constant expression: numbers, constants, NULL, $n, |, <<, unary -,
// parentheses.
function value(text, consts, rets) {
  const tokens = text.match(/\$\d+|[A-Za-z_]\w*|0x[0-9a-f]+|\d+|<<|[|()-]|\S/gi) ?? [];
  let k = 0;
  const peek = () => tokens[k], take = () => tokens[k++];
  function or() {
    let v = shift();
    while (peek() === "|") (take(), (v |= shift()));
    return v;
  }
  function shift() {
    let v = unary();
    while (peek() === "<<") (take(), (v <<= unary()));
    return v;
  }
  function unary() {
    const t = take();
    if (t === "-") return -unary();
    if (t === "(") {
      const v = or();
      if (take() !== ")") throw new Error(`a ) missing: ${text}`);
      return v;
    }
    if (t === undefined) throw new Error(`an argument missing: ${text}`);
    if (/^\$\d+$/.test(t)) {
      const r = rets[Number(t.slice(1)) - 1];
      if (r === undefined) throw new Error(`${t}: call ${t.slice(1)} is not made yet`);
      return r;
    }
    if (/^0x/i.test(t)) return parseInt(t, 16);
    if (/^0\d+$/.test(t)) return parseInt(t, 8);
    if (/^\d+$/.test(t)) return Number(t);
    if (t === "NULL") return 0;
    if (t in consts) return consts[t];
    throw new Error(`not a number, a constant or $n: ${t}`);
  }
  const v = or();
  if (k < tokens.length) throw new Error(`not an argument: ${text}`);
  return v;
}

// Argument `text` as the call was made: $n replaced by its value.
export const made = (text, rets) => String(text).replace(/\$(\d+)/g, (m, n) => (rets[n - 1] ?? m));

// A result: a number, or an error's name (from the kernel's errno names).
export function result(r, consts) {
  if (r >= 0) return String(r);
  const name = Object.keys(consts).find((c) => /^E[A-Z0-9]+$/.test(c) && consts[c] === -r);
  return `${r} (${name ?? "an error"})`;
}
