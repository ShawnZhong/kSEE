//! The code and type tables: the trace plugin's (its ksee section)
//! and the pack's (the types' text), parsed by the pack into the page's
//! `tables` file, which the page reads in place (see flat).

use alloc::collections::BTreeMap;

use crate::arch;
use crate::flat::{At, Lists, Pod, Reader, Str, Text, Writer, Zeroable};
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// A source location; `file` indexes the files (Tables::file).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Pod, Zeroable)]
pub struct Loc {
    pub file: u32,
    pub line: u32,
}

/// An instrumented function (a view of the tables', Tables::function).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Function {
    pub name: &'static str,
    /// Where it is defined.
    pub loc: Loc,
    /// Which of the source's parameters call records carry (a bit each);
    /// the others were optimized away.
    pub known: u32,
    /// Which take two words (a bit each: 64 bits wide); the others one.
    pub wide: u32,
    /// Only its call and returns are recorded, not its blocks (it loops,
    /// writes nothing the host undoes, calls nothing: see the plugin).
    pub sealed: bool,
}

/// A function, as the tables have it: by id; its blocks and its variables
/// from `blocks` and `vars` on (each numbered from 0, to the next
/// function's).
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct FunctionAt {
    id: u32,
    name: Str,
    loc: Loc,
    known: u32,
    wide: u32,
    sealed: u32,
    blocks: u32,
    vars: u32,
}

/// A named variable kept in a wasm local, whose values the recording has
/// (Record::Var): variable `name` of function `function` (the one whose
/// records they are, or one inlined into it; `of`: its id), parameter
/// `param` of it (from 1; 0: a local). `arg`: if nonzero, it is the
/// call's parameter `arg` throughout (an inlined call's parameter passed
/// one of the call's own), with no records.
/// (A view of the tables', Tables::var.)
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Var {
    pub of: u32,
    pub function: &'static str,
    pub name: &'static str,
    pub param: u32,
    pub arg: u32,
}

/// A variable, as the tables have it (none: no name; see FunctionAt).
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct VarAt {
    of: u32,
    function: Str,
    name: Str,
    param: u32,
    arg: u32,
}

/// A source line a block runs, and if it is code inlined into the block's
/// function, the functions it is inlined from (Step::inlined).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Pod, Zeroable)]
pub struct Step {
    pub loc: Loc,
    /// An inline chain (Tables::chain), u32::MAX for none.
    chain: u32,
}

impl Step {
    pub fn new(loc: Loc, inlined: Option<u32>) -> Step {
        Step { loc, chain: inlined.unwrap_or(u32::MAX) }
    }

    /// The inline chain it is in (Tables::chain), if it is inlined.
    pub fn inlined(&self) -> Option<u32> {
        (self.chain != u32::MAX).then_some(self.chain)
    }
}

/// An inlined function (Tables::inline_name) and where it is inlined: a
/// line of the next one out (of the block's function, for the last).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Pod, Zeroable)]
pub struct Site {
    pub name: u32,
    pub loc: Loc,
}

use crate::types::Types;

/// A basic block: the source lines it runs, in order, in stretches between
/// the calls it makes (a stretch runs at the block's record, the next at
/// each return into it). A Store's, Compare's or Var's `line` indexes
/// `steps`. (A view of the Tables' steps and cuts: Tables::block.)
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Block<'a> {
    pub steps: &'a [Step],
    /// Where each stretch begins in `steps` (the first at 0).
    cuts: &'a [u32],
}

/// Where a block's steps and cuts begin in the Tables' (each to where the
/// next block's do: see FunctionAt).
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct BlockAt {
    steps: u32,
    cuts: u32,
}

impl<'a> Block<'a> {
    /// Where it begins: its first line (the function's signature, for its
    /// entry block).
    pub fn loc(&self) -> Loc {
        self.steps.first().map_or(Loc { file: u32::MAX, line: 0 }, |s| s.loc)
    }

    /// Stretch `seg` (none past the last).
    pub fn stretch(&self, seg: u32) -> &'a [Step] {
        let steps: &'a [Step] = self.steps;
        &steps[self.start(seg)..self.start(seg + 1)]
    }

    /// The last line it has run by stretch `seg` (seg's, or an earlier
    /// one's if seg has none), if any.
    pub fn last_by(&self, seg: u32) -> Option<Step> {
        self.steps[..self.start(seg + 1)].last().copied()
    }

    /// Where stretch `seg` begins in `steps`.
    pub fn start(&self, seg: u32) -> usize {
        self.cuts.get(seg as usize).map_or(self.steps.len(), |&a| a as usize)
    }

    /// The stretch step `index` is in (of stretches beginning at one
    /// index, the empty ones between two calls, the last), if it has one.
    pub fn stretch_of(&self, index: u32) -> Option<u32> {
        ((index as usize) < self.steps.len()).then(|| self.cuts.partition_point(|&a| a <= index) as u32 - 1)
    }
}

/// The tables: flat (see flat), the file's as it is, or as parse made
/// them.
#[derive(Debug, Default)]
pub struct Tables {
    /// The names' text.
    text: &'static str,
    /// By id; each its blocks and variables, in its order.
    functions: &'static [FunctionAt],
    /// Where each block's steps and cuts are in `steps` and `cuts`.
    blocks: &'static [BlockAt],
    steps: &'static [Step],
    cuts: &'static [u32],
    /// Names of inlined functions.
    inline_names: &'static [Str],
    /// Inline chains (see Step), by index: innermost first.
    chains: (&'static [At], &'static [Site]),
    /// Source file paths.
    files: &'static [Str],
    /// Each function's variables, as its Var records number them.
    vars: &'static [VarAt],
    /// The kernel's types.
    pub types: Types,
}

/// Values numbered as parsing meets them, each once (files, inlined
/// functions, inline chains).
struct Index<K> {
    ids: BTreeMap<K, u32>,
    list: Vec<K>,
}

impl<K> Default for Index<K> {
    fn default() -> Self {
        Index { ids: BTreeMap::new(), list: Vec::new() }
    }
}

impl<K: Ord + Clone> Index<K> {
    fn get(&mut self, k: K) -> u32 {
        if let Some(&id) = self.ids.get(&k) {
            return id;
        }
        self.list.push(k.clone());
        self.ids.insert(k, self.list.len() as u32 - 1);
        self.list.len() as u32 - 1
    }
}

/// A compilation unit's own numbering: its files (P lines) and its inline
/// sites (I lines: the function inlined, and where).
#[derive(Default)]
struct Unit {
    files: BTreeMap<u32, u32>,
    sites: BTreeMap<u32, Site>,
}

impl Unit {
    /// Parse "k:line" (k: a P line's).
    fn loc(&self, s: &str) -> Loc {
        let (k, line) = s.split_once(':').unwrap_or((s, ""));
        let file = k.parse().ok().and_then(|k| self.files.get(&k).copied()).unwrap_or(u32::MAX);
        Loc { file, line: line.parse().unwrap_or(0) }
    }
}

impl Tables {
    /// The tables as the page loads them (the pack's: parsed once, at
    /// build time; see flat).
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut w = Writer::default();
        w.bytes(self.text.as_bytes());
        w.slice(self.functions);
        (w.slice(self.blocks), w.slice(self.steps), w.slice(self.cuts));
        (w.slice(self.inline_names), w.slice(self.chains.0), w.slice(self.chains.1));
        (w.slice(self.files), w.slice(self.vars));
        self.types.write(&mut w);
        w.done()
    }

    /// The tables of [`Tables::to_bytes`]' bytes, read where they are
    /// (8-aligned: flat::kept), kept as the tables are.
    pub fn from_bytes(bytes: &'static [u8]) -> Option<Tables> {
        let mut r = Reader::new(bytes);
        Some(Tables {
            text: r.text()?,
            functions: r.slice()?,
            blocks: r.slice()?,
            steps: r.slice()?,
            cuts: r.slice()?,
            inline_names: r.slice()?,
            chains: (r.slice()?, r.slice()?),
            files: r.slice()?,
            vars: r.slice()?,
            types: Types::read(&mut r)?,
        })
    }

    /// Parse the `ksee` section and the types' text. Lines are
    /// tab-separated and may repeat (one copy per compilation unit).
    pub fn parse(code: &str, types: &str) -> Tables {
        let types = Types::parse(types);
        let mut text = Text::default();
        let (mut files, mut names) = (Index::<String>::default(), Index::<String>::default());
        let mut chains = Index::<Vec<Site>>::default();
        let mut unit = Unit::default();
        let mut functions = BTreeMap::new();
        let mut vars: BTreeMap<(u32, u32), VarAt> = BTreeMap::new();
        // (Each block's steps and cuts, by (function, n).)
        let mut blocks: BTreeMap<(u32, u32), (Vec<Step>, Vec<u32>)> = BTreeMap::new();
        for line in code.lines() {
            let cols: Vec<&str> = line.split('\t').collect();
            match cols.as_slice() {
                ["U"] => unit = Unit::default(),
                ["P", k, path] => {
                    if let Ok(k) = k.parse() {
                        unit.files.insert(k, files.get(path.to_string()));
                    }
                }
                ["I", k, name, site] => {
                    if let Ok(k) = k.parse() {
                        unit.sites.insert(k, Site { name: names.get(name.to_string()), loc: unit.loc(site) });
                    }
                }
                ["V", id, k, of, function, name, param, arg] => {
                    if let (Ok(func), Ok(k), Ok(of)) = (id.parse(), k.parse(), of.parse()) && !name.starts_with(crate::arch::MADE_UP) {
                        let (param, arg) = (param.parse().unwrap_or(0), arg.parse().unwrap_or(0));
                        vars.insert((func, k), VarAt { of, function: text.name(function), name: text.name(name), param, arg });
                    }
                }
                ["F", id, name, location, known, rest @ ..] => {
                    let Ok(id) = id.parse() else { continue };
                    let loc = unit.loc(location);
                    let sealed = (rest.first() == Some(&"1")) as u32;
                    let wide = rest.get(1).and_then(|w| w.parse().ok()).unwrap_or(0);
                    functions.insert(id, FunctionAt { id, name: text.name(name), loc, known: known.parse().unwrap_or(0), wide, sealed, blocks: 0, vars: 0 });
                }
                ["L", id, n, items] => {
                    if let (Ok(func), Ok(n)) = (id.parse(), n.parse()) {
                        let (mut these, mut cut) = (Vec::new(), alloc::vec![0]);
                        let mut file = ""; // ":line": the item before's
                        for item in items.split(';') {
                            if item == "|" {
                                cut.push(these.len() as u32);
                                continue;
                            }
                            let (at, inl) = item.split_once('@').map_or((item, None), |(a, f)| (a, Some(f)));
                            // The functions it is inlined from: an I line's site each.
                            let chain: Vec<Site> = inl.into_iter().flat_map(|c| c.split('@'))
                                .filter_map(|k| unit.sites.get(&k.parse::<u32>().ok()?).copied()).collect();
                            let full;
                            let at = match at.strip_prefix(':') {
                                Some(line) => {
                                    full = alloc::format!("{file}:{line}");
                                    full.as_str()
                                }
                                None => {
                                    file = at.split_once(':').map_or("", |(f, _)| f);
                                    at
                                }
                            };
                            let inlined = (!chain.is_empty()).then(|| chains.get(chain));
                            these.push(Step::new(unit.loc(at), inlined));
                        }
                        // (Once: a unit's copy of a block another unit gave.)
                        blocks.entry((func, n)).or_insert((these, cut));
                    }
                }
                _ => {}
            }
        }
        // (Each function's blocks and variables in its order, from 0: one
        // missing, empty (a variable the kernel's macros made up). A function
        // with blocks or variables but no F line: one with no name.)
        for &(id, _) in blocks.keys().chain(vars.keys()) {
            functions.entry(id).or_insert(FunctionAt { id, ..FunctionAt::zeroed() });
        }
        let (mut block_at, mut steps, mut cuts, mut var_at) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for f in functions.values_mut() {
            (f.blocks, f.vars) = (block_at.len() as u32, var_at.len() as u32);
            let n = blocks.range((f.id, 0)..=(f.id, u32::MAX)).last().map_or(0, |(k, _)| k.1 + 1);
            for n in 0..n {
                block_at.push(BlockAt { steps: steps.len() as u32, cuts: cuts.len() as u32 });
                if let Some((these, cut)) = blocks.remove(&(f.id, n)) {
                    steps.extend(these);
                    cuts.extend(cut);
                }
            }
            let n = vars.range((f.id, 0)..=(f.id, u32::MAX)).last().map_or(0, |(k, _)| k.1 + 1);
            var_at.extend((0..n).map(|k| vars.get(&(f.id, k)).copied().unwrap_or(VarAt::zeroed())));
        }
        let mut name = |list: Vec<String>| crate::flat::keep(list.iter().map(|s| text.name(s)).collect());
        let (inline_names, files) = (name(names.list), name(files.list));
        let mut t = Tables {
            text: "",
            functions: crate::flat::keep(functions.into_values().collect()),
            blocks: crate::flat::keep(block_at),
            steps: &[],
            cuts: crate::flat::keep(cuts),
            inline_names,
            chains: (&[], &[]),
            files,
            vars: crate::flat::keep(var_at),
            types,
        };
        t.text = text.keep();
        (t.steps, t.chains) = t.fold_primitives(steps, chains);
        t
    }

    /// The machine's primitives (arch::PRIMITIVES) inlined into kernel code
    /// are the machine, not calls: a step in one becomes a step at the line
    /// it is inlined at, in the function that inlined it, so that nothing
    /// shows a frame, a node or a line of get_current. (The steps and
    /// chains, kept.)
    fn fold_primitives(&self, mut steps: Vec<Step>, mut chains: Index<Vec<Site>>) -> (&'static [Step], (&'static [At], &'static [Site])) {
        let prim: Vec<bool> = (0..self.inline_names.len() as u32)
            .map(|k| self.types.declared(self.inline_name(k)).is_some_and(|(f, _)| arch::PRIMITIVES.iter().any(|p| f.starts_with(p)))).collect();
        if prim.iter().any(|&p| p) {
            let mut folded: BTreeMap<u32, (Loc, Option<u32>)> = BTreeMap::new();
            for s in steps.iter_mut() {
                let Some(c) = s.inlined() else { continue };
                if let Some(&(loc, inlined)) = folded.get(&c) {
                    *s = Step::new(loc, inlined);
                    continue;
                }
                let chain = &chains.list[c as usize];
                let n = chain.iter().take_while(|site| prim[site.name as usize]).count();
                if n == 0 {
                    continue;
                }
                let loc = chain[n - 1].loc;
                let rest = chain[n..].to_vec();
                let inlined = (!rest.is_empty()).then(|| chains.get(rest));
                folded.insert(c, (loc, inlined));
                *s = Step::new(loc, inlined);
            }
        }
        let mut lists = Lists::default();
        for (k, c) in chains.list.into_iter().enumerate() {
            lists.push(k as u32, c);
        }
        (crate::flat::keep(steps), lists.keep())
    }

    /// Function `id`.
    pub fn function(&self, id: u32) -> Option<Function> {
        self.view_function(&self.functions[self.function_at(id)?])
    }

    /// Function `id`'s index in `functions`.
    fn function_at(&self, id: u32) -> Option<usize> {
        self.functions.binary_search_by_key(&id, |f| f.id).ok()
    }

    /// Function `id`'s blocks and its variables: where they are in `blocks`
    /// and `vars` (none: no function).
    fn spans(&self, id: u32) -> (core::ops::Range<usize>, core::ops::Range<usize>) {
        let Some(k) = self.function_at(id) else { return (0..0, 0..0) };
        let (f, next) = (&self.functions[k], self.functions.get(k + 1));
        (f.blocks as usize..next.map_or(self.blocks.len(), |g| g.blocks as usize),
         f.vars as usize..next.map_or(self.vars.len(), |g| g.vars as usize))
    }

    /// (None: one with blocks but no F line.)
    fn view_function(&self, f: &FunctionAt) -> Option<Function> {
        let name = f.name.of(self.text);
        (!name.is_empty()).then_some(Function { name, loc: f.loc, known: f.known, wide: f.wide, sealed: f.sealed != 0 })
    }

    /// Every function, by id.
    pub fn functions(&self) -> impl Iterator<Item = (u32, Function)> + '_ {
        self.functions.iter().filter_map(|f| Some((f.id, self.view_function(f)?)))
    }

    /// Variable `k` of function `func`.
    pub fn var(&self, func: u32, k: u32) -> Option<Var> {
        self.vars.get(self.spans(func).1).and_then(|vars| vars.get(k as usize)).and_then(|v| self.view_var(v))
    }

    fn view_var(&self, v: &VarAt) -> Option<Var> {
        (v.name.of(self.text) != "").then(|| Var { of: v.of, function: v.function.of(self.text), name: v.name.of(self.text), param: v.param, arg: v.arg })
    }

    /// Function `func`'s variables, in order.
    pub fn vars_of(&self, func: u32) -> impl Iterator<Item = Var> + '_ {
        self.vars[self.spans(func).1].iter().filter_map(|v| self.view_var(v))
    }

    /// Inlined function `k`'s name (a Site's).
    pub fn inline_name(&self, k: u32) -> &'static str {
        self.inline_names.get(k as usize).map_or("", |n| n.of(self.text))
    }

    /// Every inlined function's name, by k (see inline_name).
    pub fn inline_names(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.inline_names.iter().map(|n| n.of(self.text))
    }

    /// How many inline chains there are (see chain).
    pub fn chains(&self) -> u32 {
        self.chains.0.len() as u32
    }

    /// Block `n` of `func`.
    pub fn block(&self, func: u32, n: u32) -> Option<Block<'static>> {
        let blocks = self.spans(func).0;
        ((n as usize) < blocks.len()).then(|| self.view(blocks.start + n as usize))
    }

    /// Block `k` (of `blocks`): its steps and cuts, to the next one's.
    fn view(&self, k: usize) -> Block<'static> {
        let (steps, cuts): (&'static [Step], &'static [u32]) = (self.steps, self.cuts);
        let (b, next) = (self.blocks[k], self.blocks.get(k + 1));
        Block { steps: &steps[b.steps as usize..next.map_or(steps.len(), |n| n.steps as usize)],
                cuts: &cuts[b.cuts as usize..next.map_or(cuts.len(), |n| n.cuts as usize)] }
    }

    /// Every line block `n` of `func` runs: its lines' own, and the lines
    /// the code of each is inlined at (`x = helper();`, helper inlined, runs
    /// no code of its own line's: the helper's lines run in its place; the
    /// debug info's inline sites say where). What coverage counts, and what
    /// "run to a line" looks for.
    pub fn block_lines(&self, func: u32, n: u32) -> impl Iterator<Item = Loc> + '_ {
        self.block(func, n).into_iter().flat_map(|b| b.steps)
            .flat_map(move |s| core::iter::once(s.loc).chain(self.chain(s.inlined()).iter().map(|site| site.loc)))
    }

    /// Inline chain `k` (see Step), innermost first, or none.
    pub fn chain(&self, k: Option<u32>) -> &'static [Site] {
        k.map_or(&[], |k| crate::flat::list(self.chains.0, self.chains.1, k))
    }

    /// Function `func`'s argument values, from a call's words (trace::Args):
    /// a word each, two (low, high) for a wide one.
    pub fn values(&self, func: u32, words: &[u32]) -> Vec<u64> {
        let wide = self.function(func).map_or(0, |f| f.wide);
        let (mut out, mut w) = (Vec::new(), words.iter().copied());
        while let Some(lo) = w.next() {
            let hi = if wide >> out.len() & 1 == 1 { w.next().unwrap_or(0) } else { 0 };
            out.push(lo as u64 | (hi as u64) << 32);
        }
        out
    }

    /// Where the blocks of `func` begin, in order.
    pub fn blocks_of(&self, func: u32) -> impl Iterator<Item = Loc> + '_ {
        self.spans(func).0.map(|k| self.view(k).loc())
    }

    pub fn file(&self, loc: Loc) -> &'static str {
        self.files.get(loc.file as usize).map_or("?", |f| f.of(self.text))
    }

    /// The id of file `path` (Loc::file; the paths are each once).
    pub(crate) fn file_id(&self, path: &str) -> Option<u32> {
        self.files.iter().position(|f| f.of(self.text) == path).map(|k| k as u32)
    }

    /// The id of the (first) instrumented function called `name`.
    pub fn find(&self, name: &str) -> Option<u32> {
        self.functions.iter().find(|f| f.name.of(self.text) == name).map(|f| f.id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_code_and_types() {
        let code = "U\nP\t0\t/s/p.c\nP\t1\t/s/q.h\nF\t7\tpick\t0:2\t3\t0\nL\t7\t0\t1:2\nU\nP\t0\t/s/p.c\nF\t7\tpick\t0:2\t3\t0\nF\t8\tcrc\t0:9\t1\t1\n";
        let t = Tables::parse(code, "T\t1\tbase\tint\t4\ts\nT\t2\tstruct\ttask_struct\t740\nM\t2\tpid\t5888\t0\t1\n");
        let f = t.function(7).unwrap();
        assert_eq!((f.known, f.sealed, t.function(8).unwrap().sealed), (3, false, true));
        assert_eq!((t.file(f.loc), f.loc.line), ("/s/p.c", 2));
        let b = t.block(7, 0).unwrap().loc();
        assert_eq!((t.file(b), b.line), ("/s/q.h", 2));
        assert_eq!(t.types.field("task_struct", "pid"), Some((736, 4)));
    }

    #[test]
    fn the_machines_primitives_fold_into_their_lines() {
        // f's block 0 runs line 3 of get_current (arch/wasm's, inlined into
        // spin_lock at its line 9, inlined into f at line 2), then line 4 of
        // f: the get_current step is spin_lock's line 9.
        let code = "U\nP\t0\tf.c\nP\t1\tinclude/linux/spinlock.h\nP\t2\tarch/wasm/include/asm/current.h\n\
                      I\t0\tget_current\t1:9\nI\t1\tspin_lock\t0:2\n\
                      F\t1\tf\t0:1\t0\nL\t1\t0\t2:3@0@1;0:4\n";
        let t = Tables::parse(code, "W\tget_current\tarch/wasm/include/asm/current.h\t12\nW\tspin_lock\tinclude/linux/spinlock.h\t7\n");
        let steps = t.block(1, 0).unwrap().steps;
        let chain = |s: &Step| s.inlined().map(|k| t.chain(Some(k)).iter().map(|site| t.inline_name(site.name)).collect::<Vec<_>>());
        assert_eq!((t.file(steps[0].loc), steps[0].loc.line, chain(&steps[0])), ("include/linux/spinlock.h", 9, Some(alloc::vec!["spin_lock"])));
        assert_eq!((steps[1].loc.line, steps[1].inlined()), (4, None));
        // The block's own location was get_current's: now its first line's.
        let b = t.block(1, 0).unwrap().loc();
        assert_eq!((t.file(b), b.line), ("include/linux/spinlock.h", 9));
    }

    #[test]
    fn wide_arguments_take_two_words() {
        // f(int a, loff_t b, int c): b is wide (bit 1).
        let t = Tables::parse("F\t7\tf\t0:1\t7\t0\t2\n", "");
        assert_eq!(t.values(7, &[1, 2, 3, 4]), [1, 3 << 32 | 2, 4]);
    }

    #[test]
    fn bytes_are_the_tables() {
        let t = Tables::parse("U\nP\t0\tf.c\nF\t7\tf\t0:1\t7\t0\t2\nL\t7\t0\t0:3\n", "T\t1\tbase\tint\t4\ts\nP\t7\tf\ta:1\n");
        let bytes = t.to_bytes();
        let back = Tables::from_bytes(crate::flat::kept(bytes.len(), |b| b.copy_from_slice(&bytes))).unwrap();
        assert_eq!((back.function(7).unwrap().name, back.file(back.block(7, 0).unwrap().loc())), ("f", "f.c"));
        assert_eq!(back.types.params(7).collect::<Vec<_>>(), [("a", 1)]);
    }
}
