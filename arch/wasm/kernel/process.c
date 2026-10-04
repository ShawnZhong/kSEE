// SPDX-License-Identifier: GPL-2.0-only
/*
 * Tasks and context switching. The kernel is one wasm instance, on one
 * thread; a task's context is a call stack of it, which the host suspends
 * and resumes (JSPI). __switch_to() asks the host to hand the CPU to the
 * next task: it resumes that task's stack (or starts it, task_entry) and
 * suspends this one, until the CPU is handed back to it.
 */
#include <linux/entry-common.h>
#include <linux/sched.h>
#include <linux/sched/debug.h>
#include <linux/sched/task.h>
#include <linux/sched/task_stack.h>
#include <linux/printk.h>
#include <asm/switch_to.h>
#include <asm/wasm.h>

/* The task that most recently switched away on this CPU. */
static struct task_struct *wasm_last;

struct task_struct *__switch_to(struct task_struct *prev,
				struct task_struct *next)
{
	wasm_last = prev;
	wasm_switch_task(prev, next, READ_ONCE(prev->__state) == TASK_DEAD);
	__current_task = prev;
	return wasm_last;
}

/*
 * Return to user mode, at the start of the task's program (a direct call:
 * nothing of the host's may be on a stack the host suspends). Returning
 * from it is an exit.
 */
static void __noreturn wasm_enter_user(void)
{
	syscall_exit_to_user_mode(current_pt_regs());
	current->thread.entry();
	do_exit(0);
}

/* Called by the host, on a new task's stack, when it first gets the CPU. */
__wasm_export(task_entry) void wasm_task_entry(struct task_struct *p)
{
	__current_task = p;
	schedule_tail(wasm_last);

	/* A kernel thread returns only after a successful kernel_execve(). */
	if (p->thread.fn)
		p->thread.fn(p->thread.fn_arg);
	wasm_enter_user();
}

/* Called by the host, on a fresh stack for the current task (see syscall.c). */
__wasm_export(user_entry) void wasm_user_entry(void)
{
	wasm_enter_user();
}


int copy_thread(struct task_struct *p, const struct kernel_clone_args *args)
{
	struct pt_regs *regs = task_pt_regs(p);

	/*
	 * A user clone cannot return twice (wasm has no way to copy a call
	 * stack): the child starts over, at its program's clone entry. It
	 * shares its parent's memory: without CLONE_VM, it would need a copy
	 * at the same addresses, which NOMMU cannot give. No CLONE_SETTLS:
	 * programs have no thread-local storage (a module with some does not
	 * load: see the host's user_start).
	 */
	if (!args->fn && (!(args->flags & CLONE_VM) || (args->flags & CLONE_SETTLS) ||
			  !current->thread.clone_entry))
		return -EINVAL;

	memset(regs, 0, sizeof(*regs));
	p->thread.fn = args->fn;
	p->thread.fn_arg = args->fn_arg;

	wasm_spawn_task(p, regs);
	if (!args->fn) {
		p->thread.entry = current->thread.clone_entry;
		wasm_clone_user(p, args->stack);
	}
	return 0;
}

void flush_thread(void)
{
}

void show_regs(struct pt_regs *regs)
{
	show_regs_print_info(KERN_DEFAULT);
}
