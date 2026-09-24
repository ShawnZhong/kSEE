# Build the wasm kernel and run the tests.
#   make [kernel]    build linux/vmlinux (a wasm module)
#   make defconfig   reset linux/.config to arch/wasm/configs/defconfig
#   make test        boot it under Node and run the selftest

LLVM ?= -23
KMAKE = $(MAKE) -C linux ARCH=wasm LLVM=$(LLVM)

kernel:
	test -f linux/.config || $(KMAKE) defconfig
	$(KMAKE) -j$$(nproc)

defconfig:
	$(KMAKE) defconfig

test: kernel
	tests/selftest.sh

.PHONY: kernel defconfig test
