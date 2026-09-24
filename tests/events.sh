#!/bin/sh
# Record the selftest boot and check the trace: the scheduler at work (ping
# and pong switching to each other) and the code init's syscalls ran.
cd "$(dirname "$0")/.."
events=build/events.jsonl syscalls=build/syscalls.jsonl
node host/node.js --timeout 30 --initrd build/initramfs.cpio \
	--events $events --syscalls $syscalls linux/vmlinux wasm_selftest \
	>/dev/null 2>&1 || { echo "FAIL (boot)"; exit 1; }
check() {
	if grep -q "$3" "$2"; then echo "ok: $1"; else echo "FAIL: $1"; exit 1; fi
}
check "ping -> pong" $events '"fn":"context_switch".*"comm":"ping"}.*"comm":"pong"}'
check "pong -> ping" $events '"fn":"context_switch".*"comm":"pong"}.*"comm":"ping"}'
check "block locations" $events '"kind":"block","at":\["[a-z_]*","[^"]*:[0-9]*:[0-9]*"\]'
check "write ran vfs_write" $syscalls '"comm":"init"},"nr":64,.*"ret":36,.*\["vfs_write",'
check "getpid ran __task_pid_nr_ns" $syscalls '"nr":172,.*\["__task_pid_nr_ns",'
echo "PASS ($(wc -l < $events) records, $(wc -l < $syscalls) syscalls)"
