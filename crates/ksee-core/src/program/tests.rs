//! The programs' text and values, on made-up types.

use super::*;
use crate::types::Types;
use crate::Memory;
use alloc::string::ToString;
use alloc::vec;
use alloc::boxed::Box;
use alloc::vec::Vec;

const TYPES: &str = "T\t1\tbase\tlong long\t8\ts\nT\t2\tstruct\ttimespec\t16\nM\t2\ttv_sec\t0\t0\t1\nM\t2\ttv_nsec\t64\t0\t1\n\
                     T\t3\tptr\t\t4\t2\nT\t4\tbase\tchar\t1\tc\nT\t5\tptr\t\t4\t4\nT\t6\tptr\t\t4\t5\nT\t7\tbase\tint\t4\ts\n\
                     T\t8\tarray\t\t8\t4\t8\nT\t9\tstruct\tnamed\t12\nM\t9\tn\t0\t0\t7\nM\t9\ts\t32\t0\t8\nT\t10\tptr\t\t4\t9\n\
                     T\t11\tarray\t\t8\t7\t2\nT\t12\tstruct\tpair\t8\nM\t12\tv\t0\t0\t11\nT\t13\tptr\t\t4\t12\n";

/// Declaration `text`, in types `t`.
fn decl(t: &Types, text: &str, known: &dyn Env) -> Result<Decl, alloc::string::String> {
    t.declare(&statement(text)?.line, known)
}

fn known(name: &str) -> Option<i64> {
    [("O_CREAT", 64), ("O_WRONLY", 1), ("AT_FDCWD", -100), ("fd", 3), ("n", 5)].iter().find(|(n, _)| *n == name).map(|(_, v)| *v)
}

#[test]
fn a_lines_names_with_their_fields() {
    let got: Vec<_> = names("if (file->f_mode & FMODE_READ) x = a.b->c; // é 12").into_iter().map(|n| (n.expr, n.start, n.at, n.member)).collect();
    assert_eq!(got, [("file".into(), 4, 4, false), ("file->f_mode".into(), 4, 10, true), ("FMODE_READ".into(), 19, 19, false),
                     ("x".into(), 31, 31, false), ("a".into(), 35, 35, false), ("a.b".into(), 35, 37, true), ("a.b->c".into(), 35, 40, true)]);
    // (A field's: the expression it is a field of.)
    let parents: Vec<_> = names("a.b->c = x").into_iter().map(|n| n.parent).collect();
    assert_eq!(parents, [None, Some("a".into()), Some("a.b".into()), None]);
}

#[test]
fn a_line_is_a_call_or_a_declaration() {
    let o = |s: &str| Some(s.to_string());
    let call = |t: &str| match statement(t).unwrap() {
        Statement { runner, line: Line::Call { result, call, texts, .. }, .. } => (runner, result.clone().map(|(t, _)| t.text()), result.map(|(_, n)| n), call, texts),
        s => panic!("{s:?}"),
    };
    assert_eq!(call("int fd = openat(AT_FDCWD, \"/a, b\", O_RDONLY);"),
               (None, o("int"), o("fd"), "openat".into(), vec!["AT_FDCWD".into(), "\"/a, b\"".into(), "O_RDONLY".into()]));
    assert_eq!(call("child: void *mem = mmap2(NULL, {1, 2}, 3)"), (o("child"), o("void *"), o("mem"), "mmap2".into(), vec!["NULL".into(), "{1, 2}".into(), "3".into()]));
    assert_eq!(call("getpid()"), (None, None, None, "getpid".into(), vec![]));
    assert!(statement("fd = getpid()").is_err()); // (a result: declared)
    // (Its text, as written, but for its runner.)
    assert_eq!(statement("child: close(fd);").unwrap().text, "close(fd)");
    let decl = |t: &str| match statement(t).unwrap() {
        Statement { runner, line: Line::Decl { ty, name, count, .. }, .. } => (runner, ty.text(), name, count.is_some()),
        s => panic!("{s:?}"),
    };
    assert_eq!(decl("char buf[64]"), (None, "char".into(), "buf".into(), true));
    assert_eq!(decl("19: struct __kernel_timespec ts = {0, 1}"), (o("19"), "struct __kernel_timespec".into(), "ts".into(), false));
    assert_eq!(decl("unsigned long long n"), (None, "unsigned long long".into(), "n".into(), false));
    assert!(statement("char **argv").is_err());
    assert!(statement("write(1, \"hi)\", 2").is_err());
    // (Assignments: memory of the program's written.)
    let assign = |t: &str| match statement(t).unwrap() {
        Statement { line: Line::Assign { target, value }, .. } => (target, value),
        s => panic!("{s:?}"),
    };
    let n = |s: &str| Expr::Name(s.into());
    assert_eq!(assign("rings->sq.tail = 1;"), (Expr::Member(Box::new(Expr::Member(Box::new(n("rings")), "sq".into(), true)), "tail".into(), false), Expr::Num(1)));
    assert_eq!(assign("fds[0] = 3").0, Expr::Index(Box::new(n("fds")), Box::new(Expr::Num(0))));
    assert_eq!(assign("x = y + 2").1, Expr::Binary("+", Box::new(n("y")), Box::new(Expr::Num(2))));
    assert!(matches!(assign("*sqe = {.opcode = IORING_OP_READ, .fd = fd}"), (Expr::Unary("*", _), Expr::Init(l)) if l.len() == 2));
}

#[test]
fn declarations_are_a_programs_memory() {
    let t = Types::parse(TYPES);
    let d = decl(&t, "struct timespec ts = {0, 30000000}", &known).unwrap();
    assert_eq!((d.name.as_str(), d.ty, d.count, d.bytes.len()), ("ts", 2, None, 16));
    assert_eq!(&d.bytes[8..12], &30000000u32.to_le_bytes());
    let buf = decl(&t, "char buf[64]", &known).unwrap();
    assert_eq!((buf.ty, buf.count, buf.bytes.len()), (4, Some(64), 64));
    let fds = decl(&t, "int fds[2] = {3, 4}", &known).unwrap();
    assert_eq!(fds.bytes, [3, 0, 0, 0, 4, 0, 0, 0]);
    assert_eq!(decl(&t, "char path[] = \"/x\"", &known).unwrap().bytes, b"/x\0");
    assert!(decl(&t, "struct nope x", &known).is_err());
    // (A pointer: not a variable; a result's, or a variable's address.)
    assert!(decl(&t, "char *p = 0x40", &known).is_err());
    // Said as C writes it.
    struct M(Vec<u8>);
    impl Memory for M {
        fn read(&self, addr: u32, buf: &mut [u8]) {
            buf.copy_from_slice(&self.0[addr as usize..addr as usize + buf.len()]);
        }
    }
    let mem = M([fds.bytes.clone(), d.bytes.clone(), b"hi\0".to_vec()].concat());
    assert_eq!(t.show(&mem, 0, 7, Some(2)), "{3, 4}");
    assert_eq!(t.show(&mem, 8, 2, None), "{tv_sec = 0, tv_nsec = 30000000}");
    assert_eq!(t.show(&mem, 24, 4, Some(3)), "\"hi\"");
    // As data (for JavaScript), and assigned from a literal.
    assert_eq!(t.value(&mem, 0, 7, Some(2)), Value::List(alloc::vec![Value::Int(3), Value::Int(4)]));
    assert_eq!(t.value(&mem, 8, 2, None), Value::Fields(alloc::vec![("tv_sec".into(), Value::Int(0)), ("tv_nsec".into(), Value::Int(30000000))]));
    assert_eq!(t.value(&mem, 24, 4, Some(3)), Value::Text("hi".into()));
    assert_eq!(t.assign(7, Some(2), &expression("{5, 6}").unwrap(), &known).unwrap(), [5, 0, 0, 0, 6, 0, 0, 0]);
    // (Past a NUL too, escaped, as C writes it.)
    assert_eq!(t.show(&M(b"a\0b\x05\0\0".to_vec()), 0, 4, Some(6)), "\"a\\0b\\x05\"");
}

#[test]
fn expressions() {
    let eval = |s: &str, k: &dyn Env| expression(s).and_then(|e| value(&e, None, k));
    assert_eq!(eval("O_CREAT | O_WRONLY", &known), Ok(65));
    assert_eq!(eval("0644", &known), Ok(420));
    assert_eq!(eval("(1 << 4) + fd - n", &known), Ok(14));
    assert_eq!(eval("NULL", &known), Ok(0));
    assert!(eval("nope", &known).is_err());
    assert!(eval("1 2", &known).is_err());
}

/// A program's names and memory, made up.
struct Prog(Vec<(&'static str, Sym)>, Vec<u8>);
impl Env for Prog {
    fn sym(&self, name: &str) -> Option<Sym> {
        self.0.iter().find(|(n, _)| *n == name).map(|(_, s)| s.clone())
    }
    fn memory(&self) -> Option<&dyn Memory> {
        Some(&self.1 as &dyn Memory)
    }
}
impl Memory for Vec<u8> {
    fn read(&self, addr: u32, buf: &mut [u8]) {
        buf.copy_from_slice(&self[addr as usize..addr as usize + buf.len()]);
    }
}

#[test]
fn expressions_as_c_reads_them() {
    // struct pt { int x; int y; } p at 8 (3, 4); int fds[2] at 16 (5, 6);
    // q, a result: struct pt *, 8.
    let t = Types::parse("T\t1\tbase\tint\t4\ts\nT\t2\tstruct\tpt\t8\nM\t2\tx\t0\t0\t1\nM\t2\ty\t32\t0\t1\n\
                          T\t4\tptr\t\t4\t2\nT\t5\tbase\tunsigned int\t4\tu\n");
    let mut m = vec![0u8; 32];
    for (at, v) in [(8, 3), (12, 4), (16, 5), (20, 6)] {
        m[at] = v;
    }
    let env = Prog(vec![("p", Sym::Var { addr: 8, ty: 2, count: None }), ("fds", Sym::Var { addr: 16, ty: 1, count: Some(2) }),
                        ("q", Sym::Value { v: 8, to: Some(2) })], m);
    let v = |s| value(&expression(s).unwrap(), Some(&t), &env);
    let lv = |s| lvalue(&expression(s).unwrap(), &t, &env);
    assert_eq!((v("p.y"), v("fds[1]"), v("q->x + q[0].y"), v("&p"), v("fds")), (Ok(4), Ok(6), Ok(7), Ok(8), Ok(16)));
    // (Pointer arithmetic: by the size pointed to.)
    assert_eq!((v("*(fds + 1)"), v("(q + 1)->x")), (Ok(6), Ok(5)));
    assert!(v("p").is_err()); // (a struct: its address, or a member)
    // (Memory to write: where, of which type.)
    assert_eq!((lv("fds[1]"), lv("q->y"), lv("*(fds + 1)")), (Ok((20, 1, None)), Ok((12, 1, None)), Ok((20, 1, None))));
    assert_eq!(lv("fds"), Ok((16, 1, Some(2)))); // (an array, whole: as a declaration's value)
}

#[test]
fn a_struct_is_laid_out_by_its_type() {
    let t = Types::parse(TYPES);
    let ts = |text| t.arg(&expression(text).unwrap(), 3, &known);
    let want = Arg::Bytes([0u64.to_le_bytes(), 30000000u64.to_le_bytes()].concat());
    assert_eq!(ts("{0, 30000000}"), Ok(want.clone()));
    assert_eq!(ts("{.tv_nsec = 30000000}"), Ok(want));
    assert_eq!(ts("{.tv_sec = 1}"), Ok(Arg::Bytes([1u64.to_le_bytes(), [0; 8]].concat())));
    assert!(ts("{.tv_min = 1}").unwrap_err().contains("no member"));
    assert!(ts("{1, 2, 3}").unwrap_err().contains("too many"));
    // A struct with an int and a char array; one with an array member.
    let mut named = vec![0u8; 12];
    named[0] = 7;
    named[4..7].copy_from_slice(b"abc");
    assert_eq!(t.arg(&expression("{7, \"abc\"}").unwrap(), 10, &known), Ok(Arg::Bytes(named)));
    assert_eq!(t.arg(&expression("{{1, 2}}").unwrap(), 13, &known), Ok(Arg::Bytes([1u32.to_le_bytes(), 2u32.to_le_bytes()].concat())));
}

#[test]
fn strings_and_argv() {
    let t = Types::parse(TYPES);
    assert_eq!(t.arg(&expression("\"a\\n\"").unwrap(), 5, &known), Ok(Arg::Bytes(b"a\n\0".to_vec())));
    // (An escape's byte, not its character's UTF-8: \xff is one byte.)
    assert_eq!(t.arg(&expression("\"\\xff\"").unwrap(), 5, &known), Ok(Arg::Bytes(alloc::vec![0xff, 0])));
    assert_eq!(t.arg(&expression("{\"init\", NULL}").unwrap(), 6, &known), Ok(Arg::Strings(vec![Some("init".into()), None])));
    assert_eq!(t.arg(&expression("fd").unwrap(), 7, &known), Ok(Arg::Number(3)));
    // (No type known: no layout.)
    assert!(t.arg(&expression("{1, 2}").unwrap(), 0, &known).is_err());
    assert!(t.arg(&expression("{1}").unwrap(), 7, &known).unwrap_err().contains("not a pointer"));
}

#[test]
fn where_the_cursor_is_in_a_line_being_written() {
    let at = |t: &str| {
        let c = context(&t.replace('|', ""), t[..t.find('|').unwrap()].encode_utf16().count());
        (c.call, c.call_start, c.word, c.param)
    };
    // (The name, after a runner and a typed result.)
    assert_eq!(at("19: int fd = open|"), ("open".into(), 13, "open".into(), None));
    // (An argument: which, past strings and braces with commas in them.)
    assert_eq!(at("read(fd, b|"), ("read".into(), 0, "b".into(), Some(1)));
    assert_eq!(at("write(1, \"a,b\", {1, 2}, O_|)"), ("write".into(), 0, "O_".into(), Some(3)));
    assert_eq!(at("x = openat(|"), ("openat".into(), 4, "".into(), Some(0)));
    // (UTF-16 offsets: a character past the BMP counts two.)
    assert_eq!(at("write(1, \"😀\", n|"), ("write".into(), 0, "n".into(), Some(2)));
}

#[test]
fn a_program_on_one_line_is_its_lines() {
    assert_eq!(lines(r#"write(1, "a;b\";", 4);  close(1) ;"#), [r#"write(1, "a;b\";", 4)"#, "close(1)"]);
}

#[test]
fn a_line_sees_the_names_of_the_lines_before_it() {
    let mut n = Names::default();
    let v = |v| Sym::Value { v, to: None };
    n.set("fd", 0, v(3));
    n.set("buf", 1, Sym::Var { addr: 64, ty: 4, count: Some(8) });
    n.set("fd", 2, v(4));
    assert_eq!((n.get("fd", 0), n.get("fd", 2), n.get("fd", 3)), (None, Some(&v(3)), Some(&v(4))));
    // (A result set again, as its call returns: the same line's.)
    n.set("fd", 2, v(5));
    assert_eq!(n.before(3), [("fd", &v(5)), ("buf", &Sym::Var { addr: 64, ty: 4, count: Some(8) })]);
    assert!(n.has("fd", 2) && !n.has("fd", 1));
}
