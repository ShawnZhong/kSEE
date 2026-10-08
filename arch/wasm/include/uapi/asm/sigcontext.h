/* SPDX-License-Identifier: GPL-2.0-only WITH Linux-syscall-note */
#ifndef _UAPI_ASM_WASM_SIGCONTEXT_H
#define _UAPI_ASM_WASM_SIGCONTEXT_H

/*
 * Every port has one (a signal handler's frame holds it); wasm has no user
 * registers to save in it, only the signal mask.
 */
struct sigcontext {
	unsigned long oldmask;
};

#endif
