// SPDX-License-Identifier: GPL-2.0-only
#include <linux/printk.h>
#include <linux/sched.h>
#include <linux/sched/debug.h>
#include <asm/wasm.h>

__wasm_import(print_stack)
void wasm_print_stack(void);

/* The wasm call stack is only visible to the host. */
void show_stack(struct task_struct *task, unsigned long *sp,
		const char *loglvl)
{
	if (!task || task == current)
		wasm_print_stack();
}
