# Build the wasm kernel and run the tests.
#   make             build the kernel and the initramfs
#   make kernel      build linux/vmlinux (a wasm module)
#   make vmlinux     build/vmlinux: the kernel with its types, without DWARF
#   make initramfs   build build/initramfs.cpio from user/
#   make rust        build the Rust crates (crates/) and run their tests
#   make defconfig   reset linux/.config to arch/wasm/configs/defconfig
#   make test        boot in the web host (under Deno) and check the trace
#   ./ksee.sh serve  then open http://127.0.0.1:8000/web/ (?cmdline=...)

LLVM ?= -23
PLUGIN = $(CURDIR)/plugin/ksee-trace.so
# What the trace plugin instruments (see plugin/ksee-trace.cpp): every basic
# block, plus the calls that give blocks context.
KSEE_TRACE ?= *:block,wasm_syscall:enter+exit,context_switch:enter+exit,\
	      __schedule,try_to_wake_up,wake_up_new_task,kernel_clone,do_exit
# The kernel variable that holds the current task (recorded by enter/exit).
KSEE_CURRENT ?= __current_task
export KSEE_TRACE KSEE_CURRENT
# A hash of the plugin and its settings in the compiler flags makes kbuild
# rebuild the kernel whenever either changes.
KSEE_ID = $(shell (cat $(PLUGIN) 2>/dev/null; echo $(KSEE_TRACE) $(KSEE_CURRENT)) | sha1sum | cut -c1-12)
KMAKE = $(MAKE) -C linux ARCH=wasm LLVM=$(LLVM) \
	KCFLAGS="-g -fpass-plugin=$(PLUGIN) -DKSEE_ID=$(KSEE_ID)"
USER_CC = clang$(LLVM) --target=wasm32 -O2 -ffreestanding -nostdlib -Wl,--no-entry \
	  -Wl,--export=_start

all: kernel initramfs rust vmlinux

$(PLUGIN): plugin/ksee-trace.cpp
	$(MAKE) -C plugin LLVM=$(LLVM)

kernel: $(PLUGIN)
	test -f linux/.config || $(KMAKE) defconfig
	$(KMAKE) -j$$(nproc)

initramfs: build/initramfs.cpio

# The kernel the hosts load: its types from DWARF (ksee.types), no DWARF.
vmlinux: kernel rust
	target/release/ksee pack linux/vmlinux build/vmlinux

# The Rust crates (always rebuilt: cargo tracks its own dependencies). The
# runtime's wasm build shares the kernel's memory and runs in many Workers,
# so std is rebuilt with atomics (nightly, see rust-toolchain.toml), and
# wasm-bindgen generates thread-aware glue (target/web).
rust:
	cargo build -q --release -p ksee-runtime --target wasm32-unknown-unknown \
		-Zbuild-std=std,panic_abort
	wasm-bindgen --target web --out-dir target/web \
		target/wasm32-unknown-unknown/release/ksee_runtime.wasm
	cargo build -q --release -p ksee-cli
	cargo test -q -p ksee-core

build/user/%.wasm: user/%.c
	@mkdir -p $(@D)
	$(USER_CC) -o $@ $<

build/initramfs.cpio: user/initramfs.list build/user/init.wasm kernel
	linux/usr/gen_init_cpio $< > $@

defconfig:
	$(KMAKE) defconfig

test: all
	tests/trace.sh

.PHONY: all kernel initramfs rust vmlinux defconfig test
