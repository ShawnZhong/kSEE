/* SPDX-License-Identifier: GPL-2.0-only WITH Linux-syscall-note */
#ifndef _UAPI_ASM_WASM_PTRACE_H
#define _UAPI_ASM_WASM_PTRACE_H

/* (The generic entry code's ptrace requires them: kernel/ptrace.c.) */
#define PTRACE_SYSEMU			31
#define PTRACE_SYSEMU_SINGLESTEP	32

#ifndef __ASSEMBLY__

#include <linux/types.h>

/*
 * Wasm has no user-visible registers. pt_regs records the syscall being
 * made (arguments are 64 bits wide: see the syscall export) so the generic
 * entry code and signal handling have something to inspect and restart.
 */
struct pt_regs {
	__u64 args[6];
	long syscall_nr;
	long ret;
};

#endif /* __ASSEMBLY__ */
#endif
