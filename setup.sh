#!/bin/sh
# Install what ./ksee.sh needs on Ubuntu 26.04. Safe to run again.
set -e
cd "$(dirname "$0")"
CHROME_VERSION=156.0.8078.4

# The kernel's build, LLVM 22, the downloads; then the libraries Chrome's
# headless shell links, and a font for the tests.
sudo apt-get update
sudo apt-get install -y --no-install-recommends \
	make flex bison bc git \
	clang-22 lld-22 llvm-22-dev \
	curl ca-certificates unzip \
	libnss3 libgbm1 libasound2t64 libatk1.0-0t64 libatspi2.0-0t64 libdbus-1-3 \
	libxcomposite1 libxdamage1 libxrandr2 libxkbcommon0 fonts-dejavu-core
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal

# Chrome for Testing's headless shell (Playwright's mirror has arm64 builds).
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
