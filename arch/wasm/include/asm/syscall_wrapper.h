/* SPDX-License-Identifier: GPL-2.0-only */
/*
 * Syscalls take a pt_regs pointer, so that every sys_call_table entry has
 * the same wasm signature (call_indirect traps on a mismatch): the entry,
 * __wasm_sys_x, passes the registers to the body, __do_sys_x (inline: a
 * syscall is one function), each cast to its parameter's type, a long long
 * one whole. (No __se_sys_x: the registers are an array, so the cast needs
 * no shim pairing them with the types, and every syscall's stack is a frame
 * shorter.)
 */
#ifndef _ASM_WASM_SYSCALL_WRAPPER_H
#define _ASM_WASM_SYSCALL_WRAPPER_H

#include <asm/ptrace.h>

long sys_ni_syscall(void);

/* Register i as type t, through a long (or a long long) as __SC_LONG does. */
#define __WASM_ARG(t, i)						\
	(__force t)(__typeof(__builtin_choose_expr(__TYPE_IS_LL(t), 0LL, 0L)))regs->args[i]
#define __WASM_ARGS1(t1, a1) __WASM_ARG(t1, 0)
#define __WASM_ARGS2(t1, a1, t2, a2) __WASM_ARGS1(t1, a1), __WASM_ARG(t2, 1)
#define __WASM_ARGS3(t1, a1, t2, a2, t3, a3)				\
	__WASM_ARGS2(t1, a1, t2, a2), __WASM_ARG(t3, 2)
#define __WASM_ARGS4(t1, a1, t2, a2, t3, a3, t4, a4)			\
	__WASM_ARGS3(t1, a1, t2, a2, t3, a3), __WASM_ARG(t4, 3)
#define __WASM_ARGS5(t1, a1, t2, a2, t3, a3, t4, a4, t5, a5)		\
	__WASM_ARGS4(t1, a1, t2, a2, t3, a3, t4, a4), __WASM_ARG(t5, 4)
#define __WASM_ARGS6(t1, a1, t2, a2, t3, a3, t4, a4, t5, a5, t6, a6)	\
	__WASM_ARGS5(t1, a1, t2, a2, t3, a3, t4, a4, t5, a5), __WASM_ARG(t6, 5)

#define __SYSCALL_DEFINEx(x, name, ...)					\
	long __wasm_sys##name(const struct pt_regs *regs);		\
	static inline long __do_sys##name(__MAP(x, __SC_DECL, __VA_ARGS__)); \
	long __wasm_sys##name(const struct pt_regs *regs)		\
	{								\
		__MAP(x, __SC_TEST, __VA_ARGS__);			\
		return __do_sys##name(__WASM_ARGS##x(__VA_ARGS__));	\
	}								\
	static inline long __do_sys##name(__MAP(x, __SC_DECL, __VA_ARGS__))

#define SYSCALL_DEFINE0(sname)						\
	long __wasm_sys_##sname(const struct pt_regs *__unused);	\
	long __wasm_sys_##sname(const struct pt_regs *__unused)

/* A syscall this kernel may not build: no entry (see sys_call_table). */
#define COND_SYSCALL(name)						\
	long __wasm_sys_##name(const struct pt_regs *regs) __weak

#endif
