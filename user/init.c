// /init: the first process, and the page's runner. It mounts the ext4
// disk (the image /disk.img, through the loop device, on /mnt), then makes
// the syscalls the page asks for, one at a time: it waits in a futex on a
// word of its own, which the host sets when a call is asked for (and
// rings the task: see the page's Machine.call). A clone child of it (a thread,
// or vfork's child) starts at __wasm_clone_start and is a runner too; so
// is /init exec'd again. Plain C on nolibc (linux/tools/include/nolibc),
// the kernel's small C library.
#include <fcntl.h>
#include <linux/futex.h>
#include <sys/ioctl.h>
#include <sys/mount.h>
#include <unistd.h>
#include <linux/loop.h>

// Ready for calls: the host puts a call in *req (its number and
// arguments; buffer arguments point into data) and sets *rung; the runner
// makes it, and puts the result in req->ret, done.
struct request {
	int nr;
	int args[6];
	int done;
	long long ret;
	char data[4096];
};

__attribute__((import_module("ksee"), import_name("ready")))
void ksee_ready(volatile int *rung, struct request *req);

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
		write(1, "init: no ext4 disk on /mnt\n", 27);
	// (The loop device holds the file; the page's fds start at 3.)
	close(config.fd);
	close(loop);
}

// Make the page's calls, one at a time, as each is rung: the host puts
// the call in `req`, sets `rung` and wakes the task; the wait sees the
// word changed, and returns.
static void __attribute__((noreturn)) serve(void)
{
	struct request req;
	volatile int rung = 0;

	ksee_ready(&rung, &req);
	for (;;) {
		while (!rung)
			__nolibc_syscall4(__NR_futex_time64, &rung, FUTEX_WAIT_PRIVATE, 0, NULL);
		rung = 0;
		req.ret = __nolibc_syscall6(req.nr, req.args[0], req.args[1], req.args[2],
					    req.args[3], req.args[4], req.args[5]);
		req.done = 1;
	}
}

int main(void)
{
	if (getpid() == 1) {
		mount("devtmpfs", "/dev", "devtmpfs", 0, NULL);
		disk();
	}
	serve();
}

// Where a clone child starts (arch/wasm: it cannot return from clone), on
// the stack it was given, or below its parent's (vfork).
__attribute__((export_name("__wasm_clone_start")))
void clone_start(void)
{
	serve();
}
