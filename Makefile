# Build the wasm kernel and run the tests.
#   make             build everything, and the site (build/web)
#   make kernel      build linux/vmlinux (a wasm module)
#   make vmlinux     build/vmlinux: the kernel with its types, without DWARF
#   make initramfs   build build/initramfs.cpio from user/
#   make rust        build the Rust crates (crates/) and run their tests
#   make defconfig   reset linux/.config to arch/wasm/configs/defconfig
#   make test        boot in the web host (under Deno) and check the trace
#   make ui-test     look at the page in headless Chromium ($CHROME)
#   ./ksee.sh serve  then open http://127.0.0.1:8000/

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

all: web

$(PLUGIN): plugin/ksee-trace.cpp
	$(MAKE) -C plugin LLVM=$(LLVM)

kernel: $(PLUGIN)
	test -f linux/.config || $(KMAKE) defconfig
	$(KMAKE) -j$$(nproc)

initramfs: build/initramfs.cpio

# The kernel the hosts load: its types and syscalls from DWARF (ksee.types),
# no DWARF.
vmlinux: kernel rust
	target/release/ksee pack linux/vmlinux build/vmlinux \
		linux/arch/wasm/include/generated/uapi/asm/unistd_32.h \
		linux/arch/wasm/include/generated/asm/syscall_table_32.h

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

# The web host as a self-contained site: the page, the runtime (pkg/), the
# kernel and initramfs, and the kernel sources the probes refer to (src/).
web: kernel initramfs rust vmlinux
	rm -rf build/web && mkdir -p build/web/pkg
	cp web/* build/web/
	cp target/web/ksee_runtime.js target/web/ksee_runtime_bg.wasm build/web/pkg/
	cp build/vmlinux build/initramfs.cpio build/web/
	target/release/ksee sources build/vmlinux linux build/web/src

defconfig:
	$(KMAKE) defconfig

test: all
	tests/trace.sh

# A look at the page in headless Chromium ($CHROME): no exceptions, and a
# screenshot in build/ui.png.
ui-test: all
	@test -n "$$CHROME" || { echo "set CHROME to a (headless) Chromium binary"; exit 1; }
	deno run -A tests/ui.js build/ui.png

.PHONY: all kernel initramfs rust vmlinux web defconfig test ui-test
