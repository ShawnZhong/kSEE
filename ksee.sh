#!/bin/sh
# kSEE's front door: builds the ksee command (crates/ksee-cli), then runs it.
#   ./ksee.sh                          build, then serve http://127.0.0.1:8000/
#   ./ksee.sh build                    build
#   ./ksee.sh test [--ui]              build, then the tests (in Chromium, $CHROME)
#   ./ksee.sh deploy                   build, then push the page to gh-pages
set -e
cd "$(dirname "$0")"
cargo build -q --release -p ksee-cli
exec target/release/ksee "$@"
