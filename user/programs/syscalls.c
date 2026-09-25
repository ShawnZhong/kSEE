// A little of everything: files, a directory, writev, a pipe, uname, a sleep.
#include <dirent.h>
#include <fcntl.h>
#include <stdio.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/uio.h>
#include <sys/utsname.h>
#include <time.h>
#include <unistd.h>

int main(void)
{
	char buf[32] = { 0 };
	struct iovec iov[2] = { { "hello ", 6 }, { "from writev\n", 12 } };
	struct timespec ts = { 0, 20 * 1000 * 1000 };
	struct utsname u;
	struct dirent e, *d;
	int fds[2], n = 0;
	DIR *dir;

	printf("pid %d\n", getpid());
	write(1, "hello kSEE\n", 11);
	mkdir("/tmp", 0755);
	dir = opendir("/");
	while (!readdir_r(dir, &e, &d) && d)
		n += !strcmp(d->d_name, "init");
	closedir(dir);
	printf("ls /: init %s\n", n ? "found" : "missing");
	writev(1, iov, 2);
	pipe(fds);
	write(fds[1], "through a pipe", 14);
	read(fds[0], buf, sizeof(buf));
	printf("pipe: %s\n", buf);
	uname(&u);
	printf("uname: %s %s\n", u.sysname, u.machine);
	nanosleep(&ts, NULL);
	printf("slept\n");
	return 0;
}
