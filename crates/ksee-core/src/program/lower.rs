//! A program's line lowered (see Machine::make): checked, with the names
//! the lines before it made: a variable (its bytes, as it starts), memory
//! written, or a syscall (its number, its arguments laid out, its result's
//! name and type).

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use super::eval::{lvalue, typed, value, Env};
use super::layout::{Arg, Decl};
use super::parse::{expression, statement, Line};
use crate::types::Types;

/// What a line of the program makes, checked.
#[derive(Debug)]
pub enum Lowered {
    Decl(Decl),
    Assign { addr: u32, bytes: Vec<u8> },
    Call { nr: u32, args: Vec<Arg>, result: Option<(String, String)> },
}

/// Line `text` as line `line` of the program: its runner (a thread id, or a
/// result's name; else `main`, the program's own), and what it makes, with
/// the names `env` sees. `runners`: the thread ids that can make it ("no
/// runner", if its is not one).
pub fn lower(text: &str, types: &Types, env: &dyn Env, main: Option<u32>, runners: &[u32]) -> Result<(u32, Lowered), String> {
    let st = statement(text)?;
    let tid = match &st.runner {
        None => main.ok_or("the program is not ready")?,
        Some(r) => value(&expression(r)?, Some(types), env)? as u32,
    };
    if !runners.contains(&tid) {
        return Err(format!("no runner {tid}: the runners are {runners:?}"));
    }
    Ok((tid, match st.line {
        Line::Decl { .. } => Lowered::Decl(types.declare(&st.line, env)?),
        Line::Assign { target, value } => {
            // (As a declaration's value: an expression, "…" or {…}; an array's, whole.)
            let (addr, ty, count) = lvalue(&target, types, env)?;
            Lowered::Assign { addr, bytes: types.assign(ty, count, &value, env)? }
        }
        Line::Call { result, call, args, texts } => {
            let nr = types.syscalls.iter().find(|(_, n)| **n == call).map(|(&nr, _)| nr)
                .ok_or_else(|| {
                    let like: Vec<&str> = types.syscalls.values().filter(|n| n.starts_with(call.as_str())).take(3).map(String::as_str).collect();
                    format!("this kernel has no syscall {call}{}", if like.is_empty() { String::new() } else { format!(": {}?", like.join(", ")) })
                })?;
            let params = types.syscall_params(&call).cloned().unwrap_or_default();
            if texts.len() > params.len() {
                return Err(format!("{call}: {} arguments, not {}", params.len(), texts.len()));
            }
            let args = args.iter().enumerate().map(|(k, a)| types.arg(a, params.get(k).map_or(0, |p| p.1), env))
                .collect::<Result<Vec<_>, _>>()?;
            let result = match result {
                Some((ty, name)) => {
                    typed(0, &ty.text(), types).ok_or_else(|| format!("{name}: no type {} in the kernel", ty.text()))?;
                    Some((name, ty.text()))
                }
                None => None,
            };
            Lowered::Call { nr, args, result }
        }
    }))
}

/// How many words a call is made with (the syscall ABI's six arguments).
const MAX_ARGS: usize = 6;

/// A call's `args` as the words it is made with: a number as it is; a
/// buffer (bytes, a string's with its NUL; an array of strings, argv's) put
/// in the program's memory by `put` (as its variables are: there for as
/// long as the program), its address.
pub fn words(args: &[Arg], put: &mut impl FnMut(&[u8]) -> Result<u32, String>) -> Result<[u32; MAX_ARGS], String> {
    let mut words = [0u32; MAX_ARGS];
    for (w, a) in words.iter_mut().zip(args) {
        *w = match a {
            Arg::Number(v) => *v as u32,
            Arg::Bytes(b) => put(b)?,
            Arg::Strings(list) => {
                let pointers = list.iter().map(|s| s.as_ref().map_or(Ok(0), |s| put(&[s.as_bytes(), &[0]].concat())))
                    .collect::<Result<Vec<u32>, String>>()?;
                put(&pointers.iter().flat_map(|w| w.to_le_bytes()).collect::<Vec<u8>>())?
            }
        };
    }
    Ok(words)
}
