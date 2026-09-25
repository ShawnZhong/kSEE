// example: ext4: what happens when a file is deleted?
// question: unlink() takes the name away; the inode and its blocks go when nothing uses them. How does ext4 free them?
// stop ext4_unlink: unlink(): the directory operation.
// stop ext4_delete_entry: Remove the name from the directory's block (its entry's space joins the one before).
// stop ext4_orphan_add: No names left, but the file is open: on the orphan list, so that a crash cannot leak it.
// stop ext4_evict_inode: close(): the last use is gone: the inode is evicted...
// stop ext4_free_blocks: ... its data blocks go back to the block bitmap...
// stop ext4_free_inode: ... and the inode to the inode bitmap.
#include <fcntl.h>
#include <unistd.h>

int main(void)
{
	int fd = open("/mnt/gone.txt", O_CREAT | O_WRONLY, 0644);

	write(fd, "soon gone\n", 10);
	fsync(fd);
	unlink("/mnt/gone.txt"); // stops here
	close(fd); // stops here
	return 0;
}
