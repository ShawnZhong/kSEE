// tour: Why does open() fail?
// question: Opening a file that is not there fails with ENOENT. Where in the kernel is that decided?
// stop path_openat: Opening walks the path, one name at a time, from the root (/) down.
// stop lookup_fast: Look each name up in the dentry cache: the kernel's memory of the names it has seen. "tmp" is there.
// stop lookup_open: The last name, "missing", is not in the cache: ask the directory. (With O_CREAT, this is where the file would be made.)
// stop simple_lookup: tmpfs keeps all its names in the cache, so the name does not exist: remember that, as a negative dentry (a name without an inode). Opening a name without an inode fails: -ENOENT.
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

int main(void)
{
	int fd;

	mkdir("/tmp", 0755);
	fd = open("/tmp/missing", O_RDONLY);	// stops here
	if (fd < 0)
		printf("open: %s (errno %d)\n", strerror(errno), errno);

	/* With O_CREAT, the same walk creates the file instead. */
	fd = open("/tmp/missing", O_RDONLY | O_CREAT, 0644);
	printf("with O_CREAT: fd %d\n", fd);
	close(fd);
	unlink("/tmp/missing");
	return 0;
}
