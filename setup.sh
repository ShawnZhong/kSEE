#!/bin/sh
# What ./ksee.sh needs, on Ubuntu 26.04 (safe to run again):
#   the kernel's build tools, LLVM 22 (clang, lld's wasm-ld, the plugin's
#   headers, llvm-config), mkfs.ext4 and gzip;
#   Rust (rust-toolchain.toml adds its wasm target);
#   the tests' browser: Chrome's headless shell at CHROME_VERSION, in
#   target/chrome/ (Chrome for Testing, from Playwright's mirror: it has
#   arm64 too), the libraries it links (readelf -d shows them), and a font
#   (the tests measure text).
# `./setup.sh chrome`: the browser only (CI's: its image has the rest).
set -e
cd "$(dirname "$0")"
CHROME_VERSION=156.0.8078.4

if [ "$1" != chrome ]; then
sudo apt-get update
sudo apt-get install -y --no-install-recommends \
	make flex bison bc git \
	clang-22 lld-22 llvm-22-dev \
	e2fsprogs gzip \
	curl ca-certificates unzip \
	libnss3 libgbm1 libasound2t64 libatk1.0-0t64 libatspi2.0-0t64 libdbus-1-3 \
	libxcomposite1 libxdamage1 libxrandr2 libxkbcommon0 fonts-dejavu-core
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
fi

# The browser, for this machine (again when CHROME_VERSION changes).
if [ ! -f "target/chrome/$CHROME_VERSION" ]; then
	a=$(uname -m | sed 's/x86_64/linux64/; s/aarch64/linux-arm64/')
	rm -rf target/chrome target/chrome.zip
	mkdir -p target
	curl -fsSL --proto '=https' -o target/chrome.zip \
		"https://cdn.playwright.dev/builds/cft/$CHROME_VERSION/$a/chrome-headless-shell-$a.zip"
	unzip -q -d target target/chrome.zip
	mv "target/chrome-headless-shell-$a" target/chrome
	rm target/chrome.zip
	touch "target/chrome/$CHROME_VERSION"
fi
