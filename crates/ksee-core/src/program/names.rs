//! A program's names, as its lines make them: a variable (memory of the
//! program's), or a call's result once it has one; each by the line that
//! made it, so that a line sees only the names of the lines before it.

use alloc::string::String;
use alloc::vec::Vec;

use super::eval::Sym;

#[derive(Debug, Default, Clone)]
pub struct Names(Vec<(String, usize, Sym)>);

impl Names {
    /// `name`, made by line `line`, stands for `sym` (again: a result, as
    /// its call returns).
    pub fn set(&mut self, name: &str, line: usize, sym: Sym) {
        match self.0.iter_mut().find(|(n, l, _)| n == name && *l == line) {
            Some(e) => e.2 = sym,
            None => self.0.push((name.into(), line, sym)),
        }
    }

    /// Whether line `line` made `name` yet.
    pub fn has(&self, name: &str, line: usize) -> bool {
        self.0.iter().any(|(n, l, _)| n == name && *l == line)
    }

    /// What `name` stands for to line `line`: the last line before it
    /// that made it.
    pub fn get(&self, name: &str, line: usize) -> Option<&Sym> {
        self.0.iter().filter(|(n, l, _)| n == name && *l < line).max_by_key(|e| e.1).map(|e| &e.2)
    }

    /// The names line `line` sees, each once, as get has them.
    pub fn before(&self, line: usize) -> Vec<(&str, &Sym)> {
        let mut out: Vec<(&str, &Sym)> = Vec::new();
        for (n, _, _) in self.0.iter().filter(|e| e.1 < line) {
            if !out.iter().any(|(m, _)| m == n) {
                out.push((n, self.get(n, line).unwrap()));
            }
        }
        out
    }
}
