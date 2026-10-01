#!/bin/sh
# kSEE's front door: builds the ksee command (crates/ksee-cli), which builds
# the rest and serves the page.
#   ./ksee.sh              build, then serve http://127.0.0.1:8000/
#   ./ksee.sh build        build
#   ./ksee.sh test [--ui]  build, then run the tests (in Chromium, $CHROME)
#   ./ksee.sh shot OUT.png [--at=#ex=read] [--eval=JS]
#                          the page in Chromium, as a screenshot
#   ./ksee.sh deploy       build, then push the page to gh-pages (GitHub Pages)
#   ./ksee.sh type NAME    the layout of a kernel type (e.g. task_struct)
set -e
cd "$(dirname "$0")"
# The LLVM, lld (wasm-ld), GNU make and mkfs.ext4 setup.sh installs, ahead
# of the system's: Homebrew's, or apt.llvm.org's (versioned names; the plain
# ones in its dir).
if command -v brew >/dev/null; then
	b=$(brew --prefix)
	PATH="$b/opt/llvm/bin:$b/opt/lld/bin:$b/opt/make/libexec/gnubin:$b/opt/e2fsprogs/sbin:$PATH"
elif l=$(ls -d /usr/lib/llvm-*/bin 2>/dev/null | sort -V | tail -1) && [ -n "$l" ]; then
	PATH="$l:$PATH"
fi
# The SDK the linker reads: this CommandLineTools' TAPI knows no arm64e.x1,
# so MacOSX27's .tbd files choke it; build against the newest SDK it reads.
if [ -z "$SDKROOT" ]; then
	if echo 'int main(void){return 0;}' | cc -x c -o /dev/null - 2>/dev/null; then
		: # the default SDK links: leave it alone
	else
		for s in $(ls -d /Library/Developer/CommandLineTools/SDKs/MacOSX*.sdk 2>/dev/null | sort -r); do
			if echo 'int main(void){return 0;}' | SDKROOT=$s cc -x c -o /dev/null - 2>/dev/null; then
				SDKROOT=$s; export SDKROOT
				break
			fi
		done
	fi
fi
cargo build -q --release -p ksee-cli
case "$1" in
type) exec target/release/ksee type target/vmlinux "$2" ;;
*) exec target/release/ksee "$@" ;;
esac
