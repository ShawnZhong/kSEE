// kSEE's additions to nolibc (linux/tools/include/nolibc, the kernel's
// own small C library) for the example programs in user/examples/. The build
// includes this first (-include), so the programs are plain C with the
// usual #include <stdio.h> etc. (nolibc's headers, already included here).
//
// Each C library call that makes a syscall first reports its source line
// (the ksee.line import: a record in the kernel's trace), so the page can
// show which line of the program the kernel is working for. The macros
// wrap nolibc's functions of the same names (a function-like macro does
// not expand inside itself).
#pragma once

#include <nolibc.h>

__attribute__((import_module("ksee"), import_name("line")))
void __ksee_line(int line);

#define __KSEE(call) (__ksee_line(__LINE__), (call))

#define open(...)	__KSEE(open(__VA_ARGS__))
#define read(...)	__KSEE(read(__VA_ARGS__))
#define write(...)	__KSEE(write(__VA_ARGS__))
#define close(...)	__KSEE(close(__VA_ARGS__))
#define fsync(...)	__KSEE(fsync(__VA_ARGS__))
#define dup(...)	__KSEE(dup(__VA_ARGS__))
#define dup2(...)	__KSEE(dup2(__VA_ARGS__))
#define pipe(...)	__KSEE(pipe(__VA_ARGS__))
#define pipe2(...)	__KSEE(pipe2(__VA_ARGS__))
#define lseek(...)	__KSEE(lseek(__VA_ARGS__))
#define mkdir(...)	__KSEE(mkdir(__VA_ARGS__))
#define unlink(...)	__KSEE(unlink(__VA_ARGS__))
#define getpid(...)	__KSEE(getpid(__VA_ARGS__))
#define nanosleep(...)	__KSEE(nanosleep(__VA_ARGS__))
#define stat(...)	__KSEE(stat(__VA_ARGS__))
#define printf(...)	__KSEE(printf(__VA_ARGS__))
#define puts(...)	__KSEE(puts(__VA_ARGS__))

// 128-bit multiply, which clang emits as a libcall on wasm32 (for 64-bit
// division by constants, as in printf); there is no compiler-rt.
__attribute__((weak)) __int128 __multi3(__int128 a, __int128 b)
{
	union { __int128 ti; struct { unsigned long long low, high; } s; } x = { a }, y = { b }, r;
	unsigned long long a0 = (unsigned)x.s.low, a1 = x.s.low >> 32;
	unsigned long long b0 = (unsigned)y.s.low, b1 = y.s.low >> 32;
	unsigned long long p00 = a0 * b0, p01 = a0 * b1, p10 = a1 * b0, p11 = a1 * b1;
	unsigned long long mid = (p00 >> 32) + (unsigned)p01 + (unsigned)p10;

	r.s.low = (mid << 32) | (unsigned)p00;
	r.s.high = p11 + (p01 >> 32) + (p10 >> 32) + (mid >> 32) + x.s.high * y.s.low + x.s.low * y.s.high;
	return r.ti;
}
