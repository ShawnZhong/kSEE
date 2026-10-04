/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef _ASM_WASM_PGTABLE_H
#define _ASM_WASM_PGTABLE_H

#include <asm-generic/pgtable-nopmd.h>
#include <asm/page.h>

/* There is no MMU: page tables and protections are placeholders. */
#define PAGE_KERNEL	__pgprot(0)

#define swapper_pg_dir	((pgd_t *)0)

#define VMALLOC_START	0
#define VMALLOC_END	0xffffffff

#endif
