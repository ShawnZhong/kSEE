// Small DOM helpers shared by the page's modules.
export const $ = (id) => document.getElementById(id);

// el("div", { className: "x", dataset: { a: 1 } }, child, ...)
export function el(tag, props = {}, ...kids) {
  const { dataset, style, ...rest } = props;
  const e = Object.assign(document.createElement(tag), rest);
  if (dataset) Object.assign(e.dataset, dataset);
  if (style) e.style.cssText = style;
  e.append(...kids.filter((k) => k !== null && k !== undefined));
  return e;
}

export function svg(tag, attrs = {}, text) {
  const e = document.createElementNS("http://www.w3.org/2000/svg", tag);
  for (const [k, v] of Object.entries(attrs)) e.setAttribute(k, v);
  if (text !== undefined) e.textContent = text;
  return e;
}

export const hex = (n) => "0x" + (Number(n) >>> 0).toString(16);
export const duration = (ns) =>
  ns == null ? "running" : ns < 1e6 ? `${(ns / 1e3).toFixed(0)} µs` : `${(ns / 1e6).toFixed(1)} ms`;
export const taskName = (t) => (t?.comm ? `${t.comm}(${t.pid})` : "?");

const ERRNO = {
  1: "EPERM", 2: "ENOENT", 3: "ESRCH", 4: "EINTR", 5: "EIO", 9: "EBADF", 11: "EAGAIN", 12: "ENOMEM",
  13: "EACCES", 14: "EFAULT", 16: "EBUSY", 17: "EEXIST", 20: "ENOTDIR", 21: "EISDIR", 22: "EINVAL",
  24: "EMFILE", 25: "ENOTTY", 28: "ENOSPC", 36: "ENAMETOOLONG", 38: "ENOSYS", 39: "ENOTEMPTY",
};
export const retText = (r) => (r < 0 && ERRNO[-r] ? `${r} (${ERRNO[-r]})` : `${r}`);
