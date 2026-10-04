/* SPDX-License-Identifier: GPL-2.0-only WITH Linux-syscall-note */
#ifndef _UAPI_ASM_WASM_POSIX_TYPES_H
#define _UAPI_ASM_WASM_POSIX_TYPES_H

/* The wasm32 C ABI uses long, not int, for size_t and ptrdiff_t. */
typedef unsigned long	__kernel_size_t;
typedef long		__kernel_ssize_t;
typedef long		__kernel_ptrdiff_t;
#define __kernel_size_t __kernel_size_t

#include <asm-generic/posix_types.h>

#endif
