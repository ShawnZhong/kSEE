// /init: the first process. It mounts the ext4 disk (the image /disk.img,
// through the loop device, on /mnt), then makes the syscalls the page asks
// for, one at a time: it waits reading its terminal (the console), and a
// byte there says a call is asked for (see the page's Machine.call). Plain
// C on nolibc (linux/tools/include/nolibc), the kernel's small C library.
#include <fcntl.h>
#include <stdio.h>
#include <sys/ioctl.h>
#include <sys/mount.h>
#include <unistd.h>
#include <linux/loop.h>
#include <asm/termbits.h>

// Ready for calls (the page's boot is over); the call asked for (the
// runtime fills it in; buffer arguments point into data), and its result.
__attribute__((import_module("ksee"), import_name("ready")))
void ksee_ready(void);
__attribute__((import_module("ksee"), import_name("call")))
int ksee_call(void *req);
__attribute__((import_module("ksee"), import_name("done")))
void ksee_done(long long ret, void *req);

static struct {
	int nr;
	int args[6];
	int pad;
	char data[4096];
} req;

static void disk(void)
{
	struct loop_config config = { 0 };
	int loop;

	config.fd = open("/disk.img", O_RDWR);
	loop = open("/dev/loop0", O_RDWR);
	if (config.fd < 0 || loop < 0 || ioctl(loop, LOOP_CONFIGURE, &config) ||
	    mount("/dev/loop0", "/mnt", "ext4", 0, NULL))
		printf("init: no ext4 disk on /mnt\n");
}

int main(void)
{
	struct termios t;
	char c;

	mount("devtmpfs", "/dev", "devtmpfs", 0, NULL);
	disk();
	// (Raw: each byte is read as it comes, and not echoed.)
	ioctl(0, TCGETS, &t);
	t.c_lflag &= ~(ICANON | ECHO);
	ioctl(0, TCSETS, &t);
	printf("init: ready for the page's calls\n");
	ksee_ready();
	for (;;) {
		if (read(0, &c, 1) != 1 || ksee_call(&req) < 0)
			continue;
		ksee_done(__nolibc_syscall6(req.nr, req.args[0], req.args[1], req.args[2],
					    req.args[3], req.args[4], req.args[5]), &req);
	}
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
