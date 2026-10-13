//! What fits at the cursor of a program's line being written (see
//! parse::context): a syscall's name, or, in an argument, a name of the
//! program's or a constant of the kernel's.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use super::{Context, Sym};
use crate::types::Types;

/// A completion: `label` (and `detail`, shown beside it); a syscall's
/// name, if `call` (put in from the context's call_start; else from its
/// word_start).
#[derive(Debug, Clone, PartialEq)]
pub struct Completion {
    pub label: String,
    pub detail: String,
    pub call: bool,
}

impl Completion {
    /// What it puts in: a call's name and its "(", else the label.
    pub fn text(&self) -> String {
        if self.call { alloc::format!("{}(", self.label) } else { self.label.clone() }
    }
}

/// What fits `c`: a syscall's name, those in `common` first, then those
/// starting with what is written, then those with it inside, each with its
/// parameters; in an argument, the names `names` has (a result, a variable),
/// then the constants starting with the word, those that fit the
/// parameter first (arch::SYSCALL_CONSTS).
pub fn completions(types: &Types, c: &Context, names: &[(&str, &Sym)], common: &[String]) -> Vec<Completion> {
    let Some(param) = c.param else {
        let w = c.call.as_str();
        let all: Vec<&str> = types.syscalls.values().copied().collect();
        let starts = all.iter().copied().filter(|n| n.starts_with(w));
        let inside = all.iter().copied().filter(|n| !w.is_empty() && !n.starts_with(w) && n.contains(w));
        let mut out: Vec<&str> = Vec::new();
        for n in common.iter().map(String::as_str).filter(|n| n.starts_with(w)).chain(starts).chain(inside) {
            if !out.contains(&n) && all.contains(&n) {
                out.push(n);
            }
        }
        return out.into_iter().map(|n| Completion {
            label: n.into(),
            detail: types.params_of(n).map_or(String::new(), |ps| format!("({})", ps.join(", "))),
            call: true,
        }).collect();
    };
    let w = c.word.as_str();
    if w.is_empty() || w.starts_with(|ch: char| ch.is_ascii_digit()) {
        return Vec::new();
    }
    let item = |label: &str, detail: String| Completion { label: label.into(), detail, call: false };
    let mut out: Vec<Completion> = names.iter().filter(|(n, _)| n.starts_with(w)).map(|&(n, sym)| item(n, match *sym {
        Sym::Var { .. } => "a variable".into(),
        Sym::Value { v, to: Some(_) } => format!("= {v:#x}"),
        Sym::Value { v, to: None } => format!("= {v}"),
    })).collect();
    let pname = types.syscall_params(&c.call).and_then(|p| p.get(param).map(|p| p.0)).unwrap_or("");
    let fits = crate::arch::consts_of(&c.call, pname);
    let mut consts: Vec<(&str, i64)> = types.constants.iter().filter(|(n, _)| n.starts_with(w)).map(|(&n, &v)| (n, v)).collect();
    consts.sort_by_key(|(n, _)| !fits.iter().any(|f| n.starts_with(f)));
    out.extend(consts.into_iter().map(|(n, v)| item(n, v.to_string())));
    if "NULL".starts_with(w) {
        out.push(item("NULL", "0".into()));
    }
    out
}
