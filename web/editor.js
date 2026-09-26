// CodeMirror (codemirror.js: npm's @codemirror packages, bundled by the
// build: see crates/ksee-cli/src/build.rs), set up for the source views:
// read-only C, highlighted, in the page's colors.
import * as CM from "./codemirror.js";

export { CM };

const dark = matchMedia("(prefers-color-scheme: dark)").matches;

// The extensions every view has: read-only C, line numbers, the theme.
export const base = [
  CM.EditorState.readOnly.of(true),
  CM.EditorView.editable.of(false),
  CM.lineNumbers(),
  CM.cpp(),
  CM.syntaxHighlighting(dark ? CM.oneDarkHighlightStyle : CM.defaultHighlightStyle, { fallback: true }),
  CM.EditorView.theme({
    "&": { height: "100%", fontSize: "12px", backgroundColor: "var(--panel)", color: "var(--fg)" },
    ".cm-scroller": { fontFamily: "var(--mono)", lineHeight: "1.45" },
    ".cm-gutters": { backgroundColor: "var(--panel)", color: "var(--dim)", borderRight: "1px solid var(--line)" },
    ".cm-content": { padding: "0" },
  }, { dark }),
];
