#!/bin/sh
# What ./ksee.sh needs (macOS, Homebrew): LLVM (clang, the pass's headers),
# lld (wasm-ld), GNU make, mkfs.ext4, and Rust's wasm target. For
# ./ksee.sh test, also Node and wasm-bindgen-test-runner (at Cargo.lock's
# wasm-bindgen), and Chromium, in $CHROME.
set -e
cd "$(dirname "$0")"
brew install llvm lld make e2fsprogs node
if ! command -v rustup >/dev/null; then
	curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
	. "$HOME/.cargo/env"
fi
rustup target add wasm32-unknown-unknown
v=$(grep -A1 '^name = "wasm-bindgen"$' Cargo.lock | sed -n 's/^version = "\(.*\)"/\1/p')
cargo install --locked wasm-bindgen-cli --version "$v"
