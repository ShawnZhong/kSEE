// /init: the syscall agent. It makes the syscalls the host asks for, so
// that they run in a real process (this one); no libc, just the syscall
// import.
//
// It reads requests from /dev/ksee (the kernel's mailbox for the host, see
// linux/arch/wasm/drivers/ksee_agent.c; the read sleeps until there is one)
// and writes back replies. A request (little endian):
//   u32 nr, nbufs; i64 args[6];
//   { u32 arg, dir, len, off } bufs[nbufs];   dir 0: input, 1: output
//   data                                       (buffer b at offset b.off)
// Argument `arg` of each buffer becomes the buffer's address here. The
// reply is i64 ret, then the output buffers' contents, in order.

#define NR_mount	40
#define NR_openat	56
#define NR_read		63
#define NR_write	64
#define AT_FDCWD	-100
#define O_RDWR		02

__attribute__((import_module("linux"), import_name("syscall")))
long __syscall(long nr, long long a0, long long a1, long long a2,
	       long long a3, long long a4, long long a5);

struct buf {
	unsigned int arg, dir, len, off;
};

struct request {
	unsigned int nr, nbufs;
	long long args[6];
	struct buf bufs[];
};

static unsigned char req[64 << 10] __attribute__((aligned(8)));
static unsigned char rep[64 << 10] __attribute__((aligned(8)));

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

	if (fd < 0) {
		say("init: no /dev/ksee\n");
		return;
	}
	say("init: kSEE syscall agent ready\n");
	for (;;) {
		struct request *r = (struct request *)req;
		long n = __syscall(NR_read, fd, (long)req, sizeof(req), 0, 0, 0);
		long long ret;
		unsigned long out = 8;

		if (n < (long)sizeof(*r))
			continue;
		for (unsigned int i = 0; i < r->nbufs && i < 6; i++)
			if (r->bufs[i].arg < 6)
				r->args[r->bufs[i].arg] = (long)(req + r->bufs[i].off);
		ret = __syscall(r->nr, r->args[0], r->args[1], r->args[2],
				r->args[3], r->args[4], r->args[5]);
		*(long long *)rep = ret;
		for (unsigned int i = 0; i < r->nbufs && i < 6; i++) {
			struct buf *b = &r->bufs[i];

			if (b->dir != 1 || out + b->len > sizeof(rep))
				continue;
			for (unsigned int j = 0; j < b->len; j++)
				rep[out + j] = req[b->off + j];
			out += b->len;
		}
		__syscall(NR_write, fd, (long)rep, out, 0, 0, 0);
	}
}
