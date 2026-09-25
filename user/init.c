// /init: the first process. The kernel runs the page's programs itself (see
// linux/arch/wasm/drivers/ksee_channel.c), so init only has to be there.
// No libc, just the syscall import.

#define NR_write		64
#define NR_clock_nanosleep	407
#define CLOCK_MONOTONIC		1

__attribute__((import_module("linux"), import_name("syscall")))
long __syscall(long nr, long long a0, long long a1, long long a2,
	       long long a3, long long a4, long long a5);

void _start(void)
{
	static const char ready[] = "init: ready for the page's programs\n";
	long long hour[2] = { 3600, 0 };

	__syscall(NR_write, 1, (long)ready, sizeof(ready) - 1, 0, 0, 0);
	for (;;)
		__syscall(NR_clock_nanosleep, CLOCK_MONOTONIC, 0, (long)hour, 0, 0, 0);
}
