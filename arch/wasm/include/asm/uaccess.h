/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef _ASM_WASM_UACCESS_H
#define _ASM_WASM_UACCESS_H

/*
 * User memory is kernel memory (NOMMU: a program's data and stack are
 * pages the kernel gave it), so user copies are plain copies. But a user
 * pointer must be in the RAM the kernel allocates from: below it are the
 * kernel image and the host's own memory, and past it memory ends (an
 * access there would trap).
 */
extern char __heap_base[];
extern void *high_memory;

static inline int __access_ok(const void __user *ptr, unsigned long size)
{
	unsigned long addr = (unsigned long)ptr, end = (unsigned long)high_memory;

	return addr >= (unsigned long)__heap_base && addr <= end && size <= end - addr;
}
#define __access_ok __access_ok

#include <asm-generic/uaccess.h>

#endif
