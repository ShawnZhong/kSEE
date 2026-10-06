// SPDX-License-Identifier: GPL-2.0-only
#include <linux/console.h>
#include <linux/delay.h>
#include <linux/init.h>
#include <linux/initrd.h>
#include <linux/memblock.h>
#include <linux/panic.h>
#include <linux/pfn.h>
#include <linux/reboot.h>
#include <linux/sched/task.h>
#include <linux/sched/task_stack.h>
#include <linux/start_kernel.h>
#include <asm/sections.h>
#include <asm/setup.h>
#include <asm/wasm.h>

unsigned long init_stack[THREAD_SIZE / sizeof(unsigned long)]
	__aligned(THREAD_SIZE);

struct task_struct *__current_task = &init_task;

/* Stack pointer the host installs before calling boot(). */
__wasm_export(boot_stack) void *wasm_boot_stack(void)
{
	return task_stack_top(&init_task);
}

/* Entry point: the boot CPU, which becomes init_task/idle. */
__wasm_export(boot) void wasm_boot(void)
{
	wasm_get_cmdline(boot_command_line, COMMAND_LINE_SIZE);
	start_kernel();
}

void __init setup_arch(char **cmdline_p)
{
	phys_addr_t mem_end = wasm_memory_end();

	*cmdline_p = boot_command_line;
	register_console(&wasm_console);

	/* Memory below mem_end is RAM; the kernel image sits at its start. */
	memblock_add(0, mem_end);
	memblock_reserve(0, PAGE_ALIGN(__pa(__heap_base)));

	max_low_pfn = max_pfn = PFN_DOWN(mem_end);

	parse_early_param();
	/* The host loads an initramfs at the top of memory ("initrd=a,n"). */
	reserve_initrd_mem();
}

/* Exit status for the host: nonzero after a panic. */
void machine_restart(char *cmd)
{
	wasm_halt(panic_in_progress());
}

void machine_halt(void)
{
	wasm_halt(0);
}

void machine_power_off(void)
{
	wasm_halt(0);
}

/* Delays read the host's clock (lib/delay.c): no loop to calibrate. */
void __init calibrate_delay(void)
{
}
