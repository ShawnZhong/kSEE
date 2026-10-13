/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef _ASM_WASM_PROCESSOR_H
#define _ASM_WASM_PROCESSOR_H

#include <asm/ptrace.h>

#define TASK_SIZE		0xffffffffUL

#ifndef __ASSEMBLY__

struct task_struct;

struct thread_struct {
	/* The syscall it makes (wasm has no user registers): syscall.c's. */
	struct pt_regs regs;
	/* Kernel thread entry. */
	int (*fn)(void *);
	void *fn_arg;
	/*
	 * The program's _start, once exec loaded one (see binfmt_wasm.c), and
	 * where its clone children start (a user clone's entry is that).
	 */
	void (*entry)(void);
	void (*clone_entry)(void);
};

#define INIT_THREAD	{ }

#define task_pt_regs(tsk)	(&(tsk)->thread.regs)
/* Where its kernel stack starts (it grows down). */
#define task_stack_top(tsk)	((void *)(task_stack_page(tsk) + THREAD_SIZE))

/* Kernel code always runs on its task stack. */
#define on_thread_stack()	1

#define KSTK_EIP(tsk)	0
#define KSTK_ESP(tsk)	0

static inline unsigned long __get_wchan(struct task_struct *p)
{
	return 0;
}

static inline void cpu_relax(void)
{
	__asm__ __volatile__("" ::: "memory");
}

#endif /* __ASSEMBLY__ */
#endif
