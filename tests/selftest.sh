#!/bin/sh
# Boot the kernel with its task-switching selftest and check that it passes.
cd "$(dirname "$0")/.."
out=$(node host/node.js --timeout 30 linux/vmlinux wasm_selftest 2>&1)
if echo "$out" | grep -q "selftest: passed"; then
	echo "$out" | grep "selftest:"
	echo PASS
else
	echo "$out"
	echo FAIL
	exit 1
fi
