// SPDX-License-Identifier: GPL-2.0-only
/*
 * 128-bit multiply, which clang emits as a libcall on wasm32 (the kernel
 * does not link compiler-rt). Uses only 64-bit arithmetic.
 */
#include <linux/types.h>

typedef union {
	__int128 ti;
	struct {
		u64 low;
		u64 high;
	} s;
} twords;

__int128 __multi3(__int128 a, __int128 b);

__int128 __multi3(__int128 a, __int128 b)
{
	twords x = { .ti = a }, y = { .ti = b }, r;
	u64 a0 = (u32)x.s.low, a1 = x.s.low >> 32;
	u64 b0 = (u32)y.s.low, b1 = y.s.low >> 32;
	u64 p00 = a0 * b0, p01 = a0 * b1, p10 = a1 * b0, p11 = a1 * b1;
	u64 mid = (p00 >> 32) + (u32)p01 + (u32)p10;

	r.s.low = (mid << 32) | (u32)p00;
	r.s.high = p11 + (p01 >> 32) + (p10 >> 32) + (mid >> 32) +
		   x.s.high * y.s.low + x.s.low * y.s.high;
	return r.ti;
}
