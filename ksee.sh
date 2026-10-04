#!/bin/sh
# kSEE's front door: the ksee command (crates/ksee-cli), built if need be, run.
#   ./ksee.sh [COMMAND]   no command: build, then serve http://127.0.0.1:8000/;
#                         the commands: crates/ksee-cli/src/main.rs
set -e
cd "$(dirname "$0")"
exec cargo run -q --locked --release -p ksee-cli -- "$@"
