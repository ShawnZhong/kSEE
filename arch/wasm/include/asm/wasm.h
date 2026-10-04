/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef _ASM_WASM_WASM_H
#define _ASM_WASM_WASM_H

/*
 * The kernel <-> host interface: imports from the host runtime (module
 * "kernel"), and exports it calls. The kernel is one wasm instance, on one
 * thread; a task's context is a call stack of it, which the host suspends
 * and resumes (JavaScript Promise Integration, JSPI): see kernel/process.c.
 */

#include <linux/compiler.h>
#include <linux/types.h>

#define __wasm_import(name) \
	__attribute__((import_module("kernel"), import_name(#name)))
#define __wasm_export(name) __attribute__((export_name(#name))) __used

struct task_struct;

__wasm_import(console_write)
void wasm_console_write(const char *buf, unsigned int len);

/*
 * The next task the host rings (WASM_IRQ_DOORBELL: it has something for
 * it; see irq.c), or NULL: none left.
 */
__wasm_import(doorbell)
struct task_struct *wasm_doorbell(void);

__wasm_import(get_cmdline)
unsigned int wasm_get_cmdline(char *buf, unsigned int len);

/*
 * End of RAM: the kernel uses linear memory below this. (The host may grow
 * the memory beyond it for itself.)
 */
__wasm_import(memory_end)
unsigned long wasm_memory_end(void);

/* The host's clock, in nanoseconds: monotonic, and virtual (see time.c). */
__wasm_import(time_ns)
u64 wasm_time_ns(void);

/*
 * A new task: the host starts it, task_entry(@p) on the stack at @sp, the
 * first time switch_task hands it the CPU.
 */
__wasm_import(spawn_task)
void wasm_spawn_task(struct task_struct *p, void *sp);

/*
 * The new task @p is a user clone of the current task (a thread, or
 * vfork): it runs the same program, on its stack at @sp (0: below the
 * current task's, which waits for it: vfork).
 */
__wasm_import(clone_user)
void wasm_clone_user(struct task_struct *p, unsigned long sp);

/*
 * Drop the current task's stack, and start it again on a fresh one at @sp,
 * in user_entry (after an execve: the old program's frames are never
 * returned to). Does not return.
 */
__wasm_import(restart_task)
void __noreturn wasm_restart_task(struct task_struct *p, void *sp);

/*
 * Hand the CPU to @next, and suspend @prev (the caller) until it is handed
 * back; if @dead, @prev is never resumed (its stack is dropped). Suspends.
 */
__wasm_import(switch_task)
void wasm_switch_task(struct task_struct *prev, struct task_struct *next, bool dead);

/*
 * Suspend until an interrupt may be pending: the timer's deadline (see
 * wasm_timer_deadline), or input from the host.
 */
__wasm_import(idle)
void wasm_idle(void);

/* Stop the machine. Does not return. */
__wasm_import(halt)
void __noreturn wasm_halt(int code);

/*
 * Compile the user module at @image (a position-independent module, which
 * imports the kernel's memory) as the current task's next program: the
 * bytes its data needs, or -ENOEXEC if it is not a program.
 */
__wasm_import(user_load)
long wasm_user_load(const void *image, unsigned int len);

/*
 * Instantiate the program user_load compiled, its data at @base and its
 * stack below @top: its _start, as an index in the kernel's function table
 * (to call it directly), or 0 if it failed; and in @clone_entry, where its
 * clone children start (its __wasm_clone_start export; 0 if none).
 */
__wasm_import(user_start)
unsigned long wasm_user_start(unsigned long base, unsigned long top,
			      unsigned long *clone_entry);

/*
 * The disk (kernel/disk.c): its size, in 512-byte sectors (0: none); a
 * transfer handed to it, for the request tagged @tag, @len bytes at
 * @sector to (or, @write, from) @buf, made when the CPU next idles; and
 * the next request it has done (WASM_IRQ_DISK), its tag, or -1: none left.
 */
__wasm_import(disk_sectors)
u64 wasm_disk_sectors(void);
__wasm_import(disk_submit)
void wasm_disk_submit(unsigned int tag, u64 sector, void *buf, unsigned int len, bool write);
__wasm_import(disk_done)
int wasm_disk_done(void);
void wasm_disk_interrupt(void);

/* Exports, called by the host. */
void *wasm_boot_stack(void);
void wasm_boot(void);
void wasm_task_entry(struct task_struct *p);
void wasm_user_entry(void);
long wasm_syscall(long nr, u64 a0, u64 a1, u64 a2, u64 a3, u64 a4, u64 a5);

void wasm_timer_interrupt(void);
void __noreturn wasm_enter_user(void);

/* Interrupt lines, as bits of wasm_irq_pending. */
#define WASM_IRQ_TIMER		0
#define WASM_IRQ_DOORBELL	1	/* the host rings tasks (irq.c) */
#define WASM_IRQ_DISK		2	/* the disk has done requests (disk.c) */

/* Absolute time_ns of the next timer interrupt, U64_MAX if none. */
extern u64 wasm_timer_deadline;

#endif
