// SPDX-License-Identifier: GPL-2.0-only
#include <linux/entry-common.h>
#include <linux/signal.h>

/*
 * Signals whose default action is to stop or kill are handled in
 * get_signal(). Running a user handler needs a way to call into the user
 * module; until then such a signal kills the task.
 */
void arch_do_signal_or_restart(struct pt_regs *regs)
{
	struct ksignal ksig;

	if (get_signal(&ksig))
		force_sigsegv(ksig.sig);
	restore_saved_sigmask();
}
