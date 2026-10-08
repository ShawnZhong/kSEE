// SPDX-License-Identifier: GPL-2.0-only
#include <linux/ptrace.h>
#include <linux/sched.h>

/* There are no user registers: only the generic requests. */
long arch_ptrace(struct task_struct *child, long request,
		 unsigned long addr, unsigned long data)
{
	return ptrace_request(child, request, addr, data);
}

void ptrace_disable(struct task_struct *child)
{
}
