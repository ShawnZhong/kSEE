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
        monaco.languages.setMonarchTokensProvider("c", C);
        const dark = matchMedia("(prefers-color-scheme: dark)").matches;
        monaco.editor.defineTheme("ksee", { base: dark ? "vs-dark" : "vs", inherit: true, rules: dark ? DARK : LIGHT, colors: {} });
        monaco.editor.setTheme("ksee");
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

// C as the kernel writes it (Monaco's own "c" is its C++ tokenizer, which
// tells neither calls nor macros from other names): keywords, types (the
// kernel's usual ones too), a name before "(" (a call, or a definition),
// an all-capitals name (a macro, or a constant), preprocessor lines.
const C = {
  keywords: ("auto break case const continue default do else enum extern for goto if inline register restrict return " +
    "sizeof static struct switch typedef typeof union volatile while asm __attribute__ __always_inline noinline " +
    "__init __user __rcu __iomem __must_check likely unlikely true false NULL").split(" "),
  types: ("void char short int long float double signed unsigned bool _Bool u8 u16 u32 u64 s8 s16 s32 s64 __u8 __u16 " +
    "__u32 __u64 __s32 __s64 __le16 __le32 __le64 __be16 __be32 size_t ssize_t loff_t off_t pid_t uid_t gid_t umode_t " +
    "dev_t sector_t blk_opf_t gfp_t fmode_t vm_fault_t atomic_t atomic64_t refcount_t spinlock_t raw_spinlock_t " +
    "ktime_t time64_t uintptr_t phys_addr_t pgoff_t").split(" "),
  tokenizer: {
    root: [
      { include: "@space" },
      [/^\s*#\s*\w+/, "keyword.directive"],
      [/[A-Za-z_]\w*(?=\s*\()/, { cases: { "@keywords": "keyword", "@types": "type", "[A-Z_][A-Z0-9_]+": "macro", "@default": "function" } }],
      [/[A-Za-z_]\w*/, { cases: { "@keywords": "keyword", "@types": "type", "[A-Z_][A-Z0-9_]+": "macro", "@default": "identifier" } }],
      [/0[xX][0-9a-fA-F]+[uUlL]*|\d+[uUlL]*/, "number"],
      [/"/, "string", "@string"],
      [/'(?:[^\\']|\\.)'/, "string"],
      [/[{}()\[\]]/, "@brackets"],
      [/[;,.]/, "delimiter"],
      [/[=><!~?:&|+\-*\/^%]+/, "operator"],
    ],
    space: [[/[ \t\r\n]+/, ""], [/\/\*/, "comment", "@comment"], [/\/\/.*$/, "comment"]],
    comment: [[/[^/*]+/, "comment"], [/\*\//, "comment", "@pop"], [/[/*]/, "comment"]],
    string: [[/[^\\"]+/, "string"], [/\\./, "string.escape"], [/"/, "string", "@pop"]],
  },
};
// (VS Code's colors for them.)
const LIGHT = [{ token: "function", foreground: "795E26" }, { token: "macro", foreground: "0070C1" }, { token: "type", foreground: "267F99" }];
const DARK = [{ token: "function", foreground: "DCDCAA" }, { token: "macro", foreground: "4FC1FF" }, { token: "type", foreground: "4EC9B0" }];
