// /bin/prog: the page's program, a process as any (not /init: pid 1 is
// special, it may not exit, it ignores the signals it has no handler for).
// It makes the syscalls the page asks for, one at a time, reading each from
// its stdin, the console (the host's input: see crates/ksee-host's
// machine.rs, call). A clone child of it
// (a thread, or vfork's child) starts at __wasm_clone_start and is a runner
// too. /init starts it, and again when it exits. Plain C, with no C
// library: its syscalls as syscall.h makes them.
#include "syscall.h"

// A call, as the console gives it: its number and its arguments. The host
// records it from the kernel's syscall entry to its return.
struct request {
	int nr;
	int args[6];
};

// The program's own memory, which the host hands out (each variable
// aligned as it needs): its variables, `char buf[64]` in the page's
// program, and its calls' strings and compound literals (as C's, there
// for as long as the program); a process's, its threads' too.
static char heap[65536];

// (main: whether this runner is the program's own, its main thread, not a
// clone child's.)
__attribute__((import_module("ksee"), import_name("ready")))
void ksee_ready(char *heap, int size, int main);

// Make the page's calls, one at a time, as each comes on stdin.
static void __attribute__((noreturn)) serve(int main)
{
	struct request req;

	ksee_ready(heap, sizeof heap, main);
	for (;;) {
		if (sys(__NR_read, 0, &req, sizeof req) != sizeof req)
			continue;
		sys(req.nr, req.args[0], req.args[1], req.args[2],
		    req.args[3], req.args[4], req.args[5]);
	}
}

// Where the program starts (the host enters it: no arguments).
__attribute__((export_name("_start")))
void start(void)
{
	serve(1);
}

// Where a clone child starts (arch/wasm: it cannot return from clone), on
// the stack it was given, or below its parent's (vfork).
__attribute__((export_name("__wasm_clone_start")))
void clone_start(void)
{
	serve(0);
}
