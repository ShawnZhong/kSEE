#!/bin/sh
# What ./ksee.sh needs, on Ubuntu 26.04: the kernel's build tools, LLVM 22
# (clang, lld's wasm-ld, the plugin's headers and llvm-config), mkfs.ext4
# and gzip, and Rust (rust-toolchain.toml adds its wasm target).
# ./ksee.sh test also needs Chromium, in $CHROME.
set -e
sudo apt-get update
sudo apt-get install -y --no-install-recommends \
	make flex bison bc git \
	clang-22 lld-22 llvm-22-dev \
	e2fsprogs gzip \
	curl ca-certificates
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
