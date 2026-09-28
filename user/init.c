// /init: the first process. It mounts the ext4 disk (the image
// /disk.img, through the loop device, on /mnt), then runs the page's
// program, /bin/prog, as a child (a vfork: no MMU, no fork), and again
// when it exits. Plain C on nolibc (linux/tools/include/nolibc), the
// kernel's small C library.
#include <fcntl.h>
#include <signal.h>
#include <sys/ioctl.h>
#include <sys/mount.h>
#include <unistd.h>
#include <linux/loop.h>
#include <linux/sched.h>
#include <linux/wait.h>

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

// The vfork child: it runs the page's program (arch/wasm: a clone child
// starts here, on its parent's stack, below it).
__attribute__((export_name("__wasm_clone_start")))
void clone_start(void)
{
	char *argv[] = { "prog", NULL }, *envp[] = { NULL };

	execve("/bin/prog", argv, envp);
	write(1, "init: no /bin/prog\n", 19);
	__nolibc_syscall1(__NR_exit, 127);
}

int main(void)
{
	mount("devtmpfs", "/dev", "devtmpfs", 0, NULL);
	disk();
	for (;;) {
		long pid = __nolibc_syscall5(__NR_clone, CLONE_VM | CLONE_VFORK | SIGCHLD, 0, 0, 0, 0);
		char info[128];

		if (pid < 0)
			return 1;
		// (It exited: again, a fresh one.)
		__nolibc_syscall5(__NR_waitid, P_PID, pid, info, WEXITED, 0);
	}
}
