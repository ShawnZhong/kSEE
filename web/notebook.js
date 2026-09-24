// The syscall notebook: lines that run in init, the process the page's
// syscalls run in (web/ksee.js: process). Values live in init's memory, so
// syscalls that need buffers, structs, or pointers between them work.
//
//   [name =] syscall(arg, ...)      a syscall (names from the kernel's table)
//   name = alloc(N)                 N zeroed bytes in init
//   name = new TYPE[N] {init}       a kernel type (struct stat, int, ...) in
//                                   init; [N] and {init} optional
//   name = expr                     a number
//   name                            show a value
//
// Arguments: 12, 0x1f, 0644; constants (O_CREAT|O_WRONLY); "text" (placed
// in init, NUL-terminated); a name (a number's value, or a buffer's
// address); &name; name + N; name[i] (an element, read from init);
// sizeof(TYPE); len(name). Initializers: {a, b}, {.field = a}, nested; a
// string in a pointer field is placed in init too.

export const CONSTANTS = {
  AT_FDCWD: -100, AT_REMOVEDIR: 0x200, AT_SYMLINK_NOFOLLOW: 0x100, AT_EMPTY_PATH: 0x1000,
  O_RDONLY: 0, O_WRONLY: 1, O_RDWR: 2, O_CREAT: 0o100, O_EXCL: 0o200, O_NOCTTY: 0o400, O_TRUNC: 0o1000,
  O_APPEND: 0o2000, O_NONBLOCK: 0o4000, O_DIRECTORY: 0o200000, O_NOFOLLOW: 0o400000, O_CLOEXEC: 0o2000000,
  SEEK_SET: 0, SEEK_CUR: 1, SEEK_END: 2, CLOCK_REALTIME: 0, CLOCK_MONOTONIC: 1, CLOCK_BOOTTIME: 7,
  STATX_BASIC_STATS: 0x7ff, NULL: 0,
};

const ERRNO = {
  1: "EPERM", 2: "ENOENT", 3: "ESRCH", 4: "EINTR", 5: "EIO", 9: "EBADF", 11: "EAGAIN", 12: "ENOMEM",
  13: "EACCES", 14: "EFAULT", 16: "EBUSY", 17: "EEXIST", 20: "ENOTDIR", 21: "EISDIR", 22: "EINVAL",
  24: "EMFILE", 25: "ENOTTY", 28: "ENOSPC", 32: "EPIPE", 36: "ENAMETOOLONG", 38: "ENOSYS", 39: "ENOTEMPTY",
};
export const retText = (r) => (r < 0 && ERRNO[-r] ? `${r} (${ERRNO[-r]})` : `${r}`);

// ---- Parsing ---------------------------------------------------------------------

function tokenize(line) {
  const toks = [], re = /\s*(?:(0x[0-9a-fA-F]+|\d+)|([A-Za-z_]\w*)|("(?:\\.|[^"\\])*")|(\S))/y;
  let m;
  while (re.lastIndex < line.length && (m = re.exec(line))) {
    if (m[1]) toks.push({ num: m[1].startsWith("0x") ? parseInt(m[1], 16) : /^0\d/.test(m[1]) ? parseInt(m[1], 8) : Number(m[1]) });
    else if (m[2]) toks.push({ id: m[2] });
    else if (m[3]) toks.push({ str: JSON.parse(m[3].replace(/\\0/g, "\\u0000")) });
    else if (m[4]) toks.push({ p: m[4] });
  }
  return toks;
}

function parse(line) {
  const t = tokenize(line.replace(/\/\/.*$/, ""));
  let i = 0;
  const peek = (k = 0) => t[i + k];
  const is = (p) => peek()?.p === p;
  const eat = (p) => {
    if (!is(p)) throw new Error(`expected ${p}${peek() ? "" : " at the end"}`);
    i++;
  };
  const ident = () => {
    if (!peek()?.id) throw new Error("expected a name");
    return t[i++].id;
  };
  // TYPE: "struct x", "union x", or a name ("int", "u64", "unsigned int").
  const type = () => {
    let name = ident();
    if (name === "struct" || name === "union" || name === "enum") name += " " + ident();
    else while (["int", "long", "short", "char"].includes(peek()?.id) && /^(unsigned|signed|long|short)$/.test(name.split(" ").at(-1))) name += " " + ident();
    return name;
  };
  const expr = () => {
    let e = term();
    while (is("|") || is("+") || is("-")) {
      const op = t[i++].p;
      e = { op, a: e, b: term() };
    }
    return e;
  };
  const term = () => {
    const k = peek();
    if (!k) throw new Error("expected a value");
    if (k.num !== undefined) return i++, { num: k.num };
    if (k.str !== undefined) return i++, { str: k.str };
    if (k.p === "-") return i++, { op: "-", a: { num: 0 }, b: term() };
    if (k.p === "&") return i++, { addr: ident() };
    if (k.p === "{") return init();
    if (k.p === "(") {
      i++;
      const e = expr();
      eat(")");
      return e;
    }
    const name = ident();
    if (name === "sizeof") {
      eat("(");
      const ty = type();
      eat(")");
      return { sizeof: ty };
    }
    if (is("(")) {
      i++;
      const args = [];
      while (!is(")")) {
        args.push(expr());
        if (!is(")")) eat(",");
      }
      eat(")");
      return { call: name, args };
    }
    if (is("[")) {
      i++;
      const index = expr();
      eat("]");
      return { index: name, at: index };
    }
    return { name };
  };
  const init = () => {
    eat("{");
    const items = [];
    while (!is("}")) {
      if (is(".")) {
        i++;
        const field = ident();
        eat("=");
        items.push({ field, value: expr() });
      } else items.push({ value: expr() });
      if (!is("}")) eat(",");
    }
    eat("}");
    return { init: items };
  };
  let target = null;
  if (peek()?.id && peek(1)?.p === "=") {
    target = ident();
    i++;
  }
  let e;
  if (peek()?.id === "new") {
    i++;
    const ty = type();
    let count = null;
    if (is("[")) {
      i++;
      count = expr();
      eat("]");
    }
    e = { new: ty, count, init: is("{") ? init() : null };
  } else e = expr();
  if (i < t.length) throw new Error(`unexpected ${JSON.stringify(t[i].p ?? t[i].id ?? t[i].num)}`);
  return { target, e };
}

// ---- Running -----------------------------------------------------------------------

export class Notebook {
  constructor(m, proc) {
    this.m = m;
    this.p = proc;
    this.vars = new Map(); // name -> { num } | { addr, size, ty (id, or null: bytes), count }
    this.table = new Map(m.syscall_table().map((s) => [s.name, s]));
  }

  async reset() {
    this.vars.clear();
    await this.p.reset();
  }

  typeId(name) {
    const id = this.m.find_type(name);
    if (id === undefined) throw new Error(`no type ${name}`);
    return id;
  }

  // A value, as a number (buffers: their address).
  async value(e) {
    if (e.num !== undefined) return e.num;
    if (e.str !== undefined) {
      const bytes = new TextEncoder().encode(e.str + "\0");
      const addr = await this.p.alloc(bytes.length, 1);
      await this.p.write(addr, bytes);
      return addr;
    }
    if (e.addr !== undefined) {
      const v = this.vars.get(e.addr);
      if (!v?.addr) throw new Error(`&${e.addr}: not a buffer`);
      return v.addr;
    }
    if (e.op) {
      const [a, b] = [await this.value(e.a), await this.value(e.b)];
      return e.op === "|" ? a | b : e.op === "+" ? a + b : a - b;
    }
    if (e.sizeof) return this.m.type_info(this.typeId(e.sizeof)).size;
    if (e.index !== undefined) {
      const v = this.vars.get(e.index);
      if (!v?.addr || v.ty === null) throw new Error(`${e.index}[]: not a typed buffer`);
      const info = this.m.type_info(v.ty), i = await this.value(e.at);
      const bytes = await this.p.read(v.addr + i * info.size, info.size);
      return readInt(bytes, 0, info.size);
    }
    if (e.call === "len") {
      const v = this.vars.get(e.args[0]?.name);
      if (!v?.addr) throw new Error("len(): not a buffer");
      return v.size;
    }
    if (e.call) throw new Error(`${e.call}(): only at the top of a line`);
    if (e.name in CONSTANTS) return CONSTANTS[e.name];
    const v = this.vars.get(e.name);
    if (!v) throw new Error(`unknown name ${e.name}`);
    return v.num ?? v.addr;
  }

  // Lay out `e` (an initializer or a value) as type `ty` into `bytes` at `off`.
  async encode(e, ty, bytes, off) {
    const info = this.m.type_info(ty);
    if (info.kind === "struct" || info.kind === "union") {
      if (!e.init) throw new Error(`${info.name}: needs {…}`);
      let next = 0;
      for (const item of e.init) {
        const m = item.field ? info.members.find((m) => m.name === item.field) : info.members[next];
        if (!m) throw new Error(`${info.name}: no field ${item.field ?? `#${next + 1}`}`);
        next = info.members.indexOf(m) + 1;
        await this.encode(item.value, m.ty, bytes, off + m.offset);
      }
    } else if (info.kind === "array") {
      const elem = this.m.type_info(info.elem);
      if (e.str !== undefined && elem.kind === "char") {
        bytes.set(new TextEncoder().encode(e.str).slice(0, info.count), off);
        return;
      }
      if (!e.init) throw new Error(`${info.name}: needs {…}`);
      for (const [k, item] of e.init.entries()) await this.encode(item.value, info.elem, bytes, off + k * elem.size);
    } else {
      writeInt(bytes, off, info.size, await this.value(e));
    }
  }

  // What a buffer holds now, read from init: a string for plain bytes, else
  // decoded by its type (each element of an array).
  async show(name) {
    const v = this.vars.get(name);
    if (!v) throw new Error(`unknown name ${name}`);
    if (v.num !== undefined) return `${name} = ${v.num}`;
    const bytes = await this.p.read(v.addr, v.size);
    if (v.ty === null) {
      const end = bytes.indexOf(0) < 0 ? bytes.length : bytes.lastIndexOf(0, bytes.findLastIndex((b) => b) + 1) + 1 || bytes.length;
      const text = new TextDecoder().decode(bytes.slice(0, Math.max(0, bytes.findLastIndex((b) => b !== 0) + 1)));
      return `${name}: ${JSON.stringify(text.replace(/[\0-\x08\x0e-\x1f]/g, "·"))}` + (end ? "" : "");
    }
    const info = this.m.type_info(v.ty), n = v.count ?? 1;
    const parts = [];
    for (let k = 0; k < Math.min(n, 8); k++) {
      const o = this.m.decode_bytes(name, v.ty, bytes.slice(k * info.size, (k + 1) * info.size));
      parts.push(o.value ?? summary(o));
    }
    return `${name}: ${n > 1 || v.count ? `[${parts.join(", ")}${n > 8 ? ", …" : ""}]` : parts[0]}`;
  }

  // Run one line; { text, ok, shown: [strings], syscall: name | null }.
  async run(line) {
    if (!line.trim() || line.trim().startsWith("//")) return { text: "", ok: true, shown: [] };
    const { target, e } = parse(line);
    const result = (text, extra = {}) => ({ text, ok: true, shown: [], syscall: null, ...extra });
    if (e.new) {
      const ty = this.typeId(e.new), info = this.m.type_info(ty);
      const count = e.count ? await this.value(e.count) : null;
      const size = info.size * (count ?? 1);
      if (!size) throw new Error(`${e.new}: no size`);
      const bytes = new Uint8Array(size);
      if (e.init) {
        if (count) for (const [k, item] of e.init.init.entries()) await this.encode(item.value, ty, bytes, k * info.size);
        else await this.encode(e.init, ty, bytes, 0);
      }
      const addr = await this.p.alloc(size, 8);
      if (addr < 0) throw new Error(`alloc: ${retText(addr)}`);
      await this.p.write(addr, bytes);
      if (!target) throw new Error("new: give it a name (x = new …)");
      this.vars.set(target, { addr, size, ty, count });
      return result(`${target}: ${info.name}${count ? `[${count}]` : ""} at 0x${addr.toString(16)} (${size} bytes)`,
                    { shown: [await this.show(target)] });
    }
    if (e.call === "alloc") {
      const size = await this.value(e.args[0] ?? { num: 0 });
      const addr = await this.p.alloc(size, 8);
      if (addr < 0) throw new Error(`alloc: ${retText(addr)}`);
      if (!target) throw new Error("alloc: give it a name (x = alloc(n))");
      this.vars.set(target, { addr, size, ty: null });
      return result(`${target}: ${size} bytes at 0x${addr.toString(16)}`);
    }
    if (e.call && e.call !== "len") {
      const s = this.table.get(e.call);
      if (!s) throw new Error(`no syscall ${e.call}`);
      if (!s.built) throw new Error(`${e.call} is not built into this kernel`);
      const args = [];
      for (const a of e.args) args.push(await this.value(a));
      const ret = await this.p.syscall(s.nr, args);
      if (target) this.vars.set(target, { num: ret });
      // The buffers the call was given, as they are now.
      const touched = [...this.vars].filter(([, v]) => v.addr !== undefined && args.some((a) => a >= v.addr && a < v.addr + v.size));
      const shown = [];
      for (const [name] of touched) shown.push(await this.show(name));
      return result(`= ${retText(ret)}`, { ok: ret >= 0, shown, syscall: s.name });
    }
    if (!target && e.name) return result(await this.show(e.name));
    const v = await this.value(e);
    if (target) this.vars.set(target, { num: v });
    return result(`${target ? `${target} = ` : ""}${v}`);
  }
}

const summary = (o) => "{ " + (o.children ?? []).slice(0, 6).map((c) => `${c.name} = ${c.value ?? summary(c)}`).join(", ") +
  ((o.children?.length ?? 0) > 6 ? ", … }" : " }");

function readInt(bytes, off, size) {
  const v = new DataView(bytes.buffer, bytes.byteOffset);
  return size === 8 ? Number(v.getBigInt64(off, true)) : size === 4 ? v.getInt32(off, true)
    : size === 2 ? v.getInt16(off, true) : v.getInt8(off);
}

function writeInt(bytes, off, size, x) {
  const v = new DataView(bytes.buffer, bytes.byteOffset);
  if (size === 8) v.setBigInt64(off, BigInt(Math.trunc(x)), true);
  else if (size === 4) v.setInt32(off, x | 0, true);
  else if (size === 2) v.setInt16(off, x, true);
  else if (size === 1) v.setInt8(off, x);
}

// Example notebooks.
export const EXAMPLES = {
  "files": `dir = openat(AT_FDCWD, "/", O_RDONLY|O_DIRECTORY)
ents = alloc(256)
getdents64(dir, ents, 256)
close(dir)
mkdirat(AT_FDCWD, "/tmp", 0755)
fd = openat(AT_FDCWD, "/tmp/hello", O_WRONLY|O_CREAT|O_TRUNC, 0644)
write(fd, "hello kSEE\\n", 11)
close(fd)`,
  "read back": `fd = openat(AT_FDCWD, "/tmp/hello", O_RDONLY)
buf = alloc(64)
n = read(fd, buf, 64)
close(fd)`,
  "writev": `iov = new struct iovec[2] {{"hello ", 6}, {"from writev\\n", 12}}
writev(1, iov, 2)`,
  "pipe": `fds = new int[2]
pipe2(fds, 0)
write(fds[1], "through a pipe", 14)
buf = alloc(32)
read(fds[0], buf, 32)`,
  "uname": `u = new struct new_utsname
uname(&u)`,
  "sleep": `ts = new struct __kernel_timespec {0, 20000000}
clock_nanosleep_time64(CLOCK_MONOTONIC, 0, &ts, NULL)`,
};
