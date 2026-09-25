// /init: starts the page's programs (a tour's, see user/tours/) and waits
// for them, at the requests of the kernel's /dev/ksee channel (see
// linux/arch/wasm/drivers/ksee_channel.c; the read sleeps until there is
// one). No libc, just the syscall import.
//
// A request (little endian): u32 op, a, b, pad; i64 args[6]; data. A reply:
// i64 ret.
//   SPAWN    path = data (NUL-ended) ret: the pid of a new child running
//                                    the program (-errno)
//   WAIT     pid = a                 ret: its exit status, when it exits
//
// The channel is fd 100 (KSEE_FD), so that programs get the usual fds.

enum { SPAWN = 1, WAIT };

#define NR_dup3		24
#define NR_ioctl	29
#define NR_mount	40
#define NR_close	57
#define NR_openat	56
#define NR_read		63
#define NR_write	64
#define NR_waitid	95
#define P_PID		1
#define WEXITED		4
#define KSEE_SPAWN	0x6b01	/* _IO('k', 1) */
#define AT_FDCWD	-100
#define O_RDWR		02
#define EINVAL		22
#define KSEE_FD		100

__attribute__((import_module("linux"), import_name("syscall")))
long __syscall(long nr, long long a0, long long a1, long long a2,
	       long long a3, long long a4, long long a5);

struct request {
	unsigned int op, a, b, pad;
	long long args[6];
	unsigned char data[];
};

#define MESSAGE	(4 << 10)
static unsigned char req[MESSAGE] __attribute__((aligned(8)));
static long long rep;

// Serve one request (`n` bytes): the reply.
static long long serve(struct request *r, long n)
{
	switch (r->op) {
	case SPAWN:
		// (A child of this process: the kernel's /dev/ksee starts it.)
		r->data[n - sizeof(*r) - 1] = 0;
		return __syscall(NR_ioctl, KSEE_FD, KSEE_SPAWN, (long)r->data, 0, 0, 0);
	case WAIT: {
		// siginfo_t: si_signo, si_errno, si_code, then si_pid, si_uid, si_status.
		int info[32] = { 0 };
		long err = __syscall(NR_waitid, P_PID, r->a, (long)info, WEXITED, 0, 0);

		return err < 0 ? err : info[5];
	}
	default:
		return -EINVAL;
	}
}

static void say(const char *s)
{
	unsigned long n = 0;

	while (s[n])
		n++;
	__syscall(NR_write, 1, (long)s, n, 0, 0, 0);
}

void _start(void)
{
	long fd;

	// Device files (the kernel mounts devtmpfs itself only for a real root).
	__syscall(NR_mount, (long)"devtmpfs", (long)"/dev", (long)"devtmpfs", 0, 0, 0);
	fd = __syscall(NR_openat, AT_FDCWD, (long)"/dev/ksee", O_RDWR, 0, 0, 0);
	if (fd < 0 || __syscall(NR_dup3, fd, KSEE_FD, 0, 0, 0, 0) < 0) {
		say("init: no /dev/ksee\n");
		return;
	}
	__syscall(NR_close, fd, 0, 0, 0, 0, 0);
	fd = KSEE_FD;
	say("init: ready for the page's programs\n");
	for (;;) {
		long n = __syscall(NR_read, fd, (long)req, sizeof(req), 0, 0, 0);

		if (n >= (long)sizeof(struct request)) {
			rep = serve((struct request *)req, n);
			__syscall(NR_write, fd, (long)&rep, sizeof(rep), 0, 0, 0);
		}
	}
}
