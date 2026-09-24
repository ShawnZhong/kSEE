# Build the wasm kernel and run the tests.
#   make             build the kernel and the initramfs
#   make kernel      build linux/vmlinux (a wasm module)
#   make initramfs   build build/initramfs.cpio from user/
#   make defconfig   reset linux/.config to arch/wasm/configs/defconfig
#   make test        boot it under Node and run the selftest

LLVM ?= -23
KMAKE = $(MAKE) -C linux ARCH=wasm LLVM=$(LLVM)
USER_CC = clang$(LLVM) --target=wasm32 -O2 -ffreestanding -nostdlib -Wl,--no-entry \
	  -Wl,--export=_start

all: kernel initramfs

kernel:
	test -f linux/.config || $(KMAKE) defconfig
	$(KMAKE) -j$$(nproc)

initramfs: build/initramfs.cpio

build/user/%.wasm: user/%.c
	@mkdir -p $(@D)
	$(USER_CC) -o $@ $<

build/initramfs.cpio: user/initramfs.list build/user/init.wasm kernel
	linux/usr/gen_init_cpio $< > $@

defconfig:
	$(KMAKE) defconfig

test: kernel initramfs
	tests/selftest.sh
	tests/init.sh

.PHONY: all kernel initramfs defconfig test
