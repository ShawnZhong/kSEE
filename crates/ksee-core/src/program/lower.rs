//! A program's line lowered (see Machine::make): checked, with the names
//! the lines before it made: a variable (its bytes, as it starts), memory
//! written, or a syscall (its number, its arguments laid out, its result's
//! name and type).

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use super::eval::{Env, Eval};
use super::layout::{Arg, Decl};
use super::parse::{statement, Expr, Statement};
use crate::types::Types;

/// What a line of the program makes, checked.
#[derive(Debug)]
pub enum Lowered {
    Decl(Decl),
    /// (`result`: the name its result is declared with, and what a pointer
    /// of its type points to, a type of the kernel's; `named`: the variable
    /// each argument names, if it is one, and what its parameter points to.)
    Call { nr: u32, args: Vec<Arg>, result: Option<(String, Option<u32>)>, named: Vec<Option<(String, Option<u32>)>> },
}

/// Line `text` of the program, checked and lowered with the names `env`
/// sees. Task selection belongs to the runtime boundary.
pub fn lower(text: &str, types: &Types, env: &dyn Env) -> Result<Lowered, String> {
    let line = statement(text)?;
    Ok(match line {
        Statement::Decl { .. } => Lowered::Decl(types.declare(&line, env)?),
        Statement::Call { result, call, args } => {
            let nr = types.syscalls.iter().find(|(_, n)| **n == call).map(|(&nr, _)| nr)
                .ok_or_else(|| {
                    let like: Vec<&str> = types.syscalls.values().filter(|n| n.starts_with(call.as_str())).take(3).copied().collect();
                    format!("this kernel has no syscall {call}{}", if like.is_empty() { String::new() } else { format!(": {}?", like.join(", ")) })
                })?;
            let params = types.syscall_params(&call).unwrap_or_default();
            if args.len() > params.len() {
                return Err(format!("{call}: {} arguments, not {}", params.len(), args.len()));
            }
            let points_to = |k: usize| params.get(k).and_then(|p| match types.get(types.resolve(p.1)).kind {
                crate::types::Kind::Pointer(to) => Some(to),
                _ => None,
            });
            let named = args.iter().enumerate().map(|(k, a)| match a { Expr::Name(n) => Some((n.clone(), points_to(k))), _ => None }).collect();
            let args = args.iter().enumerate().map(|(k, a)| types.arg(a, params.get(k).map_or(0, |p| p.1), env))
                .collect::<Result<Vec<_>, _>>()?;
            let result = match result {
                Some((ty, name)) => {
                    let (_, to) = Eval { types: Some(types), env }.type_of(&ty).map_err(|_| format!("{name}: no type {} in the kernel", ty.text()))?;
                    Some((name, to))
                }
                None => None,
            };
            Lowered::Call { nr, args, result, named }
        }
    })
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
