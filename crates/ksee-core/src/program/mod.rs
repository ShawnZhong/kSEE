//! A syscall's arguments, written as in C (the page's programs):
//!
//! ```text
//! "a string"              a buffer with the string, NUL-terminated
//! 0644, 0x41, -100        numbers (0: octal, 0x: hex)
//! O_CREAT | O_WRONLY      constants (the kernel's own: Types::constants),
//!                         NULL, names of results (fd, $2), with | << + -
//! {0, 30000000}           a compound literal, laid out as the parameter's
//! {.tv_nsec = 30000000}   type says (a struct, positional or by name;
//! {"init", NULL}          an array; for a char **, an array of strings)
//! ```
//!
//! [`text`]: reading C text (expressions, literals, a program's line, the
//! names in a line of source); [`layout`]: laying values out in the
//! kernel's types, and reading them back.

mod layout;
mod text;
#[cfg(test)]
mod tests;

pub use layout::{Arg, Decl, Value};
pub use text::{eval, names, statement, Name, Statement};
