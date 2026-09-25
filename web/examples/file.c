// example: The life of a file on ext4
// question: A file is created, written, made durable, closed, and deleted. What does ext4 do at each step?
// stop ext4_create: open() with O_CREAT: the name is not there, so ext4 makes the file (all in one journal transaction).
// stop __ext4_new_inode: A free inode: find one in a block group's inode bitmap, and mark it used.
// stop add_dirent_to_buf: The name "note.txt": into free space in a block of the directory, next to the other names.
// stop ext4_buffered_write_iter: write(): the bytes go into the page cache, and the page is marked dirty. Nothing is on the disk yet.
// stop ext4_da_write_begin: Delayed allocation: no disk block is chosen yet; one is only reserved.
// stop ext4_sync_file: fsync(): now the data must reach the disk.
// stop ext4_mb_new_blocks: Write the dirty page back: the multiblock allocator picks it a free block, from the block bitmap.
// stop jbd2_journal_commit_transaction, any task: The journal (jbd2, its own thread) commits the metadata changes: the bitmaps, the inode, the directory block.
// stop submit_bio, any task: The block layer: an I/O request to the disk, /dev/loop0, whose blocks are a file in memory.
// stop __fput: close(): the last reference to the open file goes. The file stays: it still has a name.
// stop ext4_delete_entry: unlink(): the name leaves the directory block (its space joins the entry before).
// stop ext4_evict_inode: No names and no users left: the inode is evicted...
// stop ext4_free_blocks: ... its data block goes back to the block bitmap...
// stop ext4_free_inode: ... and the inode to the inode bitmap.
#include <fcntl.h>
#include <unistd.h>

int main(void)
{
	int fd = open("/mnt/note.txt", O_CREAT | O_WRONLY, 0644); // stops here

	write(fd, "hello, ext4\n", 12); // stops here
	fsync(fd); // stops here
	close(fd); // stops here
	unlink("/mnt/note.txt"); // stops here
	return 0;
}
