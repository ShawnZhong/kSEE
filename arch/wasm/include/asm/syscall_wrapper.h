/* SPDX-License-Identifier: GPL-2.0-only */
/*
 * Syscalls take a pt_regs pointer, so that every sys_call_table entry has
 * the same wasm signature (call_indirect traps on a mismatch). The generic
 * shims (__se_sys_x, which widens the arguments, and the body __do_sys_x)
 * are inline: a syscall is one function, __wasm_sys_x. __se_sys_x has no
 * debug info (nodebug): only the registers' pairing with the parameters'
 * types, it is not a frame of every syscall's stack.
 */
#ifndef _ASM_WASM_SYSCALL_WRAPPER_H
#define _ASM_WASM_SYSCALL_WRAPPER_H

#include <asm/ptrace.h>

long sys_ni_syscall(void);

#define SC_WASM_REGS_TO_ARGS(x, ...)					\
	__MAP(x, __SC_ARGS, , regs->args[0], , regs->args[1],		\
	      , regs->args[2], , regs->args[3], , regs->args[4],	\
	      , regs->args[5])

#define __SYSCALL_DEFINEx(x, name, ...)					\
	long __wasm_sys##name(const struct pt_regs *regs);		\
	static inline long __do_sys##name(__MAP(x, __SC_DECL, __VA_ARGS__)); \
	static inline __attribute__((nodebug))				\
	long __se_sys##name(__MAP(x, __SC_LONG, __VA_ARGS__))		\
	{								\
		long ret = __do_sys##name(__MAP(x, __SC_CAST, __VA_ARGS__)); \
		__MAP(x, __SC_TEST, __VA_ARGS__);			\
		__PROTECT(x, ret, __MAP(x, __SC_ARGS, __VA_ARGS__));	\
		return ret;						\
	}								\
	long __wasm_sys##name(const struct pt_regs *regs)		\
	{								\
		return __se_sys##name(SC_WASM_REGS_TO_ARGS(x, __VA_ARGS__)); \
	}								\
	static inline long __do_sys##name(__MAP(x, __SC_DECL, __VA_ARGS__))

#define SYSCALL_DEFINE0(sname)						\
	long __wasm_sys_##sname(const struct pt_regs *__unused);	\
	long __wasm_sys_##sname(const struct pt_regs *__unused)

/* A syscall this kernel may not build: no entry (see sys_call_table). */
#define COND_SYSCALL(name)						\
	long __wasm_sys_##name(const struct pt_regs *regs) __weak

#endif
