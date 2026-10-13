# The build, file to file: `make site` (as `ksee build` runs it) makes the
# site, target/site/, what `ksee serve` serves and `ksee deploy` pushes. Its
# steps that are code are the ksee command's (crates/ksee-cli: tables,
# sources, lessons, bindgen). What it makes:
#
#   linux/                    Linux, LINUX_TAG, fetched once, linux.patch (the port's edits to
#                             Linux's own files) applied, arch/wasm/ (the port's own) linked
#                             in: sources only (`make patch` writes linux.patch from it)
#   target/ksee-trace.so      the LLVM pass that instruments the kernel (plugin/)
#   target/linux/             kbuild's, out of the tree: vmlinux, the kernel, instrumented,
#                             with DWARF; usr/include, its UAPI headers; usr/gen_init_cpio
#   target/vmlinux            the kernel, split: the page's, without DWARF and the plugin's
#   target/vmlinux.debug        ksee section; and those, its description
#   target/user/              the programs (user/: plain C, no C library), the initramfs
#                             (user/initramfs.list), the disk (an ext4 image of user/disk/)
#   target/tables             the kernel's tables, of its description (ksee tables)
#   target/site/              the site: the kernel, its tables, the initramfs and the disk,
#                             gzipped (hosts compress them less, or not at all; the page
#                             unpacks them); src/, the sources the tables name (ksee
#                             sources); lessons.json and a page per lesson (ksee lessons);
#                             pkg/, the runtime and the page, one module (cargo, ksee bindgen)
#   target/chrome/            the tests' browser, Chrome's headless shell (`make chrome`)

# ---- What is pinned ------------------------------------------------------------

# The release the port is a patch to: the stable tree's tag, the commit it
# must be (what is fetched, checked), and its date (the build's: see KMAKE).
LINUX_TREE := https://git.kernel.org/pub/scm/linux/kernel/git/stable/linux.git
LINUX_TAG := v7.2.7
LINUX_COMMIT := f42acb3678424d1e08f6ed27c0d8ba8a125e14d6
LINUX_DATE := 1789996140
# The tests' browser: one version for every machine (Playwright's mirror of
# Chrome for Testing has it for arm64 Linux too).
CHROME_VERSION := 156.0.8078.4

# ---- Where things go -----------------------------------------------------------

# (setup.sh's LLVM 22 first.)
export PATH := /usr/lib/llvm-22/bin:$(PATH)
PLUGIN := target/ksee-trace.so
KOUT := target/linux
# (user/initramfs.list names the programs by it.)
export OUT := target/user
SITE := target/site
# (The ksee command: the one running make, as `ksee build` says.)
KSEE ?= target/release/ksee

# A file made as $@.new, replacing $@ only if it changed (kbuild is always
# asked; what is made of $@ is made again only when it is newer).
replace = cmp -s $@.new $@ && rm $@.new || mv $@.new $@

.PHONY: site lessons page $(KOUT)/vmlinux patch llvm chrome

site: $(SITE)/vmlinux.gz $(SITE)/tables.gz $(SITE)/src $(SITE)/initramfs.cpio.gz $(SITE)/disk.img.gz lessons page

# ---- Linux ---------------------------------------------------------------------

llvm:
	@v=$$(llvm-config --version); test "$${v%%.*}" = 22 || { echo "LLVM 22 is required, found $$v" >&2; exit 1; }

$(PLUGIN): plugin/ksee-trace.cpp | llvm
	@mkdir -p $(@D)
	clang++ -shared -fPIC -O2 $< -o $@ $$(llvm-config --cxxflags)

# (The same build each time: the release's date, not the clock's; a version,
# user and host of its own, not the tree's state (-dirty: the patch applied)
# or the machine's; build #1. A hash of the pass's source in the flags:
# kbuild rebuilds when the instrumentation changes, not when an equivalent
# object is relinked. Inlining only what the source asks for, `inline`: the
# compiler's own choices would hide calls, with their arguments, as inlined
# code. Its files named from the tree's top, fs/read_write.c, as in it, not
# where it is: kbuild, out of the tree, names them by absolute path.)
KMAKE = $(MAKE) -C linux O=$(CURDIR)/$(KOUT) ARCH=wasm LLVM=1 \
	LOCALVERSION=-ksee KBUILD_BUILD_USER=ksee KBUILD_BUILD_HOST=ksee KBUILD_BUILD_VERSION=1 KBUILD_BUILD_TIMESTAMP="$(shell LC_ALL=C date -u -d @$(LINUX_DATE))" \
	KCFLAGS="-g -ffile-prefix-map=$(CURDIR)/linux/= -finline-hint-functions -fpass-plugin=$(CURDIR)/$(PLUGIN) -DKSEE_ID=$(shell cksum < plugin/ksee-trace.cpp | cut -d' ' -f1)"

# (KERNEL_BUILT=true: the kernel's products are here already, as built,
# CI's cache's (the split kernel, the UAPI headers, gen_init_cpio, the
# sources the page shows): no kbuild, nor its tree.)
ifeq ($(KERNEL_BUILT),true)
$(KOUT)/vmlinux $(KOUT)/usr/gen_init_cpio target/vmlinux target/vmlinux.debug $(SITE)/src: ;
else

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

# (Every Kconfig the port has: a new one runs defconfig again; touched, as
# kbuild leaves a .config the same as it was.)
$(KOUT)/.config: arch/wasm/configs/defconfig $(wildcard arch/wasm/Kconfig*) | linux/Makefile
	$(KMAKE) defconfig
	touch $@

# (kbuild knows the kernel's own dependencies: it is always asked; it
# builds gen_init_cpio too, the kernel's own initramfs writer.)
$(KOUT)/usr/gen_init_cpio: $(KOUT)/vmlinux ;
$(KOUT)/vmlinux: $(PLUGIN) $(KOUT)/.config
	$(KMAKE)
	$(KMAKE) headers

# The kernel, split: the page's, and its description (DWARF, and the
# plugin's ksee section).
target/vmlinux: $(KOUT)/vmlinux
	llvm-objcopy --strip-debug --remove-section=ksee $< $@.new
	$(replace)
target/vmlinux.debug: $(KOUT)/vmlinux
	llvm-objcopy --only-keep-debug --keep-section=ksee $< $@.new
	$(replace)

# The sources its description names, for the page (again for a new ksee).
$(SITE)/src: target/vmlinux.debug $(KSEE)
	$(KSEE) sources $< linux $@
endif

# ---- The programs and the disk ------------------------------------------------

# The programs: position-independent, in the kernel's memory (see
# binfmt_wasm.c), their syscalls user/syscall.h's, with the UAPI headers.
$(OUT)/%.wasm: user/%.c user/syscall.h | $(KOUT)/vmlinux
	@mkdir -p $(@D)
	clang --target=wasm32 -O2 -g0 -ffreestanding -nostdlib -fPIC -Wl,-pie -Wl,--experimental-pic \
		-Wl,--import-memory -Wl,--no-entry -nostdinc -isystem $(KOUT)/usr/include -o $@ $<

# The archive (all times 0): the console (the kernel opens it before init),
# the programs, the note.
$(OUT)/initramfs.cpio: user/initramfs.list $(KOUT)/usr/gen_init_cpio $(OUT)/init.wasm $(OUT)/prog.wasm user/note.txt
	$(KOUT)/usr/gen_init_cpio -t 0 $< > $@

# The disk /init mounts (/dev/wda: its blocks the host's), ext4: user/disk/,
# the files the lessons start from; blocks of 4 KiB, a page each (the page
# cache's unit: one block to one page, as most systems have it), 8 MiB (the
# journal's room); initialized in full: no background work after the mount
# (ext4lazyinit's). The same each time: made anew (no blocks of the last
# left), its UUID and hash seed fixed, its times the release's.
$(OUT)/disk.img: $(shell find user/disk) Makefile
	@mkdir -p $(@D)
	rm -f $@
	SOURCE_DATE_EPOCH=$(LINUX_DATE) mkfs.ext4 -q -F -b 4096 -L ksee -N 32 -d user/disk \
		-U 6b736565-0000-4000-8000-000000000001 -E lazy_itable_init=0,lazy_journal_init=0,hash_seed=6b736565-0000-4000-8000-000000000002 $@ 8M

# ---- The site ------------------------------------------------------------------

# The kernel's tables (again for a new ksee).
target/tables: target/vmlinux.debug $(KSEE)
	$(KSEE) tables $< $(KOUT)/usr/include $@

# The site's files, gzipped: the kernel's and its tables (target/), the
# initramfs and the disk (target/user/).
gzip = @mkdir -p $(@D); gzip -9nc $< > $@.new && mv $@.new $@
$(SITE)/%.gz: target/%
	$(gzip)
$(SITE)/%.gz: $(OUT)/%
	$(gzip)

# (Each time: the pages say the commit they are of.)
lessons:
	@mkdir -p $(SITE)
	$(KSEE) lessons

# (Each time: cargo knows what the module is of.)
page:
	cargo build --locked --release -p ksee-page --target wasm32-unknown-unknown
	$(KSEE) bindgen target/wasm32-unknown-unknown/release/ksee_page.wasm $(SITE)/pkg

# ---- Upkeep --------------------------------------------------------------------

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
