#!/bin/sh
# kSEE's front door: builds the ksee command (crates/ksee-cli), then runs it.
#   ./ksee.sh [COMMAND]   no command: build, then serve http://127.0.0.1:8000/;
#                         the commands: crates/ksee-cli/src/main.rs
set -e
cd "$(dirname "$0")"
cargo build -q --locked --release -p ksee-cli
exec target/release/ksee "$@"
