/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef _ASM_WASM_IRQFLAGS_H
#define _ASM_WASM_IRQFLAGS_H

/*
 * Interrupts are a software flag. The host never interrupts running wasm
 * code; pending interrupts are delivered when the flag is set again (and
 * when the CPU idles), so re-enabling interrupts is a delivery point.
 */
#define ARCH_IRQ_DISABLED	0
#define ARCH_IRQ_ENABLED	1

#ifndef __ASSEMBLY__

#include <asm/wasm.h>

extern unsigned long wasm_irq_flags;
void wasm_deliver_irqs(void);

static inline unsigned long arch_local_save_flags(void)
{
	return wasm_irq_flags;
}

static inline void arch_local_irq_restore(unsigned long flags)
{
	wasm_irq_flags = flags;
	if (flags == ARCH_IRQ_ENABLED && READ_ONCE(wasm_machine.irq_pending))
		wasm_deliver_irqs();
}

#include <asm-generic/irqflags.h>

#endif /* __ASSEMBLY__ */
#endif
