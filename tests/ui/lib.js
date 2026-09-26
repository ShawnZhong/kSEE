// Helpers for the page scripts (tests/ui.js evaluates this, then one of them).
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const $ = (id) => document.getElementById(id);
const until = async (f, n = 100) => { for (let i = 0; i < n && !f(); i++) await sleep(100); };
// The program's calls (not the line that adds one); a call's line: its call, in its editor, then its result.
const lis = () => [...document.querySelectorAll("ol.steps > li:not(.add)")];
const line = (li) => (li.querySelector("textarea")?.value ?? "") + " " + li.querySelector(".line").innerText.replace(/\s+/g, " ").trim();
