#!/bin/sh
# What ./ksee.sh needs: LLVM (clang, lld's wasm-ld, the pass's headers),
# GNU make, mkfs.ext4, Rust and its wasm target; from Homebrew (macOS) or
# apt (Debian, Ubuntu: LLVM from apt.llvm.org). ./ksee.sh test also needs
# Chromium, in $CHROME.
set -e
cd "$(dirname "$0")"
if command -v brew >/dev/null; then
	brew install llvm lld make e2fsprogs
elif command -v apt-get >/dev/null; then
	s=; [ "$(id -u)" = 0 ] || s=sudo
	$s apt-get update
	$s apt-get install -y build-essential flex bison bc git curl wget gnupg lsb-release software-properties-common e2fsprogs
	curl -fsSL https://apt.llvm.org/llvm.sh | $s bash -s 23
	$s apt-get install -y llvm-23-dev
else
	echo "setup.sh: no Homebrew or apt" >&2
	exit 1
fi
if ! command -v rustup >/dev/null; then
	curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
	. "$HOME/.cargo/env"
fi
rustup target add wasm32-unknown-unknown
