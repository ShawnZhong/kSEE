// /init: the process the page's syscalls run in. It serves requests from
// the kernel's /dev/ksee channel (see linux/arch/wasm/drivers/ksee_channel.c;
// the read sleeps until there is one): make a syscall, or prepare memory
// for syscalls (buffers and structs the page lays out), in this process.
// No libc, just the syscall import.
//
// A request (little endian): u32 op, a, b, pad; i64 args[6]; data. A reply:
// i64 ret; data.
//   SYSCALL  nr = a, args            ret: the syscall's result
//   ALLOC    size = a, align = b     ret: the address (-ENOMEM)
//   WRITE    addr = a, len = b, data ret: 0 (-EFAULT)
//   READ     addr = a, len = b       ret: b (-EFAULT), then the bytes
//   RESET                            frees everything ALLOC gave
//
// The channel is fd 100 (KSEE_FD), so that the page's own files get the
// usual numbers, and the debugger can tell this traffic from the page's
// syscalls (it does not stop in it; see crates/ksee-runtime/src/debug.rs).

enum { SYSCALL = 1, ALLOC, WRITE, READ, RESET };

#define NR_dup3		24
#define NR_mount	40
#define NR_close	57
#define NR_openat	56
#define NR_read		63
#define NR_write	64
#define AT_FDCWD	-100
#define O_RDWR		02
#define ENOMEM		12
#define EFAULT		14
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

#define MESSAGE	(64 << 10)
static unsigned char req[MESSAGE] __attribute__((aligned(8)));
static unsigned char rep[MESSAGE] __attribute__((aligned(8)));
static unsigned char heap[1 << 20] __attribute__((aligned(16)));
static unsigned long used;

static int in_heap(unsigned long addr, unsigned long len)
{
	unsigned long base = (unsigned long)heap;

	return addr >= base && len <= sizeof(heap) && addr - base <= sizeof(heap) - len;
}

// Serve one request into rep; the reply's length.
static unsigned long serve(struct request *r, long n)
{
	long long *ret = (long long *)rep;
	unsigned long len = 8;

	switch (r->op) {
	case SYSCALL:
		*ret = __syscall(r->a, r->args[0], r->args[1], r->args[2],
				 r->args[3], r->args[4], r->args[5]);
		break;
	case ALLOC: {
		unsigned long align = r->b ? r->b : 8;
		unsigned long at = (used + align - 1) & ~(align - 1);

		if ((align & (align - 1)) || at + r->a > sizeof(heap)) {
			*ret = -ENOMEM;
			break;
		}
		used = at + r->a;
		for (unsigned long i = at; i < used; i++)
			heap[i] = 0;
		*ret = (long)(heap + at);
		break;
	}
	case WRITE:
		if (!in_heap(r->a, r->b) || (long)sizeof(*r) + r->b > n) {
			*ret = -EFAULT;
			break;
		}
		for (unsigned int i = 0; i < r->b; i++)
			((unsigned char *)(long)r->a)[i] = r->data[i];
		*ret = 0;
		break;
	case READ:
		// Anywhere in this process's memory (the page reads results).
		if (r->b > MESSAGE - 8) {
			*ret = -EFAULT;
			break;
		}
		for (unsigned int i = 0; i < r->b; i++)
			rep[8 + i] = ((unsigned char *)(long)r->a)[i];
		*ret = r->b;
		len += r->b;
		break;
	case RESET:
		used = 0;
		*ret = 0;
		break;
	default:
		*ret = -EINVAL;
	}
	return len;
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
	say("init: ready for the page's syscalls\n");
	for (;;) {
		long n = __syscall(NR_read, fd, (long)req, sizeof(req), 0, 0, 0);

		if (n >= (long)sizeof(struct request))
			__syscall(NR_write, fd, (long)rep, serve((struct request *)req, n), 0, 0, 0);
	}
}
