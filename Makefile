# Build the wasm kernel and run the tests.
#   make             build the kernel and the initramfs
#   make kernel      build linux/vmlinux (a wasm module)
#   make initramfs   build build/initramfs.cpio from user/
#   make core        build and unit-test the Rust host core (host/ksee)
#   make defconfig   reset linux/.config to arch/wasm/configs/defconfig
#   make test        boot it under Node and run the selftest

LLVM ?= -23
PLUGIN = $(CURDIR)/plugin/ksee-trace.so
# Kernel functions whose entry is reported as a kSEE event. Changing the
# list does not rebuild anything by itself: touch the files, or clean.
KSEE_TRACE ?= __schedule,context_switch,try_to_wake_up:enter+block,\
	      wake_up_new_task,\
	      kernel_clone,do_exit,wasm_syscall
# Struct fields whose layout the host needs to decode kernel objects.
KSEE_TYPES ?= task_struct.pid,task_struct.comm,task_struct.__state
export KSEE_TRACE KSEE_TYPES
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

# The Rust host core (always rebuilt: cargo tracks its own dependencies).
core:
	cd host/ksee && cargo build -q --release --target wasm32-unknown-unknown
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
