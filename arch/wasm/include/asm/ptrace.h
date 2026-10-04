/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef _ASM_WASM_PTRACE_H
#define _ASM_WASM_PTRACE_H

#include <uapi/asm/ptrace.h>

#ifndef __ASSEMBLY__

#define user_mode(regs)			0
#define instruction_pointer(regs)	0
#define user_stack_pointer(regs)	0

/* Kernel entries are syscalls, made with interrupts enabled. */
static inline bool regs_irqs_disabled(struct pt_regs *regs)
{
	return false;
}

#endif /* __ASSEMBLY__ */
#endif
