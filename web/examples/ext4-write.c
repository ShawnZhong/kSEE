// example: ext4: what happens when a program writes a file?
// question: write() returns at once, but the data is not on the disk yet. What does fsync() do to put it there?
// stop ext4_buffered_write_iter: write(): ext4 copies the bytes into the page cache, and marks the page dirty.
// stop ext4_da_write_begin: Delayed allocation: no disk block is chosen yet; one is only reserved.
// stop ext4_sync_file: fsync(): now the data must reach the disk.
// stop ext4_do_writepages: Write the dirty pages back: now choose their blocks.
// stop ext4_mb_new_blocks: The multiblock allocator picks free blocks, from the block group's bitmap.
// stop jbd2_journal_commit_transaction, any task: The journal (jbd2, its own thread) commits the metadata changes: the new block's bitmap, the inode.
// stop submit_bio, any task: The block layer: an I/O request to the disk, here /dev/loop0, whose blocks are a file in memory.
#include <fcntl.h>
#include <unistd.h>

int main(void)
{
	int fd = open("/mnt/hello.txt", O_CREAT | O_WRONLY | O_TRUNC, 0644);

	write(fd, "hello, ext4\n", 12); // stops here
	fsync(fd); // stops here
	close(fd);
	return 0;
}
