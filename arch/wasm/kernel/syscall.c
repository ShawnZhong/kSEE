// SPDX-License-Identifier: GPL-2.0-only
/*
 * System call entry. A user module imports the kernel's syscall export
 * directly, so a syscall is a plain wasm call into the kernel instance.
 */
#include <linux/entry-common.h>
#include <linux/syscalls.h>
#include <asm/syscall.h>
#include <asm/wasm.h>

#define __SYSCALL_WITH_COMPAT(nr, native, compat)	__SYSCALL(nr, native)

/*
 * Each entry weak: one this kernel does not build (COND_SYSCALL defines
 * none, and sys_ni_syscall's entries have no __wasm_sys_ni_syscall) is
 * left NULL by the linker, and returns -ENOSYS. So the table says which
 * syscalls there are.
 */
#undef __SYSCALL
#define __SYSCALL(nr, call)	long __wasm_##call(const struct pt_regs *) __weak;
#include <asm/syscall_table_32.h>

#undef __SYSCALL
#define __SYSCALL(nr, call)	[nr] = __wasm_##call,

const syscall_fn_t sys_call_table[__NR_syscalls] = {
#include <asm/syscall_table_32.h>
};

/*
 * Whether a syscall a signal interrupted is made again: still -ERESTART*
 * once the signal is dealt with (no handler ran: there are none, see
 * signal.c), as a port whose user code re-issues it would. One with a
 * restart block goes on in restart_syscall.
 */
static bool restart(struct pt_regs *regs)
{
	switch (regs->ret) {
	case -ERESTART_RESTARTBLOCK:
		regs->syscall_nr = __NR_restart_syscall;
		fallthrough;
	case -ERESTARTNOHAND:
	case -ERESTARTSYS:
	case -ERESTARTNOINTR:
		return true;
	}
	return false;
}

noinline long wasm_syscall(long nr, u64 a0, u64 a1, u64 a2, u64 a3, u64 a4,
			   u64 a5)
{
	struct pt_regs *regs = current_pt_regs();
	void (*entry)(void) = current->thread.entry;

	*regs = (struct pt_regs){ .args = { a0, a1, a2, a3, a4, a5 }, .syscall_nr = nr };
	do {
		nr = syscall_enter_from_user_mode(regs, regs->syscall_nr);
		if (nr >= 0 && nr < __NR_syscalls && sys_call_table[nr])
			regs->ret = sys_call_table[nr](regs);
		else if (nr != -1)
			regs->ret = sys_ni_syscall();
		/*
		 * After an execve, the new program runs instead, on a fresh
		 * stack (the old program's frames are never returned to).
		 */
		if (current->thread.entry != entry)
			wasm_restart_task(current, task_stack_top(current));
		syscall_exit_to_user_mode(regs);
	} while (restart(regs));
	return regs->ret;
}

/*
 * The syscall export: wasm_syscall, which the host is told of as it enters
 * and returns, so its call and return are in the recording (__ksee_*: not
 * itself instrumented).
 */
__wasm_export(syscall) long __ksee_syscall(long nr, u64 a0, u64 a1, u64 a2,
					   u64 a3, u64 a4, u64 a5)
{
	long ret;

	wasm_syscall_enter();
	ret = wasm_syscall(nr, a0, a1, a2, a3, a4, a5);
	wasm_syscall_exit(ret);
	return ret;
}

SYSCALL_DEFINE6(mmap2, unsigned long, addr, unsigned long, len,
		unsigned long, prot, unsigned long, flags,
		unsigned long, fd, unsigned long, pgoff)
{
	/*
	 * The generic 32-bit table's mmap, which each port defines: pgoff in
	 * 4096-byte units, this port's only page size (HAVE_PAGE_SIZE_4KB).
	 */
	return ksys_mmap_pgoff(addr, len, prot, flags, fd, pgoff);
}
