// /init: the first process, and the page's runner. It mounts the ext4
// disk (the image /disk.img, through the loop device, on /mnt), then makes
// the syscalls the page asks for, one at a time: it waits for SIGUSR1
// (blocked, in sigtimedwait), which says a call is asked for (the host's
// doorbell: see the page's Machine.call). A clone child of it (a thread,
// or vfork's child) starts at __wasm_clone_start and is a runner too; so
// is /init exec'd again. Plain C on nolibc (linux/tools/include/nolibc),
// the kernel's small C library.
#include <fcntl.h>
#include <signal.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/mount.h>
#include <unistd.h>
#include <linux/loop.h>

// Ready for calls; the call asked for (the runtime fills it in; buffer
// arguments point into data), and its result.
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

static void say(const char *s)
{
	write(1, s, strlen(s));
}

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
		say("init: no ext4 disk on /mnt\n");
	// (The loop device holds the file; the page's fds start at 3.)
	close(config.fd);
	close(loop);
}

// The doorbell's signal.
static const unsigned long bell = 1UL << (SIGUSR1 - 1);

// Make the page's calls, one at a time, as each is rung.
static void __attribute__((noreturn)) serve(void)
{
	struct request req;

	ksee_ready();
	for (;;) {
		if (__nolibc_syscall4(__NR_rt_sigtimedwait_time64, &bell, NULL, NULL, 8) != SIGUSR1 ||
		    ksee_call(&req) < 0)
			continue;
		ksee_done(__nolibc_syscall6(req.nr, req.args[0], req.args[1], req.args[2],
					    req.args[3], req.args[4], req.args[5]), &req);
	}
}

int main(void)
{
	// (Its threads and children inherit the mask, and a program exec'd.)
	__nolibc_syscall4(__NR_rt_sigprocmask, SIG_BLOCK, &bell, NULL, 8);
	if (getpid() == 1) {
		mount("devtmpfs", "/dev", "devtmpfs", 0, NULL);
		disk();
		say("init: ready for the page's calls\n");
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
