#!/bin/sh
# Boot the selftest in the web host (under Deno), then read the trace dump
# with the CLI: the scheduler at work and the code init's syscalls ran.
cd "$(dirname "$0")/.."
dump=build/trace.dump
deno run --allow-read --allow-write tests/web.js $dump wasm_selftest || exit 1
ksee() { ./ksee.sh "$@" linux/vmlinux $dump; }
ksee records > build/records.txt
ksee syscalls --blocks > build/syscalls.txt
check() {
	if grep -q "$3" "$2"; then echo "ok: $1"; else echo "FAIL: $1"; exit 1; fi
}
check "ping -> pong" build/records.txt '> context_switch(0x[0-9a-f]*, ping([0-9]*), pong('
check "pong -> ping" build/records.txt '> context_switch(0x[0-9a-f]*, pong([0-9]*), ping('
check "write ran vfs_write" build/syscalls.txt '^ *vfs_write  *fs/read_write.c:[0-9]*:[0-9]*$'
check "getpid ran __task_pid_nr_ns" build/syscalls.txt '^ *__task_pid_nr_ns '
echo "PASS ($(wc -l < build/records.txt) records, $(grep -c '^#' build/syscalls.txt) syscalls)"
