// A syscall, as arch/wasm makes one: a call to the kernel's syscall
// export (the programs import it as linux.syscall), with the number and
// six 64-bit arguments; the result, or a negative errno. The numbers and
// constants are the kernel's own (its UAPI headers).
#include <asm/unistd.h>

long sys(long nr, long long a1, long long a2, long long a3, long long a4, long long a5, long long a6)
	__attribute__((import_module("linux"), import_name("syscall")));

// sys(__NR_write, 1, "hi", 2): the arguments not given, 0. ((sys): the
// import, not this again.)
#define SYS_ARGS(a, b, c, d, e, f, ...) \
	(long)(a), (long)(b), (long)(c), (long)(d), (long)(e), (long)(f)
#define sys(nr, ...) (sys)((nr), SYS_ARGS(__VA_ARGS__, 0, 0, 0, 0, 0, 0))
