#!/bin/sh
# Record the selftest boot and check that the trace shows the scheduler at
# work: ping and pong switching to each other, and try_to_wake_up blocks.
cd "$(dirname "$0")/.."
trace=build/events.jsonl
node host/node.js --timeout 30 --initrd build/initramfs.cpio \
	--events $trace linux/vmlinux wasm_selftest >/dev/null 2>&1 || {
	echo "FAIL (boot)"; exit 1; }
check() {
	if grep -q "$2" $trace; then echo "ok: $1"; else echo "FAIL: $1"; exit 1; fi
}
check "ping -> pong" '"fn":"context_switch".*"comm":"ping"}.*"comm":"pong"}'
check "pong -> ping" '"fn":"context_switch".*"comm":"pong"}.*"comm":"ping"}'
check "block probes" '"kind":"block","fn":"try_to_wake_up".*"loc":"[0-9]*:[0-9]*"'
check "init syscalls" '"fn":"wasm_syscall","cur":{"pid":1,"comm":"init"}'
echo "PASS ($(wc -l < $trace) events)"
