// SPDX-License-Identifier: GPL-2.0-only
#include <linux/printk.h>
#include <linux/sched.h>
#include <linux/sched/debug.h>

/* A wasm call stack cannot be walked from inside: none to show. */
void show_stack(struct task_struct *task, unsigned long *sp,
		const char *loglvl)
{
}
