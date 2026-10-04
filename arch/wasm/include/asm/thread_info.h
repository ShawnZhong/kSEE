/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef _ASM_WASM_THREAD_INFO_H
#define _ASM_WASM_THREAD_INFO_H

#include <asm/page.h>

#define THREAD_SIZE_ORDER	2
#define THREAD_SIZE		(PAGE_SIZE << THREAD_SIZE_ORDER)

#ifndef __ASSEMBLY__

#include <linux/types.h>

struct thread_info {
	unsigned long flags;
	unsigned long syscall_work;	/* SYSCALL_WORK_* for GENERIC_ENTRY */
	int preempt_count;
};

#define INIT_THREAD_INFO(tsk)				\
{							\
	.preempt_count	= INIT_PREEMPT_COUNT,		\
}

#endif /* __ASSEMBLY__ */

#include <asm-generic/thread_info_tif.h>

#endif
