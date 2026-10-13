/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef _ASM_WASM_SYSCALL_H
#define _ASM_WASM_SYSCALL_H

#include <uapi/linux/audit.h>
#include <linux/err.h>
#include <linux/sched.h>

typedef long (*syscall_fn_t)(const struct pt_regs *regs);
extern const syscall_fn_t sys_call_table[];

static inline int syscall_get_nr(struct task_struct *task,
				 struct pt_regs *regs)
{
	return regs->syscall_nr;
}

static inline void syscall_rollback(struct task_struct *task,
				    struct pt_regs *regs)
{
}

static inline long syscall_get_return_value(struct task_struct *task,
					    struct pt_regs *regs)
{
	return regs->ret;
}

static inline void syscall_get_arguments(struct task_struct *task,
					 struct pt_regs *regs,
					 unsigned long *args)
{
	int i;

	for (i = 0; i < 6; i++)
		args[i] = regs->args[i];
}

static inline bool arch_syscall_is_vdso_sigreturn(struct pt_regs *regs)
{
	return false;
}

static inline int syscall_get_arch(struct task_struct *task)
{
	return 0;	/* no AUDIT_ARCH_* value for wasm */
}

#endif
