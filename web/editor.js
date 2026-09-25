// Monaco (the editor VS Code is built on; monaco/ in the site, see
// crates/ksee-cli/src/build.rs), loaded once, for the source views.
let loading = null;

export function loadMonaco() {
  loading ??= new Promise((resolve, reject) => {
    const base = new URL("./monaco", location).href;
    const script = document.createElement("script");
    script.src = `${base}/loader.js`;
    script.onerror = () => reject(new Error("no Monaco (monaco/): run ./ksee.sh build"));
    script.onload = () => {
      globalThis.require.config({ paths: { vs: base } });
      globalThis.require(["vs/editor/editor.main"], () => {
        const monaco = globalThis.monaco;
        const dark = matchMedia("(prefers-color-scheme: dark)").matches;
        monaco.editor.setTheme(dark ? "vs-dark" : "vs");
        resolve(monaco);
      }, reject);
    };
    document.head.append(script);
  });
  return loading;
}

// Options shared by the views: read-only C, the page's font.
export const readOnly = {
  readOnly: true,
  language: "c",
  automaticLayout: true,
  scrollBeyondLastLine: false,
  fontSize: 12,
  fontFamily: getComputedStyle(document.documentElement).getPropertyValue("--mono").trim() || "monospace",
  renderLineHighlight: "none",
  domReadOnly: true,
  stickyScroll: { enabled: true },
};
