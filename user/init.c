// /init: the first process. It mounts the ext4 disk (the image /disk.img,
// through the loop device, on /mnt), then makes the syscalls the page asks
// for, one at a time: it waits reading its terminal (the console), and a
// byte there says a call is asked for (see the page's Machine.call). (The
// kernel runs the page's programs itself: linux/arch/wasm/kernel/run.c.)
#include <fcntl.h>
#include <stdio.h>
#include <sys/ioctl.h>
#include <sys/mount.h>
#include <unistd.h>
#include <linux/loop.h>
#include <asm/termbits.h>

// The call asked for (the runtime fills it in; buffer arguments point into
// data), and its result.
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
	printf("init: ready for the page's programs\n");
	for (;;) {
		if (read(0, &c, 1) != 1 || ksee_call(&req) < 0)
			continue;
		ksee_done(__nolibc_syscall6(req.nr, req.args[0], req.args[1], req.args[2],
					    req.args[3], req.args[4], req.args[5]), &req);
	}
}
