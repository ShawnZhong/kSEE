// /bin/prog: the page's program, a process as any (not /init: pid 1 is
// special, it may not exit, it ignores the signals it has no handler for).
// It makes the syscalls the page asks for, one at a time: it waits in a
// futex on a word of its own, which the host sets when a call is asked for
// (and rings the task: see the page's Machine.call). A clone child of it
// (a thread, or vfork's child) starts at __wasm_clone_start and is a runner
// too. /init starts it, and again when it exits. Plain C on nolibc
// (linux/tools/include/nolibc), the kernel's small C library.
#include <linux/futex.h>
#include <unistd.h>

// Ready for calls: the host puts a call in *req (its number, -1 while
// none is asked; its arguments); the runner makes it between begin and end
// (recorded from the one to the other), and end gets the result.
struct request {
	volatile int nr;
	int args[6];
};

// The program's own memory, which the host hands out: its variables
// (`char buf[64]` in the page's program), and each call's strings and
// buffers. A process's, its threads' too.
static char heap[65536] __attribute__((aligned(16)));

// (main: whether this runner is the program's own, its main thread, not a
// clone child's.)
__attribute__((import_module("ksee"), import_name("ready")))
void ksee_ready(struct request *req, char *heap, int size, int main);
__attribute__((import_module("ksee"), import_name("begin")))
void ksee_begin(void);
__attribute__((import_module("ksee"), import_name("end")))
void ksee_end(long long ret);

// Make the page's calls, one at a time, as each is asked: the host puts
// the call in `req` (its number: the word the wait is on) and wakes the
// task; the wait sees the word changed, and returns.
static void __attribute__((noreturn)) serve(int main)
{
	struct request req;

	req.nr = -1;
	ksee_ready(&req, heap, sizeof heap, main);
	for (;;) {
		while (req.nr < 0)
			__nolibc_syscall4(__NR_futex_time64, &req.nr, FUTEX_WAIT_PRIVATE, -1, NULL);
		ksee_begin();
		long long ret = __nolibc_syscall6(req.nr, req.args[0], req.args[1], req.args[2],
						  req.args[3], req.args[4], req.args[5]);
		req.nr = -1;
		ksee_end(ret);
	}
}

int main(void)
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
