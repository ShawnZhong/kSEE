/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef _ASM_WASM_CURRENT_H
#define _ASM_WASM_CURRENT_H

#ifndef __ASSEMBLY__

struct task_struct;

/* One CPU, one thread: the task running (set at each switch). */
extern struct task_struct *__current_task;

static __always_inline struct task_struct *get_current(void)
{
	return __current_task;
}

#define current get_current()

#endif /* __ASSEMBLY__ */
#endif
