//! The page's programs: a line each, one of three statements, of C
//! expressions:
//!
//! ```text
//! [runner:] [type] [name =] call(args)   a syscall (its result named)
//! [runner:] type name[n] [= init]        a variable of the program's
//! [runner:] target = value               memory of the program's written
//! ```
//!
//! An expression is C's (operators, precedence, `.` `->` `[]` `*` `&`,
//! casts, `sizeof`), over typed names: a variable is memory of its type, a
//! result a value of its declared type, a constant the kernel's. An
//! argument or a value may also be a string ("a buffer with it") or `{…}`
//! (laid out as the parameter's type, or the variable's, says).
//!
//! [`lex`] and [`parse`]: the text as C's tokens, then a statement (a
//! syntax tree: any other syntax would make the same); [`eval`]: its
//! expressions' values; [`layout`]: values laid out in the kernel's types,
//! and read back. [`source`]: names in the kernel's own source, for hovers.

mod eval;
mod layout;
mod lex;
mod parse;
mod source;
#[cfg(test)]
mod tests;

pub use eval::{lvalue, typed, value, Env, Sym};
pub use layout::{Arg, Decl, Value};
pub use parse::{context, expression, lines, statement, Context, Expr, Line, Statement, TypeName};
pub use source::{names, Name};

/// A C expression's value, of numbers and names only (no types, no
/// memory): `O_CREAT | O_WRONLY`, `(1U << 16)`, `-100`.
pub fn eval(s: &str, known: &dyn Env) -> Result<i64, alloc::string::String> {
    value(s, None, known)
}
