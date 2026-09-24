# Build the wasm kernel and run the tests.
#   make             build the kernel and the initramfs
#   make kernel      build linux/vmlinux (a wasm module)
#   make initramfs   build build/initramfs.cpio from user/
#   make core        build and unit-test the Rust host runtime (host/ksee)
#   make defconfig   reset linux/.config to arch/wasm/configs/defconfig
#   make test        boot it under Node and run the selftest

LLVM ?= -23
PLUGIN = $(CURDIR)/plugin/ksee-trace.so
# What the trace plugin instruments (see plugin/ksee-trace.cpp): every basic
# block, plus the calls that give blocks context. Changing the list does not
# rebuild anything by itself: clean the kernel.
KSEE_TRACE ?= *:block,wasm_syscall:enter+exit,context_switch:enter+exit,\
	      __schedule,try_to_wake_up,wake_up_new_task,kernel_clone,do_exit
# Struct fields whose layout the host needs to decode kernel objects.
KSEE_TYPES ?= task_struct.pid,task_struct.comm,task_struct.__state
# The kernel variable that holds the current task (recorded by enter/exit).
KSEE_CURRENT ?= __current_task
export KSEE_TRACE KSEE_TYPES KSEE_CURRENT
KMAKE = $(MAKE) -C linux ARCH=wasm LLVM=$(LLVM) \
	KCFLAGS="-g -fpass-plugin=$(PLUGIN)"
USER_CC = clang$(LLVM) --target=wasm32 -O2 -ffreestanding -nostdlib -Wl,--no-entry \
	  -Wl,--export=_start

KSEE_CORE = host/ksee/target/wasm32-unknown-unknown/release/ksee.wasm

all: kernel initramfs core

$(PLUGIN): plugin/ksee-trace.cpp
	$(MAKE) -C plugin LLVM=$(LLVM)

kernel: $(PLUGIN)
	test -f linux/.config || $(KMAKE) defconfig
	$(KMAKE) -j$$(nproc)

initramfs: build/initramfs.cpio

# The Rust host runtime (always rebuilt: cargo tracks its own dependencies).
# Its wasm build shares the kernel's memory, so core and alloc are rebuilt
# with atomics (nightly, see host/ksee/rust-toolchain.toml).
core:
	cd host/ksee && cargo build -q --release --target wasm32-unknown-unknown \
		-Zbuild-std=core,alloc,panic_abort
	cd host/ksee && cargo test -q

build/user/%.wasm: user/%.c
	@mkdir -p $(@D)
	$(USER_CC) -o $@ $<

build/initramfs.cpio: user/initramfs.list build/user/init.wasm kernel
	linux/usr/gen_init_cpio $< > $@

defconfig:
	$(KMAKE) defconfig

test: kernel initramfs core
	tests/selftest.sh
	tests/init.sh
	tests/events.sh

.PHONY: all kernel initramfs core defconfig test
