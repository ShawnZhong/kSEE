#!/bin/sh
# Record the selftest boot and check the trace: the scheduler at work (ping
# and pong switching to each other) and the code init's syscalls ran.
cd "$(dirname "$0")/.."
dump=build/trace.dump ksee=target/release/ksee
node host/node.js --timeout 30 --initrd build/initramfs.cpio --dump $dump \
	linux/vmlinux wasm_selftest >/dev/null 2>&1 || { echo "FAIL (boot)"; exit 1; }
$ksee records linux/vmlinux $dump > build/records.txt
$ksee syscalls linux/vmlinux $dump --blocks > build/syscalls.txt
check() {
	if grep -q "$3" "$2"; then echo "ok: $1"; else echo "FAIL: $1"; exit 1; fi
}
check "ping -> pong" build/records.txt '> context_switch(0x[0-9a-f]*, ping([0-9]*), pong('
check "pong -> ping" build/records.txt '> context_switch(0x[0-9a-f]*, pong([0-9]*), ping('
check "write ran vfs_write" build/syscalls.txt '^ *vfs_write  *fs/read_write.c:[0-9]*:[0-9]*$'
check "write returned 36" build/syscalls.txt 'init(1) syscall 64(0x1, 0x[0-9a-f]*, 0x24) = 36'
check "getpid ran __task_pid_nr_ns" build/syscalls.txt '^ *__task_pid_nr_ns '
echo "PASS ($(wc -l < build/records.txt) records, $(grep -c '^\[' build/syscalls.txt) syscalls)"
