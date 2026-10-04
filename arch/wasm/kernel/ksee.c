// SPDX-License-Identifier: GPL-2.0-only
/*
 * The trace: a ring of the kernel's records, and its writers. The kSEE
 * plugin's probes call them (a block, a return, a store, ...); the host
 * reads the ring (ksee_core::trace) and calls two of them for its own
 * writes (ksee_bulk) and where a recording begins (ksee_task). Not
 * instrumented: the plugin leaves __ksee_* alone.
 *
 * A record is a head pair of words [a, tag] and continuation pairs
 * [value, CONT]; a tag's top nibble says what it is (the plugin's header).
 */

#include <linux/types.h>
#include <asm/wasm.h>

#define RING_WORDS (1u << 21)
#define CONT (8u << 28)
#define STORE (7u << 28)
#define TASK (3u << 28)

/*
 * pos counts the words ever written (the ring keeps the most recent),
 * stored with release order after each record, so a reader that loads it
 * sees whole records; words, the ring's size; on: probes record only while
 * it is nonzero: 1, everything; 2, stores only (the host sets it: 1 within
 * a syscall the page asked for, 2 between, so that memory stays exact
 * throughout). One writer at a time: the kernel is uniprocessor.
 */
static struct {
	u32 pos, words, on, pad;
	u32 ring[RING_WORDS];
} trace __aligned(8) = { .words = RING_WORDS };

__wasm_export(ksee_trace) u32 __ksee_trace_info(void)
{
	return (u32)(unsigned long)&trace;
}

/* Record [a, tag] and n values, if the recording takes it (`store`: a
 * store's, taken at either level). */
static __always_inline void put(bool store, u32 a, u32 tag, const u32 *v, u32 n)
{
	u32 p = trace.pos, i;

	if (store ? !trace.on : trace.on != 1)
		return;
	trace.ring[p & (RING_WORDS - 1)] = a;
	trace.ring[(p + 1) & (RING_WORDS - 1)] = tag;
	for (i = 0; i < n; i++) {
		trace.ring[(p + 2 + 2 * i) & (RING_WORDS - 1)] = v[i];
		trace.ring[(p + 3 + 2 * i) & (RING_WORDS - 1)] = CONT;
	}
	__atomic_store_n(&trace.pos, p + 2 + 2 * n, __ATOMIC_RELEASE);
}

/* The probes' records: [a, tag] and the n values the probe put in
 * __ksee_values (up to 15: a call's seven arguments, wide ones two words,
 * and its frame base) just before. */
u32 __ksee_values[15];

void __ksee_put(u32 a, u32 tag, u32 n)
{
	put(false, a, tag, __ksee_values, n);
}

/* A store of 1 << k bytes at addr, on a line: its old value (before the
 * write), low word first; a copy's says where from. */
static __always_inline void store(u32 k, u32 addr, u32 line, u32 from, bool copy)
{
	u64 old = 0;
	u32 v[3], n = 0;

	if (!trace.on)
		return;
	__builtin_memcpy(&old, (void *)(unsigned long)addr, 1u << k);
	v[n++] = (u32)old;
	if (k == 3)
		v[n++] = (u32)(old >> 32);
	if (copy)
		v[n++] = from;
	put(true, addr, STORE | line << 4 | k, v, n);
}

void __ksee_put_store1(u32 addr, u32 line) { store(0, addr, line, 0, false); }
void __ksee_put_store2(u32 addr, u32 line) { store(1, addr, line, 0, false); }
void __ksee_put_store4(u32 addr, u32 line) { store(2, addr, line, 0, false); }
void __ksee_put_store8(u32 addr, u32 line) { store(3, addr, line, 0, false); }
void __ksee_put_copy1(u32 addr, u32 line, u32 from) { store(0, addr, line, from, true); }
void __ksee_put_copy2(u32 addr, u32 line, u32 from) { store(1, addr, line, from, true); }
void __ksee_put_copy4(u32 addr, u32 line, u32 from) { store(2, addr, line, from, true); }
void __ksee_put_copy8(u32 addr, u32 line, u32 from) { store(3, addr, line, from, true); }

/* The writes of len bytes at addr, as Store records of up to 8 bytes each;
 * from: where a copy takes them from, else 0. A copy whose higher
 * destination overlaps its source goes backwards, so no earlier chunk's
 * destination is a later one's apparent source. */
__wasm_export(ksee_bulk) void __ksee_bulk(u32 addr, u32 len, u32 line, u32 from)
{
	bool back = from && from < addr && addr - from < len;
	u32 off = back ? len : 0;

	while (back ? off : off < len) {
		u32 left = back ? off : len - off;
		u32 k = left >= 8 ? 3 : left >= 4 ? 2 : left >= 2 ? 1 : 0;

		if (back)
			off -= 1u << k;
		store(k, addr + off, line, from ? from + off : 0, from);
		if (!back)
			off += 1u << k;
	}
}

/* A Task record: task runs, where a requested recording begins (no switch
 * in it says so). */
__wasm_export(ksee_task) void __ksee_task(u32 task)
{
	put(false, task, TASK, NULL, 0);
}
