// SPDX-License-Identifier: GPL-2.0-only
/*
 * The trace: a ring of the kernel's records, and its writers. The kSEE
 * plugin's probes call them (a block, a return, a store, ...); the host
 * reads the ring (ksee_trace, a global the linker exports: ksee_core::trace),
 * says what to record (ksee_record), and
 * has its own writes logged (ksee_bulk). Not instrumented: the plugin leaves
 * __ksee_* alone.
 *
 * A record is its values, then its head [a, tag]: the tag's top nibble
 * says what it is (the plugin's header), the next how many values came
 * before; so a reader walks back from pos, a record's end.
 */

#include <linux/types.h>
#include <linux/unaligned.h>
#include <asm/current.h>
#include <asm/wasm.h>

/* (Called by the plugin's probes and the host only: no header has them.) */
void __ksee_put(u32 a, u32 tag, u32 n);
void __ksee_bulk(u32 addr, u32 len, u32 line, u32 from);
void __ksee_record(u32 level);

#define RING_WORDS (1u << 21)
#define COUNT 24
#define STORE (7u << 28)
#define TASK (3u << 28)

/*
 * pos counts the words ever written (the ring keeps the most recent),
 * stored with release order after each record, so a reader that loads it
 * sees whole records, from their end; words, the ring's size; on, what it
 * records (see ksee_record). One writer at a time: the kernel is
 * uniprocessor.
 */
struct ksee_ring {
	u32 pos, words, on;
	u32 ring[RING_WORDS];
};
extern struct ksee_ring ksee_trace;
struct ksee_ring ksee_trace __aligned(8) = { .words = RING_WORDS };


/*
 * The writers write nothing but the ring: their own writes (a frame on the
 * stack: an array, a local whose address is taken) would not be recorded,
 * and memory as it was would be wrong there. Registers only.
 */
#define RING(i) ksee_trace.ring[(i) & (RING_WORDS - 1)]

/* A value at p: where the next goes. */
static __always_inline u32 value(u32 p, u32 v)
{
	RING(p) = v;
	return p + 1;
}

/* The head [a, tag] at p, after the n values before it: the record's end. */
static __always_inline u32 head(u32 p, u32 a, u32 tag, u32 n)
{
	RING(p) = a;
	RING(p + 1) = tag | n << COUNT;
	return p + 2;
}

/* The record ending at p, written: a reader that loads pos sees it whole. */
static __always_inline void done(u32 p)
{
	__atomic_store_n(&ksee_trace.pos, p, __ATOMIC_RELEASE);
}

/* Every other record (a block, a return, a call's arguments): the n values
 * the probe put in __ksee_values just before, then [a, tag]; only while
 * everything is recorded (see ksee_record). At most VALUES: a call's seven
 * arguments, wide ones two words, and its frame base (the plugin's Values,
 * the same). */
#define VALUES (2 * 7 + 1)
u32 __ksee_values[VALUES];

void __ksee_put(u32 a, u32 tag, u32 n)
{
	u32 p, i;

	if (ksee_trace.on != 1)
		return;
	p = ksee_trace.pos;
	for (i = 0; i < n; i++)
		p = value(p, __ksee_values[i]);
	done(head(p, a, tag, n));
}

/* One Store record: 1 << k bytes at addr, on a line, with their old value
 * (before the write), low word first, and for a copy (from: not 0) where
 * from. (The old value is read with one load of a fixed size, never a
 * memcpy: the kernel's memcpy is instrumented, and would record itself.) */
static __always_inline void store(u32 k, u32 addr, u32 line, u32 from)
{
	const void *a = (const void *)(unsigned long)addr;
	u64 old = k == 0 ? *(const u8 *)a :
		  k == 1 ? get_unaligned((const u16 *)a) :
		  k == 2 ? get_unaligned((const u32 *)a) :
			   get_unaligned((const u64 *)a);
	u32 p = value(ksee_trace.pos, (u32)old);

	if (k == 3)
		p = value(p, (u32)(old >> 32));
	if (from)
		p = value(p, from);
	done(head(p, addr, STORE | line << 4 | k, (k == 3) + 1 + !!from));
}

/* Every write the probes see (a store, a memcpy, a memset) and the host's
 * own: len bytes at addr, as Store records of up to 8 bytes each (a store
 * of 1, 2, 4 or 8 bytes: one); from: where a copy takes them from, else 0.
 * A copy onto a source it overlaps from above goes backwards, so each
 * piece's source is still as it was (memmove's order). */
__wasm_export(ksee_bulk) void __ksee_bulk(u32 addr, u32 len, u32 line, u32 from)
{
	bool back = from && from < addr && addr - from < len;
	u32 off = back ? len : 0;

	if (!ksee_trace.on)
		return;
	while (back ? off : off < len) {
		u32 left = back ? off : len - off;
		u32 k = left >= 8 ? 3 : left >= 4 ? 2 : left >= 2 ? 1 : 0;

		if (back)
			off -= 1u << k;
		store(k, addr + off, line, from ? from + off : 0);
		if (!back)
			off += 1u << k;
	}
}

/* What the probes record, as the host says: 1, everything, while a call
 * the page asked for is under way, from a Task record: who runs (no switch
 * in the recording says so); 2, stores only, between them, so that memory
 * stays exact; 0, nothing (the boot, until the program's first line). */
__wasm_export(ksee_record) void __ksee_record(u32 level)
{
	bool begins = level == 1 && ksee_trace.on != 1;

	ksee_trace.on = level;
	if (begins)
		done(head(ksee_trace.pos, (u32)(unsigned long)current, TASK, 0));
}
