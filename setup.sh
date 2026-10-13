#!/bin/sh
# Install what ./ksee.sh needs on Ubuntu 26.04. Safe to run again.
set -e

# Build tools and LLVM 22.
sudo apt-get update
sudo apt-get install -y --no-install-recommends \
	make flex bison bc git \
	clang-22 lld-22 llvm-22-dev \
	curl ca-certificates

# Rust.
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal

# Google Chrome, for the tests.
if ! command -v google-chrome >/dev/null; then
	deb=$(mktemp --suffix=.deb)
	curl -fsSL -o "$deb" "https://dl.google.com/linux/direct/google-chrome-stable_current_$(dpkg --print-architecture).deb"
	sudo apt-get install -y "$deb"
	rm "$deb"
fi
