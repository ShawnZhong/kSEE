# The build. `make site` (what `ksee build` runs) makes target/site/, which
# `ksee serve` serves and `ksee deploy` publishes. Steps that need code are
# ksee commands (crates/ksee-cli).
#
#   linux/                  Linux at LINUX_TAG with linux.patch applied and arch/wasm/
#                           linked in (sources only; `make patch` rewrites linux.patch)
#   target/ksee-trace.so    the LLVM pass that instruments the kernel (plugin/)
#   target/linux/           kbuild's output: vmlinux, the UAPI headers, gen_init_cpio
#   target/vmlinux          the kernel for the page (no DWARF, no ksee section)
#   target/vmlinux.debug    the DWARF and ksee section, for `ksee tables`
#   target/user/            the programs (user/), the initramfs and the disk image
#   target/tables           the kernel's tables (`ksee tables`)
#   target/site/            the site: the gzipped files, src/ (`ksee sources`), the
#                           lessons' pages (`ksee lessons`), pkg/ (the page's module)
#   target/chrome/          the tests' browser (`make chrome`)

# ---- What is pinned ------------------------------------------------------------

# The Linux release the port patches, and its date (used as the build date).
LINUX_TREE := https://git.kernel.org/pub/scm/linux/kernel/git/stable/linux.git
LINUX_TAG := v7.2.7
LINUX_COMMIT := f42acb3678424d1e08f6ed27c0d8ba8a125e14d6
LINUX_DATE := 1789996140
# The tests' browser (Chrome for Testing, from Playwright's mirror: it has arm64).
CHROME_VERSION := 156.0.8078.4

# ---- Where things go -----------------------------------------------------------

# LLVM 22 (setup.sh's) first.
export PATH := /usr/lib/llvm-22/bin:$(PATH)
PLUGIN := target/ksee-trace.so
KOUT := target/linux
# (user/initramfs.list uses OUT.)
export OUT := target/user
SITE := target/site
# The ksee command (`ksee build` passes itself).
KSEE ?= target/release/ksee

# Replace $@ with $@.new only if it differs, so that what depends on $@ is
# not rebuilt when kbuild (always run) changed nothing.
replace = cmp -s $@.new $@ && rm $@.new || mv $@.new $@

.PHONY: site lessons page $(KOUT)/vmlinux patch llvm chrome

site: $(SITE)/vmlinux.gz $(SITE)/tables.gz $(SITE)/src $(SITE)/initramfs.cpio.gz $(SITE)/disk.img.gz lessons page

# ---- Linux ---------------------------------------------------------------------

llvm:
	@v=$$(llvm-config --version); test "$${v%%.*}" = 22 || { echo "LLVM 22 is required, found $$v" >&2; exit 1; }

$(PLUGIN): plugin/ksee-trace.cpp | llvm
	@mkdir -p $(@D)
	clang++ -shared -fPIC -O2 $< -o $@ $$(llvm-config --cxxflags)

# kbuild, out of the tree. The build is reproducible: fixed date, version,
# user and host. KSEE_ID (a hash of the pass) makes kbuild rebuild when the
# instrumentation changes. Only functions marked `inline` are inlined, so
# calls stay visible. -ffile-prefix-map names files as fs/read_write.c.
KMAKE = $(MAKE) -C linux O=$(CURDIR)/$(KOUT) ARCH=wasm LLVM=1 \
	LOCALVERSION=-ksee KBUILD_BUILD_USER=ksee KBUILD_BUILD_HOST=ksee KBUILD_BUILD_VERSION=1 KBUILD_BUILD_TIMESTAMP="$(shell LC_ALL=C date -u -d @$(LINUX_DATE))" \
	KCFLAGS="-g -ffile-prefix-map=$(CURDIR)/linux/= -finline-hint-functions -fpass-plugin=$(CURDIR)/$(PLUGIN) -DKSEE_ID=$(shell cksum < plugin/ksee-trace.cpp | cut -d' ' -f1)"

# KERNEL_BUILT=true: the kernel's products are already here (CI restores
# them from its cache), so neither kbuild nor the Linux tree is needed.
ifeq ($(KERNEL_BUILT),true)
$(KOUT)/vmlinux $(KOUT)/usr/gen_init_cpio target/vmlinux target/vmlinux.debug $(SITE)/src: ;
else

# Fetch the release (one commit), check it, apply linux.patch. Only once:
# to apply an updated patch, remove linux/.
linux/Makefile:
	mkdir -p linux
	git -C linux init -q
	git -C linux fetch --depth=1 $(LINUX_TREE) tag $(LINUX_TAG)
	test "$$(git -C linux rev-parse '$(LINUX_TAG)^{commit}')" = $(LINUX_COMMIT)
	git -C linux checkout -q $(LINUX_TAG)
	git -C linux apply ../linux.patch
	ln -s ../../arch/wasm linux/arch/wasm

# A changed Kconfig runs defconfig again (touched: kbuild may leave it as it was).
$(KOUT)/.config: arch/wasm/configs/defconfig $(wildcard arch/wasm/Kconfig*) | linux/Makefile
	$(KMAKE) defconfig
	touch $@

# kbuild tracks the kernel's dependencies, so it always runs. It also builds
# gen_init_cpio, Linux's initramfs writer.
$(KOUT)/usr/gen_init_cpio: $(KOUT)/vmlinux ;
$(KOUT)/vmlinux: $(PLUGIN) $(KOUT)/.config
	$(KMAKE)
	$(KMAKE) headers

# Split the kernel: the page's, and its DWARF and ksee section.
target/vmlinux: $(KOUT)/vmlinux
	llvm-objcopy --strip-debug --remove-section=ksee $< $@.new
	$(replace)
target/vmlinux.debug: $(KOUT)/vmlinux
	llvm-objcopy --only-keep-debug --keep-section=ksee $< $@.new
	$(replace)

# The sources the tables name, for the page's source view.
$(SITE)/src: target/vmlinux.debug $(KSEE)
	$(KSEE) sources $< linux $@
endif

# ---- The programs and the disk ------------------------------------------------

# The programs: position-independent, loaded into kernel memory (binfmt_wasm.c).
$(OUT)/%.wasm: user/%.c user/syscall.h | $(KOUT)/vmlinux
	@mkdir -p $(@D)
	clang --target=wasm32 -O2 -g0 -ffreestanding -nostdlib -fPIC -Wl,-pie -Wl,--experimental-pic \
		-Wl,--import-memory -Wl,--no-entry -nostdinc -isystem $(KOUT)/usr/include -o $@ $<

# The initramfs (all times 0).
$(OUT)/initramfs.cpio: user/initramfs.list $(KOUT)/usr/gen_init_cpio $(OUT)/init.wasm $(OUT)/prog.wasm user/note.txt
	$(KOUT)/usr/gen_init_cpio -t 0 $< > $@

# The disk /init mounts (/dev/wda), ext4 of user/disk/. 4 KiB blocks (one
# per page-cache page), 8 MiB (room for the journal), fully initialized (no
# background work after mounting). Reproducible: made anew, with a fixed
# UUID, hash seed and dates.
$(OUT)/disk.img: $(shell find user/disk) Makefile
	@mkdir -p $(@D)
	rm -f $@
	SOURCE_DATE_EPOCH=$(LINUX_DATE) mkfs.ext4 -q -F -b 4096 -L ksee -N 32 -d user/disk \
		-U 6b736565-0000-4000-8000-000000000001 -E lazy_itable_init=0,lazy_journal_init=0,hash_seed=6b736565-0000-4000-8000-000000000002 $@ 8M

# ---- The site ------------------------------------------------------------------

# The kernel's tables (made again for a new ksee).
target/tables: target/vmlinux.debug $(KSEE)
	$(KSEE) tables $< $(KOUT)/usr/include $@

# The site's files, gzipped (hosts compress them less or not at all).
gzip = @mkdir -p $(@D); gzip -9nc $< > $@.new && mv $@.new $@
$(SITE)/%.gz: target/%
	$(gzip)
$(SITE)/%.gz: $(OUT)/%
	$(gzip)

# Always: the pages name the commit.
lessons:
	@mkdir -p $(SITE)
	$(KSEE) lessons

# Always: cargo tracks its own dependencies.
page:
	cargo build --locked --release -p ksee-page --target wasm32-unknown-unknown
	$(KSEE) bindgen target/wasm32-unknown-unknown/release/ksee_page.wasm $(SITE)/pkg

# ---- Upkeep --------------------------------------------------------------------

# Write linux.patch: linux/'s edits to Linux's own files (arch/wasm/ is untracked).
patch:
	@test -f linux/Makefile || { echo "no linux/: run ./ksee.sh build first" >&2; exit 1; }
	git -C linux reset -q
	git -C linux diff --no-color $(LINUX_TAG) > linux.patch

# Fetch Chrome's headless shell for this machine (again when CHROME_VERSION changes).
chrome: target/chrome/$(CHROME_VERSION)
target/chrome/$(CHROME_VERSION):
	rm -rf target/chrome target/chrome.zip
	a=$$(uname -m | sed 's/x86_64/linux64/; s/aarch64/linux-arm64/'); \
	curl -fsSL --proto '=https' -o target/chrome.zip https://cdn.playwright.dev/builds/cft/$(CHROME_VERSION)/$$a/chrome-headless-shell-$$a.zip && \
	unzip -q -d target target/chrome.zip && mv target/chrome-headless-shell-$$a target/chrome
	rm target/chrome.zip
	touch $@
