#!/bin/sh
# Build (if needed) and run the ksee command (crates/ksee-cli):
#   ./ksee.sh serve                    serve the web host at :8000/web/
#   ./ksee.sh syscalls VMLINUX DUMP    ... (see crates/ksee-cli/src/main.rs)
set -e
cd "$(dirname "$0")"
cargo build -q --release -p ksee-cli
exec target/release/ksee "$@"
