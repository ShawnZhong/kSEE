/* SPDX-License-Identifier: GPL-2.0-only */
/*
 * Not a linker script: wasm-ld has none. This is wasm assembly that
 * scripts/wasm-ld assembles and links ahead of all other objects.
 *
 * wasm-ld places data segments in the order it first sees their names
 * (within the classes .rodata*, .data*, others, .bss*) and, given
 * --no-merge-data-segments, merges input segments only with equal names.
 * Declaring the kernel's table sections here, in order, therefore lays them
 * out contiguously, and a label at the start of one of these (empty) pieces
 * marks the start of the merged section. Symbols that must follow a section
 * get a zero-size section of their own.
 *
 * Only tables the kernel walks need exact bounds. The other linker-script
 * symbols describe regions that do not exist in a wasm layout (code is not
 * in linear memory); they are defined as empty ranges.
 */

#define SECTION(name, align)	.section name,"",@; .p2align align
#define SYMBOL(sym)		.globl sym; sym:; .size sym, 0
/* A symbol at the end of the preceding section. */
#define END(sym)		SECTION(.lds.sym, 0); SYMBOL(sym)

SECTION(.lds, 0)
SYMBOL(_stext)
SYMBOL(_etext)
SYMBOL(_sinittext)
SYMBOL(_einittext)
SYMBOL(__sched_text_start)
SYMBOL(__sched_text_end)
SYMBOL(__cpuidle_text_start)
SYMBOL(__cpuidle_text_end)
SYMBOL(__start_rodata)
SYMBOL(__end_rodata)
SYMBOL(_sdata)
SYMBOL(_edata)
SYMBOL(__init_begin)
SYMBOL(__init_end)
SYMBOL(__bss_start)
SYMBOL(__bss_stop)
SYMBOL(_end)
SYMBOL(__start___ex_table)
SYMBOL(__stop___ex_table)

/* init/main.c: initcall levels, each level followed by its sync variant. */
#define LEVEL(n)			\
	SECTION(.initcall##n##.init, 2);	\
	SYMBOL(__initcall##n##_start);	\
	SECTION(.initcall##n##s.init, 2)

SECTION(.initcallearly.init, 2)
SYMBOL(__initcall_start)
LEVEL(0)
LEVEL(1)
LEVEL(2)
LEVEL(3)
LEVEL(4)
LEVEL(5)
LEVEL(rootfs)
LEVEL(6)
LEVEL(7)
END(__initcall_end)

SECTION(.con_initcall.init, 2)
SYMBOL(__con_initcall_start)
END(__con_initcall_end)

SECTION(.init.setup, 4)
SYMBOL(__setup_start)
END(__setup_end)

/*
 * kernel/sched: sched_class instances, highest priority first, as an array
 * (no padding: alignment is __alignof__(struct sched_class)). Keep in sync
 * with SCHED_DATA in include/asm-generic/vmlinux.lds.h.
 */
SECTION(__stop_sched_class, 2)
SYMBOL(__sched_class_highest)
SECTION(__dl_sched_class, 2)
SECTION(__rt_sched_class, 2)
SECTION(__fair_sched_class, 2)
SECTION(__ext_sched_class, 2)
SECTION(__idle_sched_class, 2)
END(__sched_class_lowest)

/* init/initramfs.c: the built-in initramfs (usr/initramfs_data.S). */
SECTION(.init.ramfs, 2)
SYMBOL(__initramfs_start)

/* jiffies is the low word of jiffies_64 (little endian). */
SECTION(.data.jiffies_64, 3)
SYMBOL(jiffies)
