//! What fits at the cursor of a program's line being written (see
//! parse::context): a syscall's name, or, in an argument, a name of the
//! program's or a constant of the kernel's.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use super::{Context, Sym};
use crate::types::Types;

/// A completion: `text` to put in (for a syscall's name, from the
/// context's call_start; else from its word_start), shown as `label` and
/// `detail`.
#[derive(Debug, Clone, PartialEq)]
pub struct Completion {
    pub text: String,
    pub label: String,
    pub detail: String,
    pub name: bool,
}

/// What fits `c`: a syscall's name, those in `common` first, then those
/// starting with what is written, then those with it inside, each with its
/// signature; in an argument, the names `names` has (a result, a variable),
/// then the constants starting with the word, those that fit the
/// parameter first (arch::SYSCALL_CONSTS).
pub fn completions(
    types: &Types,
    c: &Context,
    names: &[(&str, &Sym)],
    common: &[String],
) -> Vec<Completion> {
    let Some(param) = c.param else {
        let w = c.call.as_str();
        let all: Vec<&String> = types.syscalls.values().collect();
        let mut out: Vec<&str> = Vec::new();
        let starts = all.iter().filter(|n| n.starts_with(w)).map(|n| n.as_str());
        let inside = all
            .iter()
            .filter(|n| !w.is_empty() && !n.starts_with(w) && n.contains(w))
            .map(|n| n.as_str());
        for n in common
            .iter()
            .map(String::as_str)
            .filter(|n| n.starts_with(w))
            .chain(starts)
            .chain(inside)
        {
            if !out.contains(&n) && all.iter().any(|a| a.as_str() == n) {
                out.push(n);
            }
        }
        return out
            .into_iter()
            .map(|n| Completion {
                text: format!("{n}("),
                label: n.into(),
                detail: types.signature(n).unwrap_or_default(),
                name: true,
            })
            .collect();
    };
    let w = c.word.as_str();
    if w.is_empty() || w.starts_with(|ch: char| ch.is_ascii_digit()) {
        return Vec::new();
    }
    let item = |label: &str, detail: String| Completion {
        text: label.into(),
        label: label.into(),
        detail,
        name: false,
    };
    let mut out: Vec<Completion> = names
        .iter()
        .filter(|(n, _)| n.starts_with(w))
        .map(|&(n, sym)| {
            item(
                n,
                match *sym {
                    Sym::Var { .. } => "a variable".into(),
                    Sym::Value { v, to: Some(_) } => format!("= {v:#x}"),
                    Sym::Value { v, to: None } => format!("= {v}"),
                },
            )
        })
        .collect();
    let pname = types
        .syscall_params(&c.call)
        .and_then(|p| p.get(param))
        .map(|p| p.0.as_str())
        .unwrap_or("");
    let fits = crate::arch::consts_of(&c.call, pname);
    let mut consts: Vec<(&String, i64)> = types
        .constants
        .iter()
        .filter(|(n, _)| n.starts_with(w))
        .map(|(n, &v)| (n, v))
        .collect();
    consts.sort_by_key(|(n, _)| !fits.iter().any(|f| n.starts_with(f)));
    out.extend(consts.into_iter().map(|(n, v)| item(n, v.to_string())));
    if "NULL".starts_with(w) {
        out.push(item("NULL", "0".into()));
    }
    out
}
