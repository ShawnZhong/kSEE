//! The programs' text and values, on made-up types.

use super::*;
use crate::types::Types;
use crate::Memory;
use alloc::string::ToString;
use alloc::vec;
use alloc::vec::Vec;

const TYPES: &str = "T\t1\tbase\tlong long\t8\ts\nT\t2\tstruct\ttimespec\t16\nM\t2\ttv_sec\t0\t0\t1\nM\t2\ttv_nsec\t64\t0\t1\n\
                     T\t3\tptr\t\t4\t2\nT\t4\tbase\tchar\t1\tc\nT\t5\tptr\t\t4\t4\nT\t6\tptr\t\t4\t5\nT\t7\tbase\tint\t4\ts\n\
                     T\t8\tarray\t\t8\t4\t8\nT\t9\tstruct\tnamed\t12\nM\t9\tn\t0\t0\t7\nM\t9\ts\t32\t0\t8\nT\t10\tptr\t\t4\t9\n\
                     T\t11\tarray\t\t8\t7\t2\nT\t12\tstruct\tpair\t8\nM\t12\tv\t0\t0\t11\nT\t13\tptr\t\t4\t12\n";

fn known(name: &str) -> Option<i64> {
    [("O_CREAT", 64), ("O_WRONLY", 1), ("AT_FDCWD", -100), ("fd", 3), ("$2", 5)].iter().find(|(n, _)| *n == name).map(|(_, v)| *v)
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
    let call = |t: &str| match statement(t).unwrap() {
        Statement::Call { tid, ty, name, call, args } => (tid, ty, name, call, args),
        s => panic!("{s:?}"),
    };
    let o = |s: &str| Some(s.to_string());
    assert_eq!(call("int fd = openat(AT_FDCWD, \"/a, b\", O_RDONLY);"),
               (None, o("int"), o("fd"), "openat".into(), alloc::vec!["AT_FDCWD".into(), "\"/a, b\"".into(), "O_RDONLY".into()]));
    assert_eq!(call("child: void *mem = mmap2(NULL, {1, 2}, 3)"), (o("child"), o("void *"), o("mem"), "mmap2".into(), alloc::vec!["NULL".into(), "{1, 2}".into(), "3".into()]));
    assert_eq!(call("fd = getpid()"), (None, None, o("fd"), "getpid".into(), alloc::vec![]));
    assert_eq!(statement("char buf[64]").unwrap(), Statement::Decl { tid: None, text: "char buf[64]".into() });
    assert_eq!(statement("write(1, \"hi)\", 2").unwrap_err(), "a ) missing");
    assert_eq!(statement("19: struct __kernel_timespec ts = {0, 1}").unwrap(), Statement::Decl { tid: o("19"), text: "struct __kernel_timespec ts = {0, 1}".into() });
}

#[test]
fn declarations_are_a_programs_memory() {
    let t = Types::parse(TYPES);
    let d = t.declare("struct timespec ts = {0, 30000000}", &known).unwrap();
    assert_eq!((d.name.as_str(), d.ty, d.count, d.bytes.len()), ("ts", 2, None, 16));
    assert_eq!(&d.bytes[8..12], &30000000u32.to_le_bytes());
    let buf = t.declare("char buf[64]", &known).unwrap();
    assert_eq!((buf.ty, buf.count, buf.bytes.len()), (4, Some(64), 64));
    let fds = t.declare("int fds[2] = {3, 4}", &known).unwrap();
    assert_eq!(fds.bytes, [3, 0, 0, 0, 4, 0, 0, 0]);
    assert_eq!(t.declare("char path[] = \"/x\"", &known).unwrap().bytes, b"/x\0");
    assert!(t.declare("struct nope x", &known).is_err() && t.declare("char *p", &known).is_err());
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
    assert_eq!(t.assign(7, Some(2), "{5, 6}", &known).unwrap(), [5, 0, 0, 0, 6, 0, 0, 0]);
    // (Past a NUL too, escaped, as C writes it.)
    assert_eq!(t.show(&M(b"a\0b\x05\0\0".to_vec()), 0, 4, Some(6)), "\"a\\0b\\x05\"");
}

#[test]
fn expressions() {
    assert_eq!(eval("O_CREAT | O_WRONLY", &known), Ok(65));
    assert_eq!(eval("0644", &known), Ok(420));
    assert_eq!(eval("(1 << 4) + fd - $2", &known), Ok(14));
    assert_eq!((eval("'h'", &known), eval("'\\n'", &known), eval("'\\0' + 1", &known)), (Ok(104), Ok(10), Ok(1)));
    assert_eq!(eval("fds[1] + &fd", &|n| [("fds[1]", 4), ("fd", 3)].iter().find(|(k, _)| *k == n).map(|(_, v)| *v)), Ok(7));
    assert_eq!(eval("NULL", &known), Ok(0));
    assert!(eval("nope", &known).is_err());
    assert!(eval("1 2", &known).is_err());
}

#[test]
fn a_struct_is_laid_out_by_its_type() {
    let t = Types::parse(TYPES);
    let ts = |text| t.arg(text, 3, &known);
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
    assert_eq!(t.arg("{7, \"abc\"}", 10, &known), Ok(Arg::Bytes(named)));
    assert_eq!(t.arg("{{1, 2}}", 13, &known), Ok(Arg::Bytes([1u32.to_le_bytes(), 2u32.to_le_bytes()].concat())));
}

#[test]
fn strings_and_argv() {
    let t = Types::parse(TYPES);
    assert_eq!(t.arg("\"a\\n\"", 5, &known), Ok(Arg::Bytes(b"a\n\0".to_vec())));
    // (An escape's byte, not its character's UTF-8: \xff is one byte.)
    assert_eq!(t.arg("\"\\xff\"", 5, &known), Ok(Arg::Bytes(alloc::vec![0xff, 0])));
    assert_eq!(t.arg("{\"init\", NULL}", 6, &known), Ok(Arg::Strings(vec![Some("init".into()), None])));
    assert_eq!(t.arg("fd", 7, &known), Ok(Arg::Number(3)));
    // (No type known: no layout.)
    assert!(t.arg("{1, 2}", 0, &known).is_err());
    assert!(t.arg("{1}", 7, &known).unwrap_err().contains("not a pointer"));
}
