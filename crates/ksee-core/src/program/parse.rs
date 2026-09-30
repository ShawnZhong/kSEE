//! A program's line parsed (see lex): a call, a declaration or an
//! assignment, each of C expressions and initializers.

use alloc::boxed::Box;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use super::lex::{tokens, Tok, Token};
use crate::types::Types;

/// A C expression, or an initializer (a string, `{…}`).
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Num(i64),
    Str(Vec<u8>),
    Name(String),
    Unary(&'static str, Box<Expr>),
    Binary(&'static str, Box<Expr>, Box<Expr>),
    Cast(TypeName, Box<Expr>),
    SizeofType(TypeName),
    SizeofExpr(Box<Expr>),
    /// `e.m`, or `e->m` (true).
    Member(Box<Expr>, String, bool),
    Index(Box<Expr>, Box<Expr>),
    /// `{a, .x = b}`.
    Init(Vec<(Option<String>, Expr)>),
}

/// A type as written: its name ("struct io_uring_sqe", "unsigned int",
/// "pid_t"), and how many pointers deep.
#[derive(Debug, Clone, PartialEq)]
pub struct TypeName {
    pub name: String,
    pub depth: u32,
}

impl TypeName {
    /// As C writes it: "void *", "int".
    pub fn text(&self) -> String {
        let stars: String = "*".repeat(self.depth as usize);
        if stars.is_empty() { self.name.clone() } else { format!("{} {stars}", self.name) }
    }
}

/// A program's line: made by `runner` (a thread id or a result's name, as
/// written; none: the program), one of these.
#[derive(Debug, Clone, PartialEq)]
pub struct Statement {
    pub runner: Option<String>,
    pub line: Line,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Line {
    /// `[type] [name =] call(args)`: a syscall, its result named, and typed.
    Call { ty: Option<TypeName>, name: Option<String>, call: String, args: Vec<Expr>, texts: Vec<String> },
    /// `type name[count] = init`: a variable of the program's (`count`:
    /// Some(None) for `[]`).
    Decl { ty: TypeName, name: String, count: Option<Option<Expr>>, init: Option<Expr> },
    /// `target = value`: memory of the program's written.
    Assign { target: Expr, value: Expr, texts: (String, String) },
}

/// The words that begin a type.
const TYPE_WORDS: [&str; 13] = ["struct", "union", "enum", "unsigned", "signed", "char", "short", "int", "long", "void", "const", "_Bool", "volatile"];

pub(super) struct Parser<'a> {
    toks: Vec<Token>,
    at: usize,
    text: &'a str,
    /// For the names that are types in a cast: `(pid_t)x`.
    types: Option<&'a Types>,
}

impl<'a> Parser<'a> {
    pub(super) fn new(text: &'a str, types: Option<&'a Types>) -> Result<Parser<'a>, String> {
        Ok(Parser { toks: tokens(text)?, at: 0, text, types })
    }
    fn tok(&self, k: usize) -> Option<&Tok> {
        self.toks.get(self.at + k).map(|t| &t.tok)
    }
    fn is(&self, k: usize, op: &str) -> bool {
        matches!(self.tok(k), Some(Tok::Op(o)) if *o == op)
    }
    fn name(&self, k: usize) -> Option<&str> {
        match self.tok(k) {
            Some(Tok::Name(n)) => Some(n),
            _ => None,
        }
    }
    fn eat(&mut self, op: &str) -> bool {
        let yes = self.is(0, op);
        self.at += yes as usize;
        yes
    }
    fn expect(&mut self, op: &str) -> Result<(), String> {
        if self.eat(op) { Ok(()) } else { Err(format!("a {op} missing{}", self.here())) }
    }
    fn here(&self) -> String {
        self.toks.get(self.at).map_or(String::new(), |t| format!(" at {}", &self.text[t.start..]))
    }
    fn done(&self) -> bool {
        self.at >= self.toks.len()
    }
    /// The text of tokens `from..to`.
    fn span(&self, from: usize, to: usize) -> String {
        match (self.toks.get(from), to.checked_sub(1).and_then(|k| self.toks.get(k))) {
            (Some(a), Some(b)) if to > from => self.text[a.start..b.end].to_string(),
            _ => String::new(),
        }
    }

    /// Whether a type starts here, followed by a name (a declaration's, a
    /// result's): `int fd`, `void *mem`, `pid_t child`, `struct x p`.
    fn declares(&self) -> bool {
        let Some(first) = self.name(0) else { return false };
        if TYPE_WORDS.contains(&first) {
            return true;
        }
        let mut k = 1;
        while self.is(k, "*") {
            k += 1;
        }
        self.name(k).is_some()
    }

    /// Whether a type name starts here, in a cast or sizeof (after its "("):
    /// a type's word, or a type the kernel has followed by `*` or `)`.
    fn type_here(&self) -> bool {
        match self.name(0) {
            Some(w) if TYPE_WORDS.contains(&w) => true,
            Some(w) => self.types.is_some_and(|t| t.named(w).is_some()) && (self.is(1, "*") || self.is(1, ")")),
            None => false,
        }
    }

    fn type_name(&mut self) -> Result<TypeName, String> {
        // (A tag, `struct x`; C's own words, `unsigned long long`; or a typedef's name.)
        let mut words: Vec<String> = Vec::new();
        match self.name(0) {
            Some(w @ ("struct" | "union" | "enum")) => {
                let tag = self.name(1).ok_or_else(|| format!("{w}: its name missing"))?;
                words.extend([w.to_string(), tag.to_string()]);
                self.at += 2;
            }
            Some(w) if TYPE_WORDS.contains(&w) => {
                while let Some(w) = self.name(0).filter(|w| TYPE_WORDS.contains(w)) {
                    words.push(w.to_string());
                    self.at += 1;
                }
            }
            Some(w) => {
                words.push(w.to_string());
                self.at += 1;
            }
            None => return Err(format!("a type missing{}", self.here())),
        }
        words.retain(|w| !matches!(w.as_str(), "const" | "volatile"));
        let mut depth = 0;
        while self.eat("*") {
            depth += 1;
        }
        Ok(TypeName { name: words.join(" "), depth })
    }

    /// A type name, alone: all of the text.
    pub(super) fn type_name_only(&mut self) -> Result<TypeName, String> {
        let t = self.type_name()?;
        if !self.done() {
            return Err(format!("not a type{}", self.here()));
        }
        Ok(t)
    }

    /// An expression, or an initializer (`{…}`, a string).
    pub(super) fn init(&mut self) -> Result<Expr, String> {
        if !self.eat("{") {
            return self.expr();
        }
        let mut items = Vec::new();
        while !self.eat("}") {
            let name = if self.is(0, ".") && self.name(1).is_some() && self.is(2, "=") {
                let n = self.name(1).map(String::from);
                self.at += 3;
                n
            } else {
                None
            };
            items.push((name, self.init()?));
            if !self.eat(",") {
                self.expect("}")?;
                break;
            }
        }
        Ok(Expr::Init(items))
    }

    pub(super) fn expr(&mut self) -> Result<Expr, String> {
        self.binary(0)
    }

    /// Binary operators, by C's precedence (loosest first).
    fn binary(&mut self, level: usize) -> Result<Expr, String> {
        const LEVELS: [&[&str]; 10] = [&["||"], &["&&"], &["|"], &["^"], &["&"], &["==", "!="], &["<", ">", "<=", ">="], &["<<", ">>"], &["+", "-"], &["*", "/", "%"]];
        if level == LEVELS.len() {
            return self.unary();
        }
        let mut l = self.binary(level + 1)?;
        while let Some(&Tok::Op(op)) = self.tok(0) {
            if !LEVELS[level].contains(&op) {
                break;
            }
            self.at += 1;
            l = Expr::Binary(op, Box::new(l), Box::new(self.binary(level + 1)?));
        }
        Ok(l)
    }

    fn unary(&mut self) -> Result<Expr, String> {
        for op in ["-", "~", "!", "*", "&", "+"] {
            if self.eat(op) {
                return Ok(Expr::Unary(op, Box::new(self.unary()?)));
            }
        }
        if self.name(0) == Some("sizeof") {
            self.at += 1;
            self.expect("(")?;
            let e = if self.type_here() { Expr::SizeofType(self.type_name()?) } else { Expr::SizeofExpr(Box::new(self.expr()?)) };
            self.expect(")")?;
            return Ok(e);
        }
        if self.is(0, "(") {
            self.at += 1;
            if self.type_here() {
                let ty = self.type_name()?;
                self.expect(")")?;
                return Ok(Expr::Cast(ty, Box::new(self.unary()?)));
            }
            self.at -= 1;
        }
        self.postfix()
    }

    fn postfix(&mut self) -> Result<Expr, String> {
        let mut e = self.primary()?;
        loop {
            if self.eat("[") {
                let i = self.expr()?;
                self.expect("]")?;
                e = Expr::Index(Box::new(e), Box::new(i));
            } else if self.is(0, ".") || self.is(0, "->") {
                let arrow = self.is(0, "->");
                self.at += 1;
                let m = self.name(0).ok_or_else(|| format!("a member's name missing{}", self.here()))?.to_string();
                self.at += 1;
                e = Expr::Member(Box::new(e), m, arrow);
            } else {
                return Ok(e);
            }
        }
    }

    fn primary(&mut self) -> Result<Expr, String> {
        let t = self.toks.get(self.at).map(|t| t.tok.clone());
        self.at += 1;
        match t {
            Some(Tok::Num(n)) => Ok(Expr::Num(n)),
            Some(Tok::Str(s)) => Ok(Expr::Str(s)),
            Some(Tok::Name(n)) => Ok(Expr::Name(n)),
            Some(Tok::Op("(")) => {
                let e = self.expr()?;
                self.expect(")")?;
                Ok(e)
            }
            Some(Tok::Op("{")) => {
                self.at -= 1;
                self.init()
            }
            Some(Tok::Op(o)) => Err(format!("not a value: {o}")),
            None => Err("a value missing".into()),
        }
    }

    /// Whether a call starts at token `k` and ends the line: `name(…)`.
    fn call_ends_at(&self, k: usize) -> bool {
        if self.name(k).is_none() || !self.is(k + 1, "(") {
            return false;
        }
        let mut depth = 0;
        for (j, t) in self.toks.iter().enumerate().skip(self.at + k + 1) {
            match t.tok {
                Tok::Op("(" | "[" | "{") => depth += 1,
                Tok::Op(")" | "]" | "}") => {
                    depth -= 1;
                    if depth == 0 {
                        return j + 1 == self.toks.len();
                    }
                }
                _ => {}
            }
        }
        false
    }

    /// A call from here to the end: its name, its arguments, as written too.
    fn call(&mut self) -> Result<(String, Vec<Expr>, Vec<String>), String> {
        let name = self.name(0).ok_or("a call's name missing")?.to_string();
        self.at += 1;
        self.expect("(")?;
        let (mut args, mut texts) = (Vec::new(), Vec::new());
        if !self.eat(")") {
            loop {
                let from = self.at;
                args.push(self.init()?);
                texts.push(self.span(from, self.at));
                if self.eat(")") {
                    break;
                }
                self.expect(",")?;
            }
        }
        Ok((name, args, texts))
    }

    pub(super) fn statement(&mut self) -> Result<Statement, String> {
        // (Its runner: `19:`, `child:`.)
        let runner = match (self.tok(0), self.is(1, ":")) {
            (Some(Tok::Name(n)), true) => Some(n.clone()),
            (Some(Tok::Num(n)), true) => Some(n.to_string()),
            _ => None,
        };
        if runner.is_some() {
            self.at += 2;
        }
        while self.toks.last().is_some_and(|t| t.tok == Tok::Op(";")) {
            self.toks.pop();
        }
        let line = if self.declares() {
            let ty = self.type_name()?;
            let name = self.name(0).ok_or_else(|| format!("a name missing{}", self.here()))?.to_string();
            self.at += 1;
            let count = if self.eat("[") {
                let c = if self.is(0, "]") { None } else { Some(self.expr()?) };
                self.expect("]")?;
                Some(c)
            } else {
                None
            };
            if self.eat("=") {
                if count.is_none() && self.call_ends_at(0) {
                    let (call, args, texts) = self.call()?;
                    Line::Call { ty: Some(ty), name: Some(name), call, args, texts }
                } else {
                    Line::Decl { ty, name, count, init: Some(self.init()?) }
                }
            } else {
                Line::Decl { ty, name, count, init: None }
            }
        } else if self.name(0).is_some() && self.is(1, "=") && self.call_ends_at(2) {
            let name = self.name(0).map(String::from);
            self.at += 2;
            let (call, args, texts) = self.call()?;
            Line::Call { ty: None, name, call, args, texts }
        } else if self.call_ends_at(0) {
            let (call, args, texts) = self.call()?;
            Line::Call { ty: None, name: None, call, args, texts }
        } else {
            let from = self.at;
            let target = self.expr()?;
            let t = self.span(from, self.at);
            self.expect("=")?;
            let from = self.at;
            let value = self.init()?;
            Line::Assign { target, value, texts: (t, self.span(from, self.at)) }
        };
        if !self.done() {
            return Err(format!("not C here{}", self.here()));
        }
        Ok(Statement { runner, line })
    }
}

/// A program's line, parsed.
pub fn statement(text: &str) -> Result<Statement, String> {
    Parser::new(text, None)?.statement()
}

/// An expression or an initializer, parsed (`types`: for casts to their
/// typedef names).
pub fn expression(text: &str, types: Option<&Types>) -> Result<Expr, String> {
    let mut p = Parser::new(text, types)?;
    let e = p.init()?;
    if !p.done() {
        return Err(format!("not C here{}", p.here()));
    }
    Ok(e)
}

/// `text`'s tokens as far as they go (the rest, a string not closed yet,
/// left out): for text being written.
fn tokens_so_far(text: &str) -> Vec<Token> {
    let mut end = text.len();
    loop {
        if let Ok(t) = tokens(&text[..end]) {
            return t;
        }
        // (Back to before the last quote: what is being written.)
        match text[..end].rfind(['"', '\'']) {
            Some(k) => end = k,
            None => return Vec::new(),
        }
    }
}

/// A program written on one line (a URL's, an example's try): its lines,
/// separated by `;` outside strings and brackets, each trimmed; none empty.
pub fn lines(text: &str) -> Vec<&str> {
    let (mut out, mut from, mut depth) = (Vec::new(), 0, 0);
    for t in tokens_so_far(text) {
        match t.tok {
            Tok::Op("(" | "[" | "{") => depth += 1,
            Tok::Op(")" | "]" | "}") => depth -= 1,
            Tok::Op(";") if depth == 0 => {
                out.push(&text[from..t.start]);
                from = t.end;
            }
            _ => {}
        }
    }
    out.push(&text[from..]);
    out.into_iter().map(str::trim).filter(|l| !l.is_empty()).collect()
}

/// Where the cursor is in a program's line being written (see
/// statement): the call's name as far as written, and where it starts;
/// the word at the cursor, and where it starts; in the arguments, which
/// one (from 0). Offsets in UTF-16 units, as the page's text has them.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct Context {
    pub call: String,
    pub call_start: usize,
    pub word: String,
    pub word_start: usize,
    pub param: Option<usize>,
}

/// The cursor at `at` (UTF-16 units) in `text`, a program's line (see
/// Context): `19: int fd = open|` is at the call's name, "open";
/// `read(fd, b|` at argument 1, the word "b".
pub fn context(text: &str, at: usize) -> Context {
    let cursor = text.char_indices().scan(0, |u, (i, c)| {
        let here = *u;
        *u += c.len_utf16();
        Some((here, i))
    }).find(|&(u, _)| u >= at).map_or(text.len(), |(_, i)| i);
    let utf16 = |b: usize| text[..b].encode_utf16().count();
    let all = tokens_so_far(text);
    // (The call's "(": the first outside brackets; its name, the name before it, else the line's last.)
    let open = all.iter().position(|t| t.tok == Tok::Op("("));
    let callee = match open {
        Some(k) => k.checked_sub(1).filter(|&j| matches!(all[j].tok, Tok::Name(_))),
        None => all.iter().rposition(|t| matches!(t.tok, Tok::Name(_))),
    };
    let (call, call_start) = callee.map_or((String::new(), cursor), |j| (text[all[j].start..all[j].end].to_string(), all[j].start));
    // (The word the cursor ends, if it does.)
    let before = tokens_so_far(&text[..cursor]);
    let (word, word_start) = match before.last() {
        Some(t) if t.end == cursor && matches!(t.tok, Tok::Name(_)) => (text[t.start..t.end].to_string(), t.start),
        _ => (String::new(), cursor),
    };
    // (In the arguments: the commas before the cursor, at the call's own depth.)
    let param = open.filter(|&k| all[k].start < cursor).map(|k| {
        let mut depth = 0;
        before.iter().skip(k + 1).filter(|t| {
            match t.tok {
                Tok::Op("(" | "[" | "{") => depth += 1,
                Tok::Op(")" | "]" | "}") => depth -= 1,
                _ => {}
            }
            depth == 0 && t.tok == Tok::Op(",")
        }).count()
    });
    Context { call, call_start: utf16(call_start), word, word_start: utf16(word_start), param }
}
