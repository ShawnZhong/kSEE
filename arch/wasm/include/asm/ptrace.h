/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef _ASM_WASM_PTRACE_H
#define _ASM_WASM_PTRACE_H

#include <uapi/asm/ptrace.h>

#ifndef __ASSEMBLY__

#include <linux/types.h>

/*
 * Wasm has no user-visible registers (so none in the UAPI). pt_regs
 * records the syscall being made (arguments are 64 bits wide: see the
 * syscall export) so the generic entry code and signal handling have
 * something to inspect and restart.
 */
struct pt_regs {
	u64 args[6];
	long syscall_nr;
	long ret;
};

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
