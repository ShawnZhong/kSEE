// SPDX-License-Identifier: GPL-2.0-only
/*
 * Interrupts.
 *
 * The host never interrupts running wasm code. It raises an interrupt by
 * setting a bit in wasm_irq_pending (and waking the CPU if it idles); the kernel
 * takes pending interrupts when it re-enables interrupts and when the CPU
 * idles, which are the points real hardware would deliver them to a
 * PREEMPT_NONE kernel that has interrupts masked in between.
 */
#include <linux/cpu.h>
#include <linux/hardirq.h>
#include <linux/init.h>
#include <linux/irqflags.h>
#include <linux/sched.h>
#include <linux/sched/signal.h>
#include <linux/sched/task_stack.h>
#include <asm/irq_regs.h>
#include <asm/wasm.h>

unsigned long wasm_irq_flags = ARCH_IRQ_DISABLED;
int wasm_irq_pending;

/*
 * The doorbell: the host has something for a task (a program waiting for
 * it, as /bin/prog waits for the page's calls, in a futex on a word the host
 * has written: woken, the wait sees the word changed and returns).
 */
static void wasm_ring(void)
{
	struct task_struct *p;

	while ((p = wasm_doorbell()))
		wake_up_process(p);
}

void wasm_deliver_irqs(void)
{
	struct pt_regs *old_regs;
	int pending;

	while ((pending = READ_ONCE(wasm_irq_pending))) {
		WRITE_ONCE(wasm_irq_pending, 0);
		wasm_irq_flags = ARCH_IRQ_DISABLED;
		old_regs = set_irq_regs(task_pt_regs(current));
		irq_enter();
		if (pending & BIT(WASM_IRQ_TIMER))
			wasm_timer_interrupt();
		if (pending & BIT(WASM_IRQ_DOORBELL))
			wasm_ring();
		irq_exit();
		set_irq_regs(old_regs);
		wasm_irq_flags = ARCH_IRQ_ENABLED;
	}
}

void __init init_IRQ(void)
{
}

/* Called with interrupts disabled; returns with them enabled. */
void arch_cpu_idle(void)
{
	wasm_idle();
	raw_local_irq_enable();
}
