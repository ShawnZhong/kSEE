// /init: the first process, and the page's runner. It mounts the ext4
// disk (the image /disk.img, through the loop device, on /mnt), then makes
// the syscalls the page asks for, one at a time: it waits reading its
// console line, and a byte there says a call is asked for (see the page's
// Machine.call). A clone child of it (a thread, or vfork's child) starts
// at __wasm_clone_start and is a runner too, on a line of its own; so is
// /init exec'd again. Plain C on nolibc (linux/tools/include/nolibc), the
// kernel's small C library.
#include <fcntl.h>
#include <stdio.h>
#include <sys/ioctl.h>
#include <sys/mount.h>
#include <unistd.h>
#include <linux/loop.h>

// This runner's console line (0: the first /init's); ready for calls; the
// call asked for (the runtime fills it in; buffer arguments point into
// data), and its result.
__attribute__((import_module("ksee"), import_name("line")))
int ksee_line(void);
__attribute__((import_module("ksee"), import_name("ready")))
void ksee_ready(void);
__attribute__((import_module("ksee"), import_name("call")))
int ksee_call(void *req);
__attribute__((import_module("ksee"), import_name("done")))
void ksee_done(long long ret, void *req);

struct request {
	int nr;
	int args[6];
	int pad;
	char data[4096];
};

// (No background work after the mount: the image is initialized in full,
// and ext4lazyinit need not prefetch its block bitmaps.)
static void disk(void)
{
	struct loop_config config = { 0 };
	int loop;

	config.fd = open("/disk.img", O_RDWR);
	loop = open("/dev/loop0", O_RDWR);
	if (config.fd < 0 || loop < 0 || ioctl(loop, LOOP_CONFIGURE, &config) ||
	    mount("/dev/loop0", "/mnt", "ext4", 0, "no_prefetch_block_bitmaps"))
		printf("init: no ext4 disk on /mnt\n");
	// (The loop device holds the file; the page's fds start at 3.)
	close(config.fd);
	close(loop);
}

// Make the page's calls, from console line `line` (its fd out of the way
// of the page's, and not inherited by a program exec'd).
static void __attribute__((noreturn)) serve(int line)
{
	char tty[] = "/dev/ttyW0", c;
	struct request req;
	int fd, bell;

	tty[9] += line;
	fd = open(tty, O_RDONLY | O_CLOEXEC);
	bell = dup3(fd, 64 + line, O_CLOEXEC);
	close(fd);
	ksee_ready();
	for (;;) {
		if (read(bell, &c, 1) != 1 || ksee_call(&req) < 0)
			continue;
		ksee_done(__nolibc_syscall6(req.nr, req.args[0], req.args[1], req.args[2],
					    req.args[3], req.args[4], req.args[5]), &req);
	}
}

int main(void)
{
	int line = ksee_line();

	if (line == 0) {
		mount("devtmpfs", "/dev", "devtmpfs", 0, NULL);
		disk();
		printf("init: ready for the page's calls\n");
	}
	serve(line);
}

// Where a clone child starts (arch/wasm: it cannot return from clone), on
// the stack it was given, or below its parent's (vfork).
__attribute__((export_name("__wasm_clone_start")))
void clone_start(void)
{
	serve(ksee_line());
}

// 128-bit multiply, which clang emits as a libcall on wasm32 (for 64-bit
// division by constants, as in printf); there is no compiler-rt.
__int128 __multi3(__int128 a, __int128 b)
{
	union { __int128 ti; struct { unsigned long long low, high; } s; } x = { a }, y = { b }, r;
	unsigned long long a0 = (unsigned)x.s.low, a1 = x.s.low >> 32;
	unsigned long long b0 = (unsigned)y.s.low, b1 = y.s.low >> 32;
	unsigned long long p00 = a0 * b0, p01 = a0 * b1, p10 = a1 * b0, p11 = a1 * b1;
	unsigned long long mid = (p00 >> 32) + (unsigned)p01 + (unsigned)p10;

	r.s.low = (mid << 32) | (unsigned)p00;
	r.s.high = p11 + (p01 >> 32) + (p10 >> 32) + (mid >> 32) + x.s.high * y.s.low + x.s.low * y.s.high;
	return r.ti;
}
