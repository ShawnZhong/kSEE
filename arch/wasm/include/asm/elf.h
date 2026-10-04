/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef _ASM_WASM_ELF_H
#define _ASM_WASM_ELF_H

#define ELF_CLASS	ELFCLASS32

#ifndef __ASSEMBLY__
#include <asm/ptrace.h>

typedef unsigned long elf_greg_t;
#define ELF_NGREG	(sizeof(struct pt_regs) / sizeof(elf_greg_t))
typedef elf_greg_t elf_gregset_t[ELF_NGREG];
typedef struct { } elf_fpregset_t;
#endif

#endif
