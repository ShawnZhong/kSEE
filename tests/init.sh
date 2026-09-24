#!/bin/sh
# Boot to /init from the initramfs; it prints its pid and powers off.
cd "$(dirname "$0")/.."
out=$(node host/node.js --timeout 30 --initrd build/initramfs.cpio \
	linux/vmlinux 2>&1)
status=$?
if [ $status -eq 0 ] && echo "$out" | grep -q "Hello from wasm userspace! I am pid 1"; then
	echo "$out" | grep "Hello"
	echo PASS
else
	echo "$out"
	echo "FAIL (exit $status)"
	exit 1
fi
