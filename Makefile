# Build the kSEE site in web/: the kernel (instrumented by plugin/), its
# initramfs (user/), and the Rust runtime. ./ksee.sh is the front door.
LLVM ?= -23
PLUGIN := plugin/ksee-trace.so
# What the plugin instruments (see plugin/ksee-trace.cpp): every basic block,
# plus the calls that give blocks context; and the current-task variable.
export KSEE_TRACE ?= *:block,wasm_syscall:enter+exit,context_switch:enter+exit,\
	__schedule,try_to_wake_up,wake_up_new_task,kernel_clone,do_exit
export KSEE_CURRENT ?= __current_task
# A hash of the plugin and its settings in the flags rebuilds the kernel
# when either changes.
KSEE_ID = $(shell (cat $(PLUGIN); echo $$KSEE_TRACE $$KSEE_CURRENT) | sha1sum | cut -c1-12)
KMAKE = $(MAKE) -C linux ARCH=wasm LLVM=$(LLVM) \
	KCFLAGS="-g -fpass-plugin=$(CURDIR)/$(PLUGIN) -DKSEE_ID=$(KSEE_ID)"
RUNTIME := target/wasm32-unknown-unknown/release/ksee_runtime.wasm

all: web/vmlinux web/initramfs.cpio web/pkg

$(PLUGIN): plugin/ksee-trace.cpp
	clang++$(LLVM) -shared -fPIC -O2 $$(llvm-config$(LLVM) --cxxflags) $< -o $@

# kbuild and cargo track their own dependencies: always ask them.
linux/vmlinux: $(PLUGIN) FORCE
	@test -f linux/.config || $(KMAKE) defconfig
	$(KMAKE) -j$$(nproc)

target/release/ksee: FORCE
	cargo build -q --release -p ksee-cli

# The runtime shares the kernel's memory and runs in many Workers: std
# rebuilt with atomics (nightly, rust-toolchain.toml), and thread-aware
# wasm-bindgen glue.
web/pkg: FORCE
	cargo build -q --release -p ksee-runtime --target wasm32-unknown-unknown -Zbuild-std=std,panic_abort
	wasm-bindgen --target web --out-dir $@ $(RUNTIME)

# The kernel for the page, with its types and syscalls; and its sources.
web/vmlinux: linux/vmlinux target/release/ksee
	target/release/ksee pack linux web

web/initramfs.cpio: user/initramfs.list user/init.c linux/vmlinux
	clang$(LLVM) --target=wasm32 -O2 -ffreestanding -nostdlib -Wl,--no-entry -Wl,--export=_start \
		-o web/init.wasm user/init.c
	cd web && ../linux/usr/gen_init_cpio ../$< > initramfs.cpio

test: all
	cargo test -q -p ksee-core
	deno run --allow-read tests/web.js

# The page in headless Chromium ($CHROME); a screenshot in web/ui.png.
ui-test: all
	deno run -A tests/ui.js web/ui.png

clean:
	$(KMAKE) clean
	cargo clean
	rm -rf $(PLUGIN) web/pkg web/src web/vmlinux web/initramfs.cpio web/init.wasm web/ui.png

.PHONY: all test ui-test clean FORCE
