//! The page's programs: a line each, one of three statements, of C
//! expressions:
//!
//! ```text
//! [type name =] call(args)     a syscall (its result declared)
//! type name[n] [= init]        a variable of the program's
//! ```
//!
//! An expression is a small part of C's (`| ^ & << >> + - * /`, unary
//! `- ~ &`, `[]`, C's precedence), over typed names: a variable is memory of its type, a
//! result a value of its declared type, a constant the kernel's. An
//! argument or a value may also be a string ("a buffer with it") or `{…}`
//! (laid out as the parameter's type, or the variable's, says).
//!
//! [`lex`] and [`parse`]: the text as C's tokens, then a statement (a
//! syntax tree: any other syntax would make the same); [`eval`]: its
//! expressions' values; [`complete`]: what fits at a cursor; [`names`]: what the lines before a line named;
//! [`layout`]: values laid out in the kernel's types, and read back.

mod complete;
mod eval;
mod layout;
mod lex;
mod lower;
mod names;
mod parse;
#[cfg(test)]
mod tests;

pub use complete::{completions, Completion};
pub use eval::{value, Env, Sym};
pub use layout::{Arg, Decl};
pub use lower::{lower, words, Lowered};
pub use names::Names;
pub use parse::{context, expression, statement, Line};
pub use parse::{Context, Expr, TypeName};
