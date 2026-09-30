//! Names in a line of the kernel's C source, for the page's hovers (not
//! the program's language: see parse).

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

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
