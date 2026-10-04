# SPDX-License-Identifier: GPL-2.0-only

# wasm-ld has no linker scripts; kernel/vmlinux.lds.S is wasm assembly that
# the wrapper links first to fix the data layout. The wrappers also drop GNU
# options that wasm-ld and llvm-objcopy (for wasm) do not understand.

LD := $(srctree)/arch/wasm/scripts/wasm-ld
OBJCOPY := $(srctree)/arch/wasm/scripts/objcopy

# No modules: EXPORT_SYMBOL() emits ELF-only inline asm.
KBUILD_CPPFLAGS += -D__DISABLE_EXPORTS
# Return addresses are not observable in wasm.
KBUILD_CPPFLAGS += '-D__builtin_return_address(n)=((void*)0)'

KBUILD_LDFLAGS += --no-gc-sections

# The final link only. The kernel imports the host runtime's memory (one
# thread: not shared); the runtime lives in it below CONFIG_WASM_HOST_MEMORY.
LDFLAGS_vmlinux += --global-base=$(shell printf %d $(CONFIG_WASM_HOST_MEMORY)) \
		   --import-memory \
		   --no-merge-data-segments --no-entry --export-table --growable-table \
		   --export=__stack_pointer --export=wasm_irq_pending \
		   --export=wasm_timer_deadline --export=__current_task

core-y += arch/wasm/kernel/ arch/wasm/mm/
libs-y += arch/wasm/lib/

# modpost cannot read wasm objects, so it does not write the vmlinux export
# table. It would be empty anyway: exports are disabled.
archprepare:
	$(Q)test -e .vmlinux.export.c || echo '/* no exports */' > .vmlinux.export.c
