// Syscalls from typed values, made in /init (web/ksee.js: process). A call
// is a syscall of the kernel's table (its parameters and their C types come
// from DWARF) with a value per parameter:
//   numbers   "12", "0x1f", "0644", "O_WRONLY|O_CREAT", or "auto" (a length:
//             that of the buffer before it)
//   fds       a number, "latest" (the newest fd a call returned), or the
//             label of one ("/tmp/hello", "pipe write end")
//   in        text (\n, \t, \0, \\ escapes); for a struct: [{ field: value }]
//             (an array's elements), values as above, text for pointers
//   out       a size in bytes ("" for the type's size)
// Session.call puts the buffers in /init's memory, makes the call, reads
// the outputs back, and describes it all as strace would.

export const CONSTANTS = {
  AT_FDCWD: -100, AT_REMOVEDIR: 0x200, AT_SYMLINK_NOFOLLOW: 0x100, AT_EMPTY_PATH: 0x1000,
  O_RDONLY: 0, O_WRONLY: 1, O_RDWR: 2, O_CREAT: 0o100, O_EXCL: 0o200, O_NOCTTY: 0o400, O_TRUNC: 0o1000,
  O_APPEND: 0o2000, O_NONBLOCK: 0o4000, O_DIRECTORY: 0o200000, O_NOFOLLOW: 0o400000, O_CLOEXEC: 0o2000000,
  SEEK_SET: 0, SEEK_CUR: 1, SEEK_END: 2, CLOCK_REALTIME: 0, CLOCK_MONOTONIC: 1, CLOCK_BOOTTIME: 7,
  F_OK: 0, R_OK: 4, W_OK: 2, X_OK: 1, NULL: 0,
};

// Choices and flags offered for parameters, by "syscall.param" or "param".
const OPEN_FLAGS = ["O_RDONLY", "O_WRONLY", "O_RDWR", "O_CREAT", "O_EXCL", "O_TRUNC", "O_APPEND", "O_DIRECTORY", "O_NONBLOCK", "O_CLOEXEC"];
export const OPTIONS = {
  "openat.flags": { flags: OPEN_FLAGS }, "open.flags": { flags: OPEN_FLAGS },
  "pipe2.flags": { flags: ["O_CLOEXEC", "O_NONBLOCK"] }, "dup3.flags": { flags: ["O_CLOEXEC"] },
  "unlinkat.flag": { flags: ["AT_REMOVEDIR"] },
  which_clock: { choices: ["CLOCK_REALTIME", "CLOCK_MONOTONIC", "CLOCK_BOOTTIME"] },
  whence: { choices: ["SEEK_SET", "SEEK_CUR", "SEEK_END"] },
  dfd: { choices: ["AT_FDCWD"], fd: true }, olddfd: { choices: ["AT_FDCWD"], fd: true }, newdfd: { choices: ["AT_FDCWD"], fd: true },
};
const FD_PARAMS = /^(fd|oldfd|newfd|fildes|out_fd|in_fd|dirfd|epfd)$/;
const LENGTH_PARAMS = /^(count|len|size|nbytes|bufsiz|vlen|iovcnt|nr|length)$/;
// Out buffers of this many elements (else one of the type, or bytes).
const OUT_COUNTS = { "pipe2.fildes": 2, "pipe.fildes": 2, "socketpair.usockvec": 2 };
// Syscalls whose result is a new fd.
const RETURNS_FD = new Set(["openat", "open", "openat2", "creat", "dup", "dup2", "dup3", "eventfd2", "memfd_create",
  "epoll_create1", "timerfd_create", "signalfd4", "socket", "inotify_init1", "pidfd_open", "fcntl"]);

const ERRNO = {
  1: "EPERM", 2: "ENOENT", 3: "ESRCH", 4: "EINTR", 5: "EIO", 9: "EBADF", 11: "EAGAIN", 12: "ENOMEM",
  13: "EACCES", 14: "EFAULT", 16: "EBUSY", 17: "EEXIST", 20: "ENOTDIR", 21: "EISDIR", 22: "EINVAL",
  24: "EMFILE", 25: "ENOTTY", 28: "ENOSPC", 32: "EPIPE", 36: "ENAMETOOLONG", 38: "ENOSYS", 39: "ENOTEMPTY",
};
export const retText = (r) => (r < 0 && ERRNO[-r] ? `${r} ${ERRNO[-r]}` : `${r}`);

const unescape = (s) => s.replace(/\\(n|t|0|\\)/g, (_, c) => ({ n: "\n", t: "\t", 0: "\0", "\\": "\\" })[c]);
const quote = (s, max = 32) => JSON.stringify(s.length > max ? s.slice(0, max) : s) + (s.length > max ? "..." : "");

// A number: constants joined by |, octal (0644), hex, decimal.
export function number(text) {
  let v = 0;
  for (const part of String(text).split("|").map((s) => s.trim()).filter(Boolean)) {
    let n;
    if (part in CONSTANTS) n = CONSTANTS[part];
    else if (/^-?0[0-7]+$/.test(part)) n = parseInt(part, 8);
    else n = Number(part);
    if (!Number.isFinite(n)) throw new Error(`not a number: ${part}`);
    v |= n;
  }
  return v;
}

function writeInt(bytes, off, size, x) {
  const v = new DataView(bytes.buffer, bytes.byteOffset);
  if (size === 8) v.setBigInt64(off, BigInt(Math.trunc(x)), true);
  else if (size === 4) v.setInt32(off, x | 0, true);
  else if (size === 2) v.setInt16(off, x, true);
  else if (size === 1) v.setInt8(off, x);
}
function readInt(bytes, off, size) {
  const v = new DataView(bytes.buffer, bytes.byteOffset);
  return size === 8 ? Number(v.getBigInt64(off, true)) : size === 4 ? v.getInt32(off, true)
    : size === 2 ? v.getInt16(off, true) : v.getInt8(off);
}
const text = (bytes) => new TextDecoder().decode(bytes);

export class Session {
  constructor(m, proc) {
    this.m = m;
    this.p = proc;
    this.table = m.syscall_table().filter((s) => s.built && s.params);
    this.byName = new Map(this.table.map((s) => [s.name, s]));
    this.fds = []; // [{ fd, label }], oldest first
  }

  async reset() {
    this.fds = [];
    await this.p.reset();
  }

  // ---- What a card shows ----

  // Each parameter of `s`: { name, type, kind (value | fd | in | struct |
  // out), flags?, choices?, fields? (a struct's), value (the default) }.
  fields(s) {
    let lastBuffer = null;
    return s.params.map((p) => {
      const opt = OPTIONS[`${s.name}.${p.name}`] ?? OPTIONS[p.name] ?? {};
      const f = { name: p.name, type: p.type, ...opt };
      if (p.kind === "in" && p.aggregate) {
        f.kind = "struct";
        f.struct = this.m.type_info(p.target).name;
        f.fields = this.layout(p.target);
        f.value = [Object.fromEntries(f.fields.map((x) => [x.name, x.auto ? "auto" : x.text ? "" : "0"]))];
      } else if (p.kind === "in") {
        f.kind = "in";
        f.value = /name|path|file/.test(p.name) ? "/" : "hello kSEE\\n";
        f.path = /name|path|file/.test(p.name);
      } else if (p.kind === "out") {
        f.kind = "out";
        f.value = p.aggregate && !this.isRecords(s, p) ? "" : String(this.outSize(s, p) ?? 256);
      } else if (opt.fd || FD_PARAMS.test(p.name)) {
        f.kind = "fd";
        f.value = opt.choices?.[0] ?? (s.name === "write" || s.name === "writev" ? "1" : "latest");
      } else {
        f.kind = "value";
        f.value = LENGTH_PARAMS.test(p.name) && lastBuffer ? "auto" : /mode/.test(p.name) ? "0644"
          : opt.choices?.[0] ?? (opt.flags ? opt.flags[0] : "0");
      }
      if (["in", "struct", "out"].includes(f.kind)) lastBuffer = f;
      return f;
    });
  }

  // A struct's fields, flattened: { name, offset, size, type, text (a
  // pointer or char array: text), auto (a length after a pointer) }.
  layout(ty) {
    const fields = this.m.layout(ty);
    return fields.map((f, i) => ({
      ...f,
      text: f.type.endsWith("*") || /^char\[/.test(f.type),
      auto: /len$/.test(f.name) && i > 0 && fields[i - 1].type.endsWith("*"),
    }));
  }

  // A byte buffer of records (getdents64's struct linux_dirent64 *).
  isRecords(s, p) {
    const i = s.params.indexOf(p);
    return p.aggregate && LENGTH_PARAMS.test(s.params[i + 1]?.name ?? "");
  }
  outSize(s, p) {
    const n = OUT_COUNTS[`${s.name}.${p.name}`];
    if (n) return n * p.size;
    if (this.isRecords(s, p)) return 1024;
    return p.aggregate ? p.size : null;
  }

  // An fd value: a number, "latest", or a label.
  fd(v) {
    v = String(v).trim();
    if (v === "latest") {
      if (!this.fds.length) throw new Error("no fd yet: open something first");
      return this.fds.at(-1).fd;
    }
    const known = this.fds.findLast((f) => f.label === v);
    return known ? known.fd : number(v.split(/\s/)[0]);
  }
  label(fd) {
    return this.fds.findLast((f) => f.fd === fd)?.label ?? ({ 0: "stdin", 1: "stdout", 2: "stderr" })[fd];
  }

  // ---- Making the call ----

  // Text in /init: its address and byte length (without the NUL).
  async place(str) {
    const bytes = new TextEncoder().encode(unescape(str) + "\0");
    const addr = await this.p.alloc(bytes.length, 1);
    if (addr < 0) throw new Error(`alloc: ${retText(addr)}`);
    await this.p.write(addr, bytes);
    return { addr, len: bytes.length - 1 };
  }

  // Make syscall `s` (a table entry, or its name) with `values` (by
  // parameter name; missing ones get the defaults). Returns { ret, line
  // (strace's), outputs: [{ name, text, object?, list? }] }.
  async call(s, values = {}) {
    if (typeof s === "string") s = this.byName.get(s) ?? (() => { throw new Error(`no syscall ${s}`); })();
    const fields = this.fields(s);
    const args = [], shown = [], outs = [];
    let lastLen = 0;
    for (const [i, f] of fields.entries()) {
      const p = s.params[i], v = values[f.name] ?? f.value;
      if (f.kind === "in") {
        if (v === "" || v === "NULL") {
          args.push(0);
          shown.push("NULL");
          continue;
        }
        const { addr, len } = await this.place(v);
        args.push(addr);
        shown.push(quote(unescape(v)));
        lastLen = len;
      } else if (f.kind === "struct") {
        const { addr, parts } = await this.structs(p.target, f.fields, v);
        args.push(addr);
        shown.push(parts.length === 1 ? parts[0] : `[${parts.join(", ")}]`);
        lastLen = parts.length;
      } else if (f.kind === "out") {
        const size = v === "" ? p.size : number(v);
        const addr = await this.p.alloc(Math.max(1, size), 8);
        if (addr < 0) throw new Error(`alloc: ${retText(addr)}`);
        args.push(addr);
        shown.push(null); // filled in after the call
        outs.push({ i, p, addr, size });
        lastLen = size;
      } else if (f.kind === "fd") {
        const fd = this.fd(v);
        args.push(fd);
        const l = this.label(fd);
        shown.push(l && fd >= 0 ? `${fd}<${l}>` : String(v).trim() === "AT_FDCWD" ? "AT_FDCWD" : String(fd));
      } else {
        const n = String(v).trim() === "auto" ? lastLen : number(v);
        args.push(n);
        shown.push(/[A-Z_]|^0\d/.test(String(v).trim()) ? String(v).trim() : String(n));
      }
    }
    const ret = await this.p.syscall(s.nr, args);
    const outputs = [];
    for (const o of outs) {
      const out = await this.output(s, o, ret);
      shown[o.i] = out.brief;
      outputs.push({ name: o.p.name, ...out });
    }
    this.track(s, fields, values, args, ret, outputs);
    return { ret, line: `${s.name}(${shown.join(", ")}) = ${retText(ret)}`, outputs, name: s.name };
  }

  // Elements of struct type `ty` (fields as from layout) in /init.
  async structs(ty, fields, elems) {
    const size = this.m.type_info(ty).size, list = Array.isArray(elems) ? elems : [elems];
    const bytes = new Uint8Array(size * list.length), parts = [];
    for (const [k, e] of list.entries()) {
      const shown = [];
      let lastLen = 0;
      for (const f of fields) {
        const v = String(e[f.name] ?? (f.auto ? "auto" : "0"));
        const off = k * size + f.offset;
        if (f.text && /^char\[/.test(f.type)) {
          bytes.set(new TextEncoder().encode(unescape(v)).slice(0, f.size), off);
          shown.push(`${f.name}=${quote(unescape(v))}`);
        } else if (f.text && v !== "" && !/^(0x[0-9a-f]+|\d+|NULL)$/i.test(v.trim())) {
          const { addr, len } = await this.place(v);
          writeInt(bytes, off, 4, addr);
          shown.push(`${f.name}=${quote(unescape(v))}`);
          lastLen = len;
        } else {
          const n = v.trim() === "auto" ? lastLen : number(v || "0");
          writeInt(bytes, off, f.size, n);
          shown.push(`${f.name}=${n}`);
        }
      }
      parts.push(`{${shown.join(", ")}}`);
    }
    const addr = await this.p.alloc(bytes.length, 8);
    if (addr < 0) throw new Error(`alloc: ${retText(addr)}`);
    await this.p.write(addr, bytes);
    return { addr, parts };
  }

  // What an out buffer holds after the call: { brief (for the strace line),
  // text?, object? (decoded by type), list? (records, elements) }.
  async output(s, { p, addr, size }, ret) {
    const bytes = await this.p.read(addr, size);
    if (ret < 0) return { brief: `0x${addr.toString(16)}` };
    if (this.isRecords(s, p) && p.aggregate) {
      const list = dirents(bytes.slice(0, Math.min(ret, size)));
      return { brief: `[${list.slice(0, 4).map((d) => `{d_name=${quote(d.name)}}`).join(", ")}${list.length > 4 ? ", ..." : ""}]`,
               list: list.map((d) => `${d.name}  (inode ${d.ino}, type ${d.type})`) };
    }
    if (p.aggregate) {
      const object = this.m.decode_bytes(p.name, p.target, bytes);
      return { brief: brief(object), object };
    }
    const info = p.target ? this.m.type_info(p.target) : null;
    if (info && info.kind === "int" && info.size > 1) {
      const list = [];
      for (let o = 0; o + info.size <= size; o += info.size) list.push(readInt(bytes, o, info.size));
      return { brief: `[${list.join(", ")}]`, list: list.map(String) };
    }
    const n = ret > 0 && ret <= size ? ret : bytes.findLastIndex((b) => b !== 0) + 1;
    const t = text(bytes.slice(0, n));
    return { brief: quote(t), text: t };
  }

  // Remember the fds a call made (and forget the ones it closed).
  track(s, fields, values, args, ret, outputs) {
    if (ret < 0) return;
    if (s.name === "close") this.fds = this.fds.filter((f) => f.fd !== args[0]);
    if (RETURNS_FD.has(s.name) && !(s.name === "fcntl")) {
      const path = fields.findIndex((f) => f.kind === "in" && f.path);
      const label = path >= 0 ? unescape(values[fields[path].name] ?? fields[path].value) : `${s.name} result`;
      this.fds.push({ fd: ret, label });
    }
    if (s.name === "pipe2" || s.name === "pipe") {
      const [r, w] = outputs[0]?.list?.map(Number) ?? [];
      this.fds.push({ fd: r, label: "pipe read end" }, { fd: w, label: "pipe write end" });
    }
  }
}

// struct linux_dirent64 { u64 d_ino; s64 d_off; u16 d_reclen; u8 d_type; char d_name[]; }
function dirents(bytes) {
  const v = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength), list = [];
  for (let o = 0; o + 19 <= bytes.length;) {
    const len = v.getUint16(o + 16, true);
    if (!len) break;
    const name = bytes.slice(o + 19, o + len);
    list.push({ ino: Number(v.getBigUint64(o, true)), type: v.getUint8(o + 18), name: text(name.slice(0, name.indexOf(0) < 0 ? name.length : name.indexOf(0))) });
    o += len;
  }
  return list;
}

// "{sysname="Linux", nodename="(none)", ...}"
function brief(o, max = 3) {
  const kids = o.children ?? [];
  return "{" + kids.slice(0, max).map((c) => `${c.name}=${c.value ?? brief(c, 1)}`).join(", ") + (kids.length > max ? ", ..." : "") + "}";
}

// Example sessions: [syscall, values] each.
export const SCENARIOS = {
  "write a file": [["mkdirat", { dfd: "AT_FDCWD", pathname: "/tmp", mode: "0755" }],
                   ["openat", { dfd: "AT_FDCWD", filename: "/tmp/hello", flags: "O_WRONLY|O_CREAT|O_TRUNC", mode: "0644" }],
                   ["write", { fd: "latest", buf: "hello kSEE\\n", count: "auto" }],
                   ["close", { fd: "latest" }]],
  "read it back": [["openat", { dfd: "AT_FDCWD", filename: "/tmp/hello", flags: "O_RDONLY" }],
                   ["read", { fd: "latest", buf: "64", count: "auto" }],
                   ["close", { fd: "latest" }]],
  "ls /": [["openat", { dfd: "AT_FDCWD", filename: "/", flags: "O_RDONLY|O_DIRECTORY" }],
           ["getdents64", { fd: "latest", count: "auto" }],
           ["close", { fd: "latest" }]],
  "pipe": [["pipe2", { flags: "0" }],
           ["write", { fd: "pipe write end", buf: "through a pipe", count: "auto" }],
           ["read", { fd: "pipe read end", buf: "32", count: "auto" }]],
  "writev": [["writev", { fd: "1", vec: [{ iov_base: "hello ", iov_len: "auto" }, { iov_base: "from writev\\n", iov_len: "auto" }], vlen: "auto" }]],
  "uname": [["uname", {}]],
  "sleep 20 ms": [["clock_nanosleep_time64", { which_clock: "CLOCK_MONOTONIC", flags: "0", rqtp: [{ tv_sec: "0", tv_nsec: "20000000" }], rmtp: "" }]],
  "getpid": [["getpid", {}]],
};
