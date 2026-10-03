#!/bin/sh
# What ./ksee.sh needs: LLVM 23 (clang, lld's wasm-ld, the pass's headers),
# GNU make and sed, mkfs.ext4, gzip, Rust and its wasm target; from Homebrew
# on macOS or apt on Ubuntu. ./ksee.sh test also needs Chromium, in $CHROME.
set -e
cd "$(dirname "$0")"
case "$(uname -s)" in
Darwin)
	command -v brew >/dev/null || {
		echo "setup.sh: Homebrew is required on macOS" >&2
		exit 1
	}
	brew install llvm@23 lld@23 make gnu-sed e2fsprogs
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
	s=; [ "$(id -u)" = 0 ] || s=sudo
	$s apt-get update
	$s apt-get install -y --no-install-recommends build-essential flex bison bc git curl ca-certificates e2fsprogs gzip
	$s install -d -m 0755 /etc/apt/keyrings
	curl -fsSL https://apt.llvm.org/llvm-snapshot.gpg.key | $s tee /etc/apt/keyrings/apt.llvm.org.asc >/dev/null
	printf '%s\n' 'deb [signed-by=/etc/apt/keyrings/apt.llvm.org.asc] https://apt.llvm.org/resolute/ llvm-toolchain-resolute-23 main' |
		$s tee /etc/apt/sources.list.d/llvm-23.list >/dev/null
	$s apt-get update
	$s apt-get install -y --no-install-recommends clang-23 lld-23 llvm-23-dev
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
