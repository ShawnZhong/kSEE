# The kernel's half of the build: C and LLVM, file to file. `ksee build`
# runs it (with setup.sh's LLVM 22 first on PATH), then packs the kernel and
# builds the site (crates/ksee-cli/src/build.rs).
#
#   linux/                       Linux, the stable tree's LINUX_TAG, fetched once, linux.patch applied
#   plugin/ksee-trace.so         the LLVM pass that instruments the kernel
#   linux/vmlinux                the kernel, instrumented (kbuild), and its UAPI headers
#   target/initramfs/initramfs.cpio
#                                /init and /bin/prog (user/: plain C, no C library), a note
#                                (user/note.txt), and an ext4 disk image of user/disk/:
#                                user/initramfs.list
#
# `make patch` writes linux.patch from linux/ (never the build).

# The release the port is a patch to: the stable tree's tag, and the commit
# it must be (what is fetched, checked).
LINUX_TREE := https://git.kernel.org/pub/scm/linux/kernel/git/stable/linux.git
LINUX_TAG := v7.2.7
LINUX_COMMIT := f42acb3678424d1e08f6ed27c0d8ba8a125e14d6

export OUT := target/initramfs
PLUGIN := plugin/ksee-trace.so
# (A hash of the pass's source in the flags: kbuild rebuilds when the
# instrumentation changes, not when an equivalent object is relinked.
# Inlining only what the source asks for, `inline`: the compiler's own
# choices would hide calls, with their arguments, as inlined code.)
KMAKE = $(MAKE) -C linux ARCH=wasm LLVM=1 \
	KCFLAGS="-g -finline-hint-functions -fpass-plugin=$(CURDIR)/$(PLUGIN) -DKSEE_ID=$(shell cksum < plugin/ksee-trace.cpp | cut -d' ' -f1)"

all: linux/vmlinux $(OUT)/initramfs.cpio
.PHONY: all linux/vmlinux patch llvm

llvm:
	@v=$$(llvm-config --version); test "$${v%%.*}" = 22 || { echo "LLVM 22 is required, found $$v" >&2; exit 1; }

# linux/: the release (a shallow git repo: its one commit), and the port,
# the first time only (to apply an updated linux.patch, remove linux/).
linux/Makefile:
	mkdir -p linux
	git -C linux init -q
	git -C linux fetch --depth=1 $(LINUX_TREE) tag $(LINUX_TAG)
	test "$$(git -C linux rev-parse '$(LINUX_TAG)^{commit}')" = $(LINUX_COMMIT)
	git -C linux checkout -q $(LINUX_TAG)
	git -C linux apply ../linux.patch

# (macOS links -shared eagerly: the LLVM symbols are left to clang, which
# loads the pass.)
$(PLUGIN): plugin/ksee-trace.cpp | llvm
	clang++ -shared -fPIC -O2 $< -o $@ $$(llvm-config --cxxflags) $$(test $$(uname -s) = Darwin && echo -Wl,-undefined,dynamic_lookup)

# (Every Kconfig the port has: a new one runs defconfig again.)
linux/.config: linux/Makefile linux/arch/wasm/configs/defconfig $(wildcard linux/arch/wasm/Kconfig*)
	$(KMAKE) defconfig

# (kbuild knows the kernel's own dependencies: it is always asked.)
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

# The kernel's own initramfs writer, for this machine.
$(OUT)/gen_init_cpio: linux/usr/gen_init_cpio.c
	@mkdir -p $(OUT)
	clang -O2 -o $@ $<

# The archive (all times 0): the console (the kernel opens it before init),
# the programs, the note, the disk.
$(OUT)/initramfs.cpio: user/initramfs.list $(OUT)/gen_init_cpio $(OUT)/init.wasm $(OUT)/prog.wasm user/note.txt $(OUT)/disk.img
	$(OUT)/gen_init_cpio -t 0 $< > $@

# linux.patch: the port, all of it (the files new too), as linux/ differs
# from the release.
patch:
	@test -f linux/Makefile || { echo "no linux/: run ./ksee.sh build first" >&2; exit 1; }
	git -C linux add -A --intent-to-add
	git -C linux diff --no-color $(LINUX_TAG) > linux.patch
