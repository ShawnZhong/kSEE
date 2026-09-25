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

export const taskName = (t) => (t?.comm ? `${t.comm}(${t.pid})` : "?");
