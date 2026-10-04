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

__wasm_export(syscall) long wasm_syscall(long nr, u64 a0, u64 a1, u64 a2,
					 u64 a3, u64 a4, u64 a5)
{
	struct pt_regs *regs = current_pt_regs();
	void (*entry)(void) = current->thread.entry;

	regs->syscall_nr = nr;
	regs->args[0] = a0;
	regs->args[1] = a1;
	regs->args[2] = a2;
	regs->args[3] = a3;
	regs->args[4] = a4;
	regs->args[5] = a5;

	nr = syscall_enter_from_user_mode(regs, nr);
	if (nr >= 0 && nr < __NR_syscalls && sys_call_table[nr])
		regs->ret = sys_call_table[nr](regs);
	else if (nr != -1)
		regs->ret = sys_ni_syscall();
	/*
	 * After an execve, the new program runs instead, on a fresh stack
	 * (the old program's frames are never returned to).
	 */
	if (current->thread.entry != entry)
		wasm_restart_task(current, regs);
	syscall_exit_to_user_mode(regs);
	return regs->ret;
}

SYSCALL_DEFINE6(mmap2, unsigned long, addr, unsigned long, len,
		unsigned long, prot, unsigned long, flags,
		unsigned long, fd, unsigned long, pgoff)
{
	/* pgoff is in 4096-byte units, as on every 32-bit port. */
	return ksys_mmap_pgoff(addr, len, prot, flags, fd,
			       pgoff >> (PAGE_SHIFT - 12));
}
