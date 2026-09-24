#!/bin/sh
# kSEE's front door: builds the ksee command (crates/ksee-cli), which builds
# the rest and serves the page.
#   ./ksee.sh              build, then serve http://127.0.0.1:8000/
#   ./ksee.sh build        build
#   ./ksee.sh test [--ui]  build, then run the tests
#   ./ksee.sh type NAME    the layout of a kernel type (e.g. task_struct)
set -e
cd "$(dirname "$0")"
cargo build -q --release -p ksee-cli
case "$1" in
type) exec target/release/ksee type web/vmlinux "$2" ;;
*) exec target/release/ksee "$@" ;;
esac
