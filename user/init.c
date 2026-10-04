// /init: the first process. It mounts the ext4 disk (/dev/wda, the host's
// blocks: the kernel's disk.c) on /mnt, then runs the page's
// program, /bin/prog, as a child (a vfork: no MMU, no fork), and again
// when it exits. Plain C, with no C library: its syscalls as syscall.h
// makes them.
#include <linux/fcntl.h>
#include <linux/sched.h>
#include <linux/wait.h>
#include <asm/signal.h>
#include "syscall.h"

#define say(s) sys(__NR_write, 1, s, sizeof(s) - 1)

// The disk, /dev/wda, on /mnt. (No background work after the mount: the
// image is initialized in full, and ext4lazyinit need not prefetch its
// block bitmaps.)
static void disk(void)
{
	if (sys(__NR_mount, "/dev/wda", "/mnt", "ext4", 0, "no_prefetch_block_bitmaps"))
		say("init: no ext4 disk on /mnt\n");
}

// The vfork child: it runs the page's program (arch/wasm: a clone child
// starts here, on its parent's stack, below it).
__attribute__((export_name("__wasm_clone_start")))
void clone_start(void)
{
	char *argv[] = { "prog", 0 }, *envp[] = { 0 };

	sys(__NR_execve, "/bin/prog", argv, envp);
	say("init: no /bin/prog\n");
	sys(__NR_exit, 127);
}

// Where it starts (the host enters it: no arguments).
__attribute__((export_name("_start")))
void start(void)
{
	sys(__NR_mount, "devtmpfs", "/dev", "devtmpfs", 0, 0);
	disk();
	for (;;) {
		long pid = sys(__NR_clone, CLONE_VM | CLONE_VFORK | SIGCHLD, 0, 0, 0, 0);
		char info[128];

		if (pid < 0)
			sys(__NR_exit, 1);
		// (It exited: again, a fresh one.)
		sys(__NR_waitid, P_PID, pid, info, WEXITED, 0);
	}
}
