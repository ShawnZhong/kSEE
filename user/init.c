// /init: a syscall shell. Each command makes one or a few system calls, so
// that they can be watched in kSEE; no libc, just the syscall import.
//
//   getpid | getppid | gettid | uname
//   write FD TEXT...      read FD [N]        open PATH [r|w|rw|creat]
//   close FD              ls [PATH]          cat PATH
//   mkdir PATH            rm PATH            sleep MS
//   sys NR [ARGS...]      poweroff           help

#define NR_mkdirat	34
#define NR_unlinkat	35
#define NR_openat	56
#define NR_close	57
#define NR_getdents64	61
#define NR_read		63
#define NR_write	64
#define NR_reboot	142
#define NR_uname	160
#define NR_getpid	172
#define NR_getppid	173
#define NR_gettid	178
#define NR_clock_nanosleep	407	/* the 64-bit time one */

#define AT_FDCWD	-100
#define AT_REMOVEDIR	0x200
#define O_WRONLY	01
#define O_RDWR		02
#define O_CREAT		0100
#define O_TRUNC		01000
#define O_DIRECTORY	0200000

__attribute__((import_module("linux"), import_name("syscall")))
long __syscall(long nr, long long a0, long long a1, long long a2,
	       long long a3, long long a4, long long a5);

static long sys(long nr, long a0, long a1, long a2, long a3)
{
	return __syscall(nr, a0, a1, a2, a3, 0, 0);
}

// ---- Output ----------------------------------------------------------------

static unsigned long len(const char *s)
{
	unsigned long n = 0;

	while (s[n])
		n++;
	return n;
}

static void out(const char *s)
{
	sys(NR_write, 1, (long)s, len(s), 0);
}

static void num(long v)
{
	char buf[24], *p = buf + sizeof(buf) - 1;
	int neg = v < 0;
	unsigned long u = neg ? -(unsigned long)v : v;

	*p = 0;
	do
		*--p = '0' + u % 10;
	while (u /= 10);
	if (neg)
		*--p = '-';
	out(p);
}

static const char *const errnames[] = {
	[1] = "EPERM", [2] = "ENOENT", [9] = "EBADF", [11] = "EAGAIN",
	[12] = "ENOMEM", [13] = "EACCES", [14] = "EFAULT", [17] = "EEXIST",
	[20] = "ENOTDIR", [21] = "EISDIR", [22] = "EINVAL", [25] = "ENOTTY",
	[36] = "ENAMETOOLONG", [38] = "ENOSYS", [39] = "ENOTEMPTY",
};

// "= 3", or "= -2 (ENOENT)".
static void result(long r)
{
	out("= ");
	num(r);
	if (r < 0 && -r < (long)(sizeof(errnames) / sizeof(errnames[0])) && errnames[-r]) {
		out(" (");
		out(errnames[-r]);
		out(")");
	}
	out("\n");
}

// ---- Input -----------------------------------------------------------------

static char line[512];
static char *argv[16];
static int argc;

static int same(const char *a, const char *b)
{
	while (*a && *a == *b)
		a++, b++;
	return *a == *b;
}

static long number(const char *s)
{
	long v = 0, base = 10, sign = 1;

	if (*s == '-')
		sign = -1, s++;
	if (s[0] == '0' && s[1] == 'x')
		base = 16, s += 2;
	for (; *s; s++) {
		int d = *s >= 'a' ? *s - 'a' + 10 : *s >= 'A' ? *s - 'A' + 10 : *s - '0';

		if (d < 0 || d >= base)
			break;
		v = v * base + d;
	}
	return sign * v;
}

static long arg(int i)
{
	return i < argc ? number(argv[i]) : 0;
}

// Read a line from stdin and split it into words; 0 at end of input.
static int prompt(void)
{
	long n;
	char *p;

	out("ksee$ ");
	n = sys(NR_read, 0, (long)line, sizeof(line) - 1, 0);
	if (n <= 0)
		return 0;
	line[n] = 0;
	argc = 0;
	for (p = line; *p && argc < 16;) {
		while (*p == ' ' || *p == '\t' || *p == '\n')
			*p++ = 0;
		if (!*p)
			break;
		argv[argc++] = p;
		while (*p && *p != ' ' && *p != '\t' && *p != '\n')
			p++;
	}
	return 1;
}

// ---- Commands --------------------------------------------------------------

static char buf[4096];

static void cmd_write(void)
{
	char *p = buf;
	int i;

	for (i = 2; i < argc; i++) {
		unsigned long n = len(argv[i]);

		if (p + n + 2 > buf + sizeof(buf))
			break;
		for (unsigned long j = 0; j < n; j++)
			*p++ = argv[i][j];
		*p++ = i + 1 < argc ? ' ' : '\n';
	}
	result(sys(NR_write, arg(1), (long)buf, p - buf, 0));
}

static void cmd_read(void)
{
	long want = argc > 2 ? arg(2) : 256;
	long n = sys(NR_read, arg(1), (long)buf, want < (long)sizeof(buf) ? want : (long)sizeof(buf), 0);

	if (n > 0)
		sys(NR_write, 1, (long)buf, n, 0);
	if (n > 0 && buf[n - 1] != '\n')
		out("\n");
	result(n);
}

static void cmd_open(void)
{
	long flags = 0;
	const char *mode = argc > 2 ? argv[2] : "r";

	if (same(mode, "w"))
		flags = O_WRONLY | O_CREAT | O_TRUNC;
	else if (same(mode, "rw"))
		flags = O_RDWR;
	else if (same(mode, "creat"))
		flags = O_RDWR | O_CREAT;
	else if (!same(mode, "r"))
		flags = number(mode);
	result(sys(NR_openat, AT_FDCWD, (long)argv[1], flags, 0644));
}

static void cmd_ls(void)
{
	const char *path = argc > 1 ? argv[1] : "/";
	long fd = sys(NR_openat, AT_FDCWD, (long)path, O_DIRECTORY, 0), n;

	if (fd < 0) {
		result(fd);
		return;
	}
	while ((n = sys(NR_getdents64, fd, (long)buf, sizeof(buf), 0)) > 0) {
		// struct linux_dirent64 { u64 ino; s64 off; u16 reclen; u8 type; char name[]; }
		for (long off = 0; off < n;) {
			unsigned short reclen = *(unsigned short *)(buf + off + 16);

			out(buf + off + 19);
			out(buf[off + 18] == 4 ? "/  " : "  ");
			off += reclen;
		}
	}
	out("\n");
	sys(NR_close, fd, 0, 0, 0);
	result(n);
}

static void cmd_cat(void)
{
	long fd = sys(NR_openat, AT_FDCWD, (long)argv[1], 0, 0), n;

	if (fd < 0) {
		result(fd);
		return;
	}
	while ((n = sys(NR_read, fd, (long)buf, sizeof(buf), 0)) > 0)
		sys(NR_write, 1, (long)buf, n, 0);
	sys(NR_close, fd, 0, 0, 0);
	result(n);
}

static void cmd_sleep(void)
{
	// struct __kernel_timespec, on CLOCK_MONOTONIC (1)
	long long ts[2] = { arg(1) / 1000, arg(1) % 1000 * 1000000 };

	result(sys(NR_clock_nanosleep, 1, 0, (long)ts, 0));
}

static void cmd_uname(void)
{
	long r = sys(NR_uname, (long)buf, 0, 0, 0);

	// struct new_utsname: six 65-byte strings.
	for (int i = 0; r == 0 && i < 5; i++) {
		out(buf + 65 * i);
		out(i < 4 ? " " : "\n");
	}
	result(r);
}

static void help(void)
{
	out("getpid | getppid | gettid | uname\n"
	    "write FD TEXT...      read FD [N]        open PATH [r|w|rw|creat]\n"
	    "close FD              ls [PATH]          cat PATH\n"
	    "mkdir PATH            rm PATH            sleep MS\n"
	    "sys NR [ARGS...]      poweroff           help\n");
}

static void run(void)
{
	const char *c = argv[0];

	if (same(c, "help"))
		help();
	else if (same(c, "getpid"))
		result(sys(NR_getpid, 0, 0, 0, 0));
	else if (same(c, "getppid"))
		result(sys(NR_getppid, 0, 0, 0, 0));
	else if (same(c, "gettid"))
		result(sys(NR_gettid, 0, 0, 0, 0));
	else if (same(c, "uname"))
		cmd_uname();
	else if (same(c, "write") && argc > 1)
		cmd_write();
	else if (same(c, "read") && argc > 1)
		cmd_read();
	else if (same(c, "open") && argc > 1)
		cmd_open();
	else if (same(c, "close") && argc > 1)
		result(sys(NR_close, arg(1), 0, 0, 0));
	else if (same(c, "ls"))
		cmd_ls();
	else if (same(c, "cat") && argc > 1)
		cmd_cat();
	else if (same(c, "mkdir") && argc > 1)
		result(sys(NR_mkdirat, AT_FDCWD, (long)argv[1], 0755, 0));
	else if (same(c, "rm") && argc > 1) {
		long r = sys(NR_unlinkat, AT_FDCWD, (long)argv[1], 0, 0);

		if (r == -21) // EISDIR: a directory
			r = sys(NR_unlinkat, AT_FDCWD, (long)argv[1], AT_REMOVEDIR, 0);
		result(r);
	} else if (same(c, "sleep") && argc > 1)
		cmd_sleep();
	else if (same(c, "sys") && argc > 1)
		result(__syscall(arg(1), arg(2), arg(3), arg(4), arg(5), arg(6), arg(7)));
	else if (same(c, "poweroff"))
		// LINUX_REBOOT_CMD_POWER_OFF (as init, exiting would panic)
		result(sys(NR_reboot, 0xfee1dead, 672274793, 0x4321fedc, 0));
	else {
		out(c);
		out(": unknown command (try help)\n");
	}
}

void _start(void)
{
	out("kSEE syscall shell: each command makes system calls to watch (try help)\n");
	while (prompt())
		if (argc)
			run();
	sys(NR_reboot, 0xfee1dead, 672274793, 0x4321fedc, 0);
}
