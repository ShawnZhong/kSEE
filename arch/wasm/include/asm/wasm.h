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

struct console;
struct task_struct;

__wasm_import(console_write)
void wasm_console_write(const char *buf, unsigned int len);

/*
 * The console's input for the current task, at most @len bytes into @buf:
 * how many (0: none yet; WASM_IRQ_CONSOLE says when there may be).
 */
__wasm_import(console_read)
unsigned int wasm_console_read(char *buf, unsigned int len);

/*
 * A syscall from user mode enters, and returns @ret (syscall.c): the host
 * records the calls the page asks for from the one to the other.
 */
__wasm_import(syscall_enter)
void wasm_syscall_enter(void);
__wasm_import(syscall_exit)
void wasm_syscall_exit(long ret);

/* The console the kernel prints on, and its input's interrupt (drivers/console.c). */
extern struct console wasm_console;
void wasm_console_interrupt(void);

/*
 * The machine, the memory the kernel and the host share (setup.c; the
 * host finds its fields by the kernel's debug info, ksee-host's machine.rs): the interrupts pending (bits,
 * WASM_IRQ_*: the host sets them, the kernel takes them, irq.c); the next
 * timer interrupt's time_ns, U64_MAX if none (the kernel sets it, the host
 * raises the timer once it passes, time.c); and what the host says before
 * it calls boot: the end of RAM (the kernel uses linear memory below it;
 * the host may grow the memory beyond it for itself), the disk's size in
 * 512-byte sectors (0: none), and the command line.
 */
struct wasm_machine {
	u32 irq_pending;
	u32 memory_end;
	u64 timer_deadline;
	u64 disk_sectors;
	char cmdline[512];
};
extern struct wasm_machine wasm_machine;

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
 * wasm_machine), or input from the host.
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
 * The disk (drivers/disk.c): a transfer handed to it, for the request tagged @tag, @len bytes at
 * @sector to (or, @write, from) @buf, made when the CPU next idles; and
 * the next request it has done (WASM_IRQ_DISK), its tag, or -1: none left.
 */
__wasm_import(disk_submit)
void wasm_disk_submit(unsigned int tag, u64 sector, void *buf, unsigned int len, bool write);
__wasm_import(disk_done)
int wasm_disk_done(void);
void wasm_disk_interrupt(void);

/* Exports, called by the host. */
void *wasm_boot_stack(void);
void wasm_boot(void);
void wasm_task_entry(struct task_struct *p);
void __noreturn wasm_user_entry(void);
long wasm_syscall(long nr, u64 a0, u64 a1, u64 a2, u64 a3, u64 a4, u64 a5);
long __ksee_syscall(long nr, u64 a0, u64 a1, u64 a2, u64 a3, u64 a4, u64 a5);

/* The timer's interrupt (time.c, irq.c). */
void wasm_timer_interrupt(void);

/* Interrupt lines, as bits of wasm_machine.irq_pending (ksee-host's state.rs, IRQ_*, the same). */
#define WASM_IRQ_TIMER		0
#define WASM_IRQ_CONSOLE	1	/* the console has input (drivers/console.c) */
#define WASM_IRQ_DISK		2	/* the disk has done requests (drivers/disk.c) */

#endif
