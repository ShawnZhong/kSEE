// tour: What happens in read()?
// question: A program asks for 64 bytes of a file. Which parts of the kernel does that go through?
// stop ksys_read: The syscall itself: fdget_pos turns the fd into the open file (a struct file), and the position to read at.
// stop vfs_read: The VFS, the same for every filesystem: is the file open for reading, is the range sane? Then it calls the filesystem's own read_iter.
// stop filemap_read: The page cache: a file's data lives in memory pages. Find the page for the position, and copy from it to the program.
#include <fcntl.h>
#include <stdio.h>
#include <sys/stat.h>
#include <unistd.h>

int main(void)
{
	char buf[64];
	int fd, n;

	/* A file to read (the kernel keeps its files in memory: tmpfs). */
	mkdir("/tmp", 0755);
	fd = open("/tmp/note", O_WRONLY | O_CREAT | O_TRUNC, 0644);
	write(fd, "hello from the page cache\n", 26);
	close(fd);

	fd = open("/tmp/note", O_RDONLY);
	n = read(fd, buf, sizeof(buf));	// stops here
	close(fd);

	printf("read %d bytes\n", n);
	return 0;
}
