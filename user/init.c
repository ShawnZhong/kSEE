// The first user program: no libc, just the syscall import.
#define __NR_write	64
#define __NR_exit	93
#define __NR_reboot	142
#define __NR_getpid	172

__attribute__((import_module("linux"), import_name("syscall")))
long __syscall(long nr, long long a0, long long a1, long long a2,
	       long long a3, long long a4, long long a5);

static long syscall3(long nr, long a0, long a1, long a2)
{
	return __syscall(nr, a0, a1, a2, 0, 0, 0);
}

static void puts1(const char *s)
{
	unsigned long n = 0;

	while (s[n])
		n++;
	syscall3(__NR_write, 1, (long)s, n);
}

static void putnum(long v)
{
	char buf[24], *p = buf + sizeof(buf) - 1;

	*p = 0;
	do
		*--p = '0' + v % 10;
	while (v /= 10);
	puts1(p);
}

void _start(void)
{
	puts1("Hello from wasm userspace! I am pid ");
	putnum(syscall3(__NR_getpid, 0, 0, 0));
	puts1("\n");
	/* LINUX_REBOOT_CMD_POWER_OFF; as init, exiting would panic. */
	__syscall(__NR_reboot, 0xfee1dead, 672274793, 0x4321fedc, 0, 0, 0);
	syscall3(__NR_exit, 1, 0, 0);
}
