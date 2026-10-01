// Helpers for the page scripts (tests/ui.js evaluates this, then one of them).
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const until = async (f, n = 100) => { for (let i = 0; i < n && !f(); i++) await sleep(100); return f(); };
const $$ = (s) => [...document.querySelectorAll(s)];
// The program's lines (not the one that adds a line): their text and what each did.
const cells = () => $$(".cells .cell:not(.add)").map((c) => ({ text: c.querySelector("textarea").value, result: c.querySelector(".result")?.textContent ?? "" }));
const done = (n) => cells().length >= n && cells().slice(0, n).every((c) => c.result && c.result !== "…");
// Where the cursor is, as the transport says: "func file:line".
const where = () => document.querySelector(".transport .mono")?.textContent ?? "";
const button = (label) => $$(".transport button").find((b) => b.textContent === label);
// A line's text changed and Enter pressed (a cell's, or the one that adds a line).
const write = (ta, text) => {
  ta.focus();
  ta.value = text;
  ta.setSelectionRange(text.length, text.length);
  ta.dispatchEvent(new Event("input", { bubbles: true }));
  ta.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
};
const fail = (what, r) => { throw new Error(`${what}: ${JSON.stringify(r)}`); };
