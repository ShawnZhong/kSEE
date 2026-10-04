#!/bin/sh
# What ./ksee.sh needs, on Ubuntu 26.04: the kernel's build tools, LLVM 22
# (clang, lld's wasm-ld, the plugin's headers and llvm-config), mkfs.ext4
# and gzip, Rust (rust-toolchain.toml adds its wasm target), and what the
# tests' browser needs: unzip, and the libraries Chrome's headless shell
# links (./ksee.sh test fetches the shell itself: see the Makefile).
set -e
sudo apt-get update
sudo apt-get install -y --no-install-recommends \
	make flex bison bc git \
	clang-22 lld-22 llvm-22-dev \
	e2fsprogs gzip \
	curl ca-certificates unzip \
	libnss3 libgbm1 libasound2t64 libatk1.0-0t64 libatspi2.0-0t64 libdbus-1-3 \
	libxcomposite1 libxdamage1 libxrandr2 libxkbcommon0 libxi6 libxres1 fonts-dejavu-core
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
