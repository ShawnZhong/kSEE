#!/bin/sh
# kSEE: build the site (web/) and serve it.
#   ./ksee.sh              build what changed, then serve at http://127.0.0.1:8000/
#   ./ksee.sh build        build
#   ./ksee.sh test         build, then run the tests
#   ./ksee.sh type NAME    the layout of a kernel type (e.g. task_struct)
set -e
cd "$(dirname "$0")"
case "$1" in
build) exec make -s ;;
test) exec make -s test ;;
type) make -s target/release/ksee && exec target/release/ksee type web/vmlinux "$2" ;;
"" | serve) make -s && exec target/release/ksee serve web ;;
*) sed -n '2,7s/^# //p' "$0"; exit 2 ;;
esac
