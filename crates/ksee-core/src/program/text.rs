//! Reading C text: constant expressions, literals, a program's line, the
//! names in a line of source.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// A C constant expression: numbers (decimal, 0x hex, 0 octal, a U or L
/// suffix), names (`known`, `&name` the same; `name[i]` known's as
/// "name[i]": a variable's element), `|`, `<<`, `+`, `-`, parentheses.
pub fn eval(s: &str, known: &dyn Fn(&str) -> Option<i64>) -> Result<i64, String> {
    type K<'a> = &'a dyn Fn(&str) -> Option<i64>;
    fn or(t: &mut &str, k: K) -> Result<i64, String> {
        let mut v = shift(t, k)?;
        while let Some(r) = t.trim_start().strip_prefix('|') {
            *t = r;
            v |= shift(t, k)?;
        }
        Ok(v)
    }
    fn shift(t: &mut &str, k: K) -> Result<i64, String> {
        let mut v = sum(t, k)?;
        while let Some(r) = t.trim_start().strip_prefix("<<") {
            *t = r;
            v <<= sum(t, k)?;
        }
        Ok(v)
    }
    fn sum(t: &mut &str, k: K) -> Result<i64, String> {
        let mut v = unary(t, k)?;
        loop {
            let s = t.trim_start();
            if let Some(r) = s.strip_prefix('+') {
                *t = r;
                v += unary(t, k)?;
            } else if let Some(r) = s.strip_prefix('-') {
                *t = r;
                v -= unary(t, k)?;
            } else {
                return Ok(v);
            }
        }
    }
    fn unary(t: &mut &str, k: K) -> Result<i64, String> {
        *t = t.trim_start();
        if let Some(r) = t.strip_prefix('-') {
            *t = r;
            return Ok(-unary(t, k)?);
        }
        // ('h', '\n': a character's number.)
        if let Some(r) = t.strip_prefix('\'') {
            let end = r.char_indices().skip(1).find(|&(i, c)| c == '\'' && !r[..i].ends_with('\\')).map(|(i, _)| i)
                .or_else(|| r.find('\''))
                .ok_or("a ' missing")?;
            let c = unquote(&format!("\"{}\"", &r[..end]))?;
            *t = &r[end + 1..];
            // (A character's byte, or the first of its UTF-8 bytes.)
            return c.first().map(|&c| c as i64).ok_or_else(|| "an empty character".into());
        }
        // (&x: a program's variable's address, which its name stands for
        // anyway, as C writes it for a struct.)
        if let Some(r) = t.strip_prefix('&') {
            *t = r;
            return unary(t, k);
        }
        if let Some(r) = t.strip_prefix('(') {
            *t = r;
            let v = or(t, k)?;
            *t = t.trim_start().strip_prefix(')').ok_or("a ) missing")?;
            return Ok(v);
        }
        let n = t.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '$')).unwrap_or(t.len());
        let (word, rest) = t.split_at(n);
        *t = rest;
        if word.is_empty() {
            return Err(if rest.is_empty() { "a value missing".into() } else { format!("not an argument: {rest}") });
        }
        let num = word.trim_end_matches(['U', 'L', 'u', 'l']);
        if let Some(h) = num.strip_prefix("0x").or_else(|| num.strip_prefix("0X")) {
            i64::from_str_radix(h, 16).map_err(|_| format!("not a number: {word}"))
        } else if num.len() > 1 && num.starts_with('0') {
            i64::from_str_radix(&num[1..], 8).map_err(|_| format!("not a number: {word}"))
        } else if num.starts_with(|c: char| c.is_ascii_digit()) {
            num.parse().map_err(|_| format!("not a number: {word}"))
        } else if word == "NULL" {
            Ok(0)
        } else if let Some(r) = t.trim_start().strip_prefix('[') {
            // (A variable's element: `fds[1]`, its value, `known`'s as
            // "fds[1]".)
            *t = r;
            let i = or(t, k)?;
            *t = t.trim_start().strip_prefix(']').ok_or("a ] missing")?;
            k(&format!("{word}[{i}]")).ok_or_else(|| format!("no {word}[{i}]"))
        } else {
            k(word).ok_or_else(|| format!("not a number, a constant or a result: {word}"))
        }
    }
    let mut t = s;
    let v = or(&mut t, known)?;
    if !t.trim().is_empty() {
        return Err(format!("not an argument: {}", t.trim()));
    }
    Ok(v)
}

/// A compound literal's items: `{a, b}`, `{.x = a}`, nested braces, strings.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Item {
    Expr(String),
    Str(Vec<u8>),
    List(Vec<(Option<String>, Item)>),
}

/// The text in the braces of a literal, split at its commas (not those in
/// strings or nested brackets), each item with its `.name =` if given.
pub(super) fn items(text: &str) -> Result<Vec<(Option<String>, Item)>, String> {
    split(text).into_iter().filter(|s| !s.is_empty()).map(|s| {
        let (name, value) = match s.strip_prefix('.').and_then(|r| r.split_once('=')) {
            Some((n, v)) if n.trim().chars().all(|c| c.is_ascii_alphanumeric() || c == '_') => (Some(n.trim().to_string()), v.trim()),
            _ => (None, s),
        };
        Ok((name, literal(value)?))
    }).collect()
}

/// Each byte of `text`'s nesting: None in a string (its quotes too), else
/// how many brackets ((, {, [) are open around it (a bracket's own: its
/// pair's).
pub(super) fn nesting(text: &str) -> Vec<Option<u32>> {
    let (mut out, mut depth, mut quote, mut escaped) = (Vec::with_capacity(text.len()), 0u32, false, false);
    for &b in text.as_bytes() {
        if quote || b == b'"' {
            out.push(None);
            match b {
                _ if escaped => escaped = false,
                b'\\' => escaped = true,
                b'"' => quote = !quote,
                _ => {}
            }
            continue;
        }
        if matches!(b, b')' | b'}' | b']') {
            depth = depth.saturating_sub(1);
        }
        out.push(Some(depth));
        if matches!(b, b'(' | b'{' | b'[') {
            depth += 1;
        }
    }
    out
}

/// `text` split at its commas outside strings and brackets, each trimmed.
pub(super) fn split(text: &str) -> Vec<&str> {
    let n = nesting(text);
    let mut out = Vec::new();
    let mut start = 0;
    for (i, &b) in text.as_bytes().iter().enumerate() {
        if b == b',' && n[i] == Some(0) {
            out.push(text[start..i].trim());
            start = i + 1;
        }
    }
    out.push(text[start..].trim());
    out
}

/// A C string literal's bytes (\n, \t, \\, \", \0, \xHH).
pub(super) fn unquote(s: &str) -> Result<Vec<u8>, String> {
    let inner = s.strip_prefix('"').and_then(|r| r.strip_suffix('"')).ok_or_else(|| format!("not a string: {s}"))?;
    let mut out = Vec::new();
    let mut chars = inner.chars();
    // (A character as its UTF-8 bytes; an escape, the byte it says.)
    let push = |out: &mut Vec<u8>, c: char| out.extend_from_slice(c.encode_utf8(&mut [0; 4]).as_bytes());
    while let Some(c) = chars.next() {
        if c != '\\' {
            push(&mut out, c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push(b'\n'),
            Some('t') => out.push(b'\t'),
            Some('r') => out.push(b'\r'),
            Some('0') => out.push(0),
            Some('x') => {
                let h: String = chars.by_ref().take(2).collect();
                out.push(u8::from_str_radix(&h, 16).map_err(|_| format!("bad escape in {s}"))?);
            }
            Some(c) => push(&mut out, c),
            None => return Err(format!("bad escape in {s}")),
        }
    }
    Ok(out)
}

/// A name in a line of C source (see names): `expr`, it with the fields
/// before it ("file->f_pos" for f_pos), from `start` (where the first of
/// them is) to `end`, the name itself from `at`; `member`: it follows a ->
/// or a . (a field's, not a variable's), and then `parent`, the
/// expression it is a field of ("file"). Offsets in UTF-16 units, as the
/// page's text has them.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct Name {
    pub expr: String,
    pub start: usize,
    pub at: usize,
    pub end: usize,
    pub member: bool,
    pub parent: Option<String>,
}

/// C's keywords (and its constants' words): not names of values.
const KEYWORDS: &[&str] = &["auto", "break", "case", "char", "const", "continue", "default", "do", "double", "else", "enum", "extern",
    "float", "for", "goto", "if", "inline", "int", "long", "register", "restrict", "return", "short", "signed", "sizeof", "static",
    "struct", "switch", "typedef", "union", "unsigned", "void", "volatile", "while", "bool", "true", "false", "NULL"];

/// The names in `text`, a line of C source, in order (see Name): what the
/// page's source hovers, and the values under a line, are of.
pub fn names(text: &str) -> Vec<Name> {
    let chars: Vec<char> = text.chars().collect();
    // (Each char's offset in UTF-16 units.)
    let mut pos = Vec::with_capacity(chars.len() + 1);
    let mut u = 0;
    for c in &chars {
        pos.push(u);
        u += c.len_utf16();
    }
    pos.push(u);
    let word = |c: char| c.is_ascii_alphanumeric() || c == '_';
    // (The identifier ending just before char `k`, past spaces: its start.)
    let before = |mut k: usize| {
        while k > 0 && chars[k - 1] == ' ' {
            k -= 1;
        }
        let end = k;
        while k > 0 && word(chars[k - 1]) {
            k -= 1;
        }
        (k < end && !chars[k].is_ascii_digit()).then_some((k, end))
    };
    let mut out = Vec::new();
    let mut k = 0;
    while k < chars.len() {
        if !word(chars[k]) {
            k += 1;
            continue;
        }
        let a = k;
        while k < chars.len() && word(chars[k]) {
            k += 1;
        }
        let token: String = chars[a..k].iter().collect();
        if chars[a].is_ascii_digit() || KEYWORDS.contains(&token.as_str()) {
            continue;
        }
        // (The fields before it: `x->` or `x.`, back to the first.)
        let (mut start, mut expr, mut member, mut op1) = (a, token.clone(), false, 0);
        loop {
            let mut j = start;
            while j > 0 && chars[j - 1] == ' ' {
                j -= 1;
            }
            let op = if j >= 2 && chars[j - 2] == '-' && chars[j - 1] == '>' { 2 } else if j >= 1 && chars[j - 1] == '.' { 1 } else { break };
            let Some((b, e)) = before(j - op) else { break };
            if !member {
                (member, op1) = (true, op);
            }
            expr = format!("{}{}{expr}", chars[b..e].iter().collect::<String>(), if op == 2 { "->" } else { "." });
            start = b;
        }
        let parent = member.then(|| expr[..expr.len() - token.len() - op1].to_string());
        out.push(Name { expr, start: pos[start], at: pos[a], end: pos[k], member, parent });
    }
    out
}

/// A program written on one line (a URL's, an example's try): its lines,
/// separated by `;` outside strings, each trimmed; none empty.
pub fn lines(text: &str) -> Vec<&str> {
    let n = nesting(text);
    let mut out = Vec::new();
    let mut from = 0;
    for (i, b) in text.bytes().enumerate() {
        if b == b';' && n[i].is_some() {
            out.push(&text[from..i]);
            from = i + 1;
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
    // (UTF-16 offsets to bytes and back.)
    let byte = text.char_indices().scan(0, |u, (i, c)| {
        let here = *u;
        *u += c.len_utf16();
        Some((here, i))
    }).find(|&(u, _)| u >= at).map_or(text.len(), |(_, i)| i);
    let utf16 = |b: usize| text[..b].encode_utf16().count();
    let word = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '$';
    // (The call's "(": the first outside a string.)
    let n = nesting(text);
    let open = text.bytes().enumerate().find(|&(i, c)| c == b'(' && n[i].is_some()).map(|(i, _)| i);
    // (Its name: the word before it, after a runner's `19:` and a result's `int fd =`.)
    let head = &text[..open.unwrap_or(text.len())];
    let head = head.trim_end();
    let start = head.rfind(|c: char| !word(c)).map_or(0, |k| k + head[k..].chars().next().map_or(1, char::len_utf8));
    let before = &text[..byte];
    let w = before.rfind(|c: char| !word(c)).map_or(0, |k| k + before[k..].chars().next().map_or(1, char::len_utf8));
    // (In the arguments: the commas before the cursor outside strings and () {}.)
    let param = open.filter(|&o| o < byte).map(|o| {
        let depth = n[o];
        (o + 1..byte).filter(|&i| text.as_bytes()[i] == b',' && n[i] == depth.map(|d| d + 1)).count()
    });
    Context { call: head[start..].to_string(), call_start: utf16(start), word: before[w..].to_string(), word_start: utf16(w), param }
}

/// A program's line (see statement): a call, `[runner:] [type] [name =]
/// syscall(args)` (its arguments as written: Types::arg reads each), or a
/// variable's declaration, `[runner:] type name[n] = init` (Types::declare
/// reads it).
#[derive(Debug, Clone, PartialEq)]
pub enum Statement {
    Call { tid: Option<String>, ty: Option<String>, name: Option<String>, call: String, args: Vec<String> },
    Decl { tid: Option<String>, text: String },
}

/// A program's line, parsed (see Statement): a call if it ends in one
/// (`name(…)`, its parentheses the line's end), else a declaration.
pub fn statement(text: &str) -> Result<Statement, String> {
    let s = text.trim().trim_end_matches(';').trim();
    let word = |w: &str| !w.is_empty() && w.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$') && !w.starts_with(|c: char| c.is_ascii_digit());
    // (`19:` or `child:`, its runner; not `::`.)
    let (tid, s) = match s.split_once(':') {
        Some((t, r)) if word(t.trim()) || t.trim().chars().all(|c| c.is_ascii_digit()) && !t.trim().is_empty() => (Some(t.trim().to_string()), r.trim()),
        _ => (None, s),
    };
    // (The call: the first "(" outside strings, closed at the line's end.)
    let n = nesting(s);
    let b = s.as_bytes();
    let open = (0..b.len()).find(|&i| b[i] == b'(' && n[i].is_some());
    let close = open.and_then(|o| (o + 1..b.len()).find(|&i| b[i] == b')' && n[i] == n[o]));
    if open.is_some() && close.is_none() {
        return Err("a ) missing".into());
    }
    let (Some(open), Some(close)) = (open, close) else { return Ok(Statement::Decl { tid, text: s.to_string() }) };
    if close + 1 != s.len() {
        return Ok(Statement::Decl { tid, text: s.to_string() });
    }
    let head = s[..open].trim();
    let (lhs, call) = match head.rsplit_once('=') {
        Some((l, c)) if !l.ends_with(['=', '!', '<', '>']) => (Some(l.trim()), c.trim()),
        _ => (None, head),
    };
    if !word(call) {
        return Err(format!("not a call: {head}"));
    }
    // (`int fd`, `void *mem`: the result's type, then its name.)
    let (ty, name) = match lhs {
        None => (None, None),
        Some(l) => {
            let at = l.rfind(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).map_or(0, |k| k + 1);
            let (t, n) = (l[..at].trim(), &l[at..]);
            if !word(n) {
                return Err(format!("not a name for the result: {l}"));
            }
            (if t.is_empty() { None } else { Some(t.split_whitespace().collect::<Vec<_>>().join(" ")) }, Some(n.to_string()))
        }
    };
    let inner = &s[open + 1..close];
    let args = if inner.trim().is_empty() { Vec::new() } else { split(inner).into_iter().map(String::from).collect() };
    Ok(Statement::Call { tid, ty, name, call: call.to_string(), args })
}

/// A C literal's item: a string, {…}, or an expression.
pub(super) fn literal(v: &str) -> Result<Item, String> {
    Ok(match v.strip_prefix('{').and_then(|r| r.strip_suffix('}')) {
        Some(inner) => Item::List(items(inner)?),
        None if v.starts_with('"') => Item::Str(unquote(v)?),
        None => Item::Expr(v.to_string()),
    })
}
