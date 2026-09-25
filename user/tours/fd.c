// tour: The life of a file descriptor
// question: An fd is a number; the open file is a struct file in the kernel. How do they relate, and when is the file freed?
// stop alloc_fd: open(): the lowest free number in the process's fd table.
// stop fd_install: ... then point it at the new struct file.
// stop __fput: The last reference is gone (both fds are closed): the struct file is freed.
#include <fcntl.h>
#include <stdio.h>
#include <sys/stat.h>
#include <unistd.h>

int main(void)
{
	int fd, copy;

	mkdir("/tmp", 0755);
	fd = open("/tmp/fd-demo", O_WRONLY | O_CREAT | O_TRUNC, 0644);	// stops here
	/* Another fd for the same open file: its count goes to 2. */
	copy = dup(fd);
	printf("fd %d and fd %d\n", fd, copy);
	/* The file stays open: the copy still refers to it. */
	close(fd);
	write(copy, "still open\n", 11);
	/* Now it is freed. */
	close(copy);	// stops here
	return 0;
}
