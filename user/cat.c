// cat: its input to its output, until the end of the input. Plain C on
// nolibc, as /init is; the pipe example's child execs it.
#include <unistd.h>

int main(void)
{
	char buf[256];
	long n;

	while ((n = read(0, buf, sizeof(buf))) > 0)
		write(1, buf, n);
	return 0;
}
