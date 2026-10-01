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
# The LLVM, GNU make and mkfs.ext4 setup.sh installs, ahead of the system's:
# Homebrew's, or apt.llvm.org's (versioned names; the plain ones in its dir).
if command -v brew >/dev/null; then
	b=$(brew --prefix)
	PATH="$b/opt/llvm/bin:$b/opt/make/libexec/gnubin:$b/opt/e2fsprogs/sbin:$PATH"
elif l=$(ls -d /usr/lib/llvm-*/bin 2>/dev/null | sort -V | tail -1) && [ -n "$l" ]; then
	PATH="$l:$PATH"
fi
cargo build -q --release -p ksee-cli
case "$1" in
type) exec target/release/ksee type target/vmlinux "$2" ;;
*) exec target/release/ksee "$@" ;;
esac
