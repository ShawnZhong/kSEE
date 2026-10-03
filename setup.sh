#!/bin/sh
# What ./ksee.sh needs: LLVM 22 (clang, lld's wasm-ld, the pass's headers),
# GNU make and sed, mkfs.ext4, gzip, Rust, its wasm target and wasm-bindgen;
# from Homebrew or apt plus Cargo. ./ksee.sh test also needs Chromium, in $CHROME.
set -e
cd "$(dirname "$0")"
case "$(uname -s)" in
Darwin)
	command -v brew >/dev/null || {
		echo "setup.sh: Homebrew is required on macOS" >&2
		exit 1
	}
	brew install llvm@22 lld@22 make gnu-sed e2fsprogs
	;;
Linux)
	command -v apt-get >/dev/null || {
		echo "setup.sh: apt is required on Linux" >&2
		exit 1
	}
	. /etc/os-release
	if [ "${ID:-}" != ubuntu ] || [ "${VERSION_CODENAME:-}" != resolute ]; then
		echo "setup.sh: Linux setup supports Ubuntu 26.04 (resolute)" >&2
		exit 1
	fi
	sudo apt-get update
	sudo apt-get install -y --no-install-recommends make flex bison bc git curl ca-certificates e2fsprogs gzip clang-22 lld-22 llvm-22-dev
	;;
*)
	echo "setup.sh: unsupported host: $(uname -s)" >&2
	exit 1
	;;
esac
if ! command -v rustup >/dev/null; then
	curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
	. "$HOME/.cargo/env"
fi
rustup target add wasm32-unknown-unknown
if ! command -v wasm-bindgen >/dev/null 2>&1 ||
	[ "$(wasm-bindgen --version 2>/dev/null)" != "wasm-bindgen 0.2.128" ]; then
	cargo install wasm-bindgen-cli --version 0.2.128 --locked --force
fi
