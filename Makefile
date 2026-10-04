# The kernel's half of the build: C and LLVM, file to file. `ksee build`
# runs it, then packs the kernel and builds the site
# (crates/ksee-cli/src/build.rs).
#
#   linux/                       Linux, the stable tree's LINUX_TAG, fetched once; linux.patch
#                                (the port's edits to Linux's own files) applied, and arch/wasm/
#                                (the port's own files) linked in
#   plugin/ksee-trace.so         the LLVM pass that instruments the kernel
#   linux/vmlinux                the kernel, instrumented (kbuild), and its UAPI headers
#   target/initramfs/initramfs.cpio
#                                /init and /bin/prog (user/: plain C, no C library), a note
#                                (user/note.txt), and an ext4 disk image of user/disk/:
#                                user/initramfs.list
#   target/chrome/               the tests' browser: Chrome for Testing's headless shell,
#                                CHROME_VERSION, fetched once (`make chrome`: `ksee test` runs it)
#
# `make patch` writes linux.patch from linux/ (never the build). The port's own
# files are edited in arch/wasm/, not linux/.

# The release the port is a patch to: the stable tree's tag, and the commit
# it must be (what is fetched, checked).
LINUX_TREE := https://git.kernel.org/pub/scm/linux/kernel/git/stable/linux.git
LINUX_TAG := v7.2.7
LINUX_COMMIT := f42acb3678424d1e08f6ed27c0d8ba8a125e14d6
# The tests' browser: one version for every machine (Playwright's mirror
# of Chrome for Testing has it for arm64 Linux too).
CHROME_VERSION := 156.0.8078.4

# (setup.sh's LLVM 22 first.)
export PATH := /usr/lib/llvm-22/bin:$(PATH)
export OUT := target/initramfs
PLUGIN := plugin/ksee-trace.so
# (A hash of the pass's source in the flags: kbuild rebuilds when the
# instrumentation changes, not when an equivalent object is relinked.
# Inlining only what the source asks for, `inline`: the compiler's own
# choices would hide calls, with their arguments, as inlined code.)
KMAKE = $(MAKE) -C linux ARCH=wasm LLVM=1 \
	KCFLAGS="-g -finline-hint-functions -fpass-plugin=$(CURDIR)/$(PLUGIN) -DKSEE_ID=$(shell cksum < plugin/ksee-trace.cpp | cut -d' ' -f1)"

all: linux/vmlinux $(OUT)/initramfs.cpio
.PHONY: all linux/vmlinux patch llvm chrome

llvm:
	@v=$$(llvm-config --version); test "$${v%%.*}" = 22 || { echo "LLVM 22 is required, found $$v" >&2; exit 1; }

# linux/: the release (a shallow git repo: its one commit), and
# linux.patch, the first time only (to apply an updated one, remove linux/).
linux/Makefile:
	mkdir -p linux
	git -C linux init -q
	git -C linux fetch --depth=1 $(LINUX_TREE) tag $(LINUX_TAG)
	test "$$(git -C linux rev-parse '$(LINUX_TAG)^{commit}')" = $(LINUX_COMMIT)
	git -C linux checkout -q $(LINUX_TAG)
	git -C linux apply ../linux.patch
	ln -s ../../arch/wasm linux/arch/wasm

$(PLUGIN): plugin/ksee-trace.cpp | llvm
	clang++ -shared -fPIC -O2 $< -o $@ $$(llvm-config --cxxflags)

# (Every Kconfig the port has: a new one runs defconfig again; touched,
# as kbuild leaves a .config the same as it was.)
linux/.config: arch/wasm/configs/defconfig $(wildcard arch/wasm/Kconfig*) | linux/Makefile
	$(KMAKE) defconfig
	touch $@

# (kbuild knows the kernel's own dependencies: it is always asked; it
# builds gen_init_cpio too, the kernel's own initramfs writer.)
linux/usr/gen_init_cpio: linux/vmlinux ;
linux/vmlinux: $(PLUGIN) linux/.config
	$(KMAKE)
	$(KMAKE) headers

# The programs: position-independent, in the kernel's memory (see
# binfmt_wasm.c), their syscalls user/syscall.h's, with the UAPI headers.
$(OUT)/%.wasm: user/%.c user/syscall.h | linux/vmlinux
	@mkdir -p $(OUT)
	clang --target=wasm32 -O2 -g0 -ffreestanding -nostdlib -fPIC -Wl,-pie -Wl,--experimental-pic \
		-Wl,--import-memory -Wl,--no-entry -nostdinc -isystem linux/usr/include -o $@ $<

# The ext4 disk /init mounts (a file, through the loop device): user/disk/,
# the files the examples start from; initialized in full: no background
# work after the mount (ext4lazyinit's).
$(OUT)/disk.img: $(shell find user/disk)
	@mkdir -p $(OUT)
	rm -f $@
	mkfs.ext4 -q -F -b 1024 -L ksee -N 32 -d user/disk -E lazy_itable_init=0,lazy_journal_init=0 $@ 2M

# The archive (all times 0): the console (the kernel opens it before init),
# the programs, the note, the disk.
$(OUT)/initramfs.cpio: user/initramfs.list linux/usr/gen_init_cpio $(OUT)/init.wasm $(OUT)/prog.wasm user/note.txt $(OUT)/disk.img
	linux/usr/gen_init_cpio -t 0 $< > $@

# linux.patch: the port's edits to Linux's own files, as linux/ differs from
# the release (the files it adds are arch/wasm/'s: git leaves them untracked).
patch:
	@test -f linux/Makefile || { echo "no linux/: run ./ksee.sh build first" >&2; exit 1; }
	git -C linux reset -q
	git -C linux diff --no-color $(LINUX_TAG) > linux.patch

# target/chrome/: the headless shell for this machine, CHROME_VERSION's
# (another version: fetched again, in place of the one there).
chrome: target/chrome/$(CHROME_VERSION)
target/chrome/$(CHROME_VERSION):
	rm -rf target/chrome target/chrome.zip
	a=$$(uname -m | sed 's/x86_64/linux64/; s/aarch64/linux-arm64/'); \
	curl -fsSL --proto '=https' -o target/chrome.zip https://cdn.playwright.dev/builds/cft/$(CHROME_VERSION)/$$a/chrome-headless-shell-$$a.zip && \
	unzip -q -d target target/chrome.zip && mv target/chrome-headless-shell-$$a target/chrome
	rm target/chrome.zip
	touch $@
