// SPDX-License-Identifier: GPL-2.0-only
/*
 * Delays spin on the host's clock (one thread: nothing to sleep on; there
 * is no cycle counter).
 */
#include <linux/delay.h>
#include <linux/math64.h>
#include <asm/wasm.h>

void __ndelay(unsigned long nsecs)
{
	u64 end = wasm_time_ns() + nsecs;

	while (wasm_time_ns() < end)
		cpu_relax();
}

void __udelay(unsigned long usecs)
{
	__ndelay(usecs * NSEC_PER_USEC);
}

/* xloops is usecs * 2^32 / 10^6 (see asm-generic/delay.h). */
void __const_udelay(unsigned long xloops)
{
	__ndelay(((u64)xloops * NSEC_PER_SEC) >> 32);
}
