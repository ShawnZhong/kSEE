#!/bin/sh
# What ./ksee.sh needs: LLVM 22 (clang, lld's wasm-ld, the pass's headers),
# make, mkfs.ext4, gzip, and Rust (rust-toolchain.toml adds its wasm
# target), on Ubuntu 26.04. ./ksee.sh test also needs Chromium, in $CHROME.
set -e
cd "$(dirname "$0")"
. /etc/os-release
if [ "${ID:-}" != ubuntu ] || [ "${VERSION_CODENAME:-}" != resolute ]; then
	echo "setup.sh: setup supports Ubuntu 26.04 (resolute)" >&2
	exit 1
fi
sudo apt-get update
sudo apt-get install -y --no-install-recommends make flex bison bc git curl ca-certificates e2fsprogs gzip clang-22 lld-22 llvm-22-dev
if ! command -v rustup >/dev/null; then
	curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
	. "$HOME/.cargo/env"
fi
