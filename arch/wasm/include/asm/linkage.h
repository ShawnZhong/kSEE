/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef _ASM_WASM_LINKAGE_H
#define _ASM_WASM_LINKAGE_H

/*
 * Code addresses are not observable in wasm (labels have no address and
 * functions are table indices). Identify a code location by the address
 * of a static marker instead; it is unique and constant per call site.
 */
#define _THIS_IP_ ({ static const char __here; (unsigned long)&__here; })

#endif
