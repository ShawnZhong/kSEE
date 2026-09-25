// example: Directories on ext4
// question: A directory is a file of names, each pointing to an inode. What happens when one is made, listed, renamed in, and removed?
// stop ext4_mkdir: mkdir(): a new directory, all in one journal transaction.
// stop __ext4_new_inode: A free inode for it, from a block group's inode bitmap.
// stop ext4_init_new_dir: Its first block, with its own two entries: "." (itself) and ".." (its parent).
// stop add_dirent_to_buf: The name "dir": into a block of /mnt, the parent.
// stop ext4_create: A file in it: another inode, and another name, now in dir's block.
// stop ext4_readdir: getdents64(): read the directory's blocks, entry by entry.
// stop filldir64: Each entry, copied out to the program: ".", "..", "a".
// stop ext4_rename: rename() in one directory: the entry's name changes, "a" to "b"; the inode stays as it is.
// stop ext4_empty_dir: rmdir() of a directory with a name in it: is it empty? No ("b"): the call fails, ENOTEMPTY.
// stop ext4_free_inode: unlink(): "b" was the file's only name, and nobody has it open: its inode is freed.
// stop ext4_free_blocks: rmdir() again, now empty: the directory's block goes back to the block bitmap.
#include <fcntl.h>
#include <sys/stat.h>
#include <unistd.h>

int main(void)
{
	char buf[256];
	int fd;

	mkdir("/mnt/dir", 0755); // stops here
	close(open("/mnt/dir/a", O_CREAT | O_WRONLY, 0644)); // stops here
	fd = open("/mnt/dir", O_RDONLY | O_DIRECTORY);
	getdents64(fd, (void *)buf, sizeof(buf)); // stops here
	close(fd);
	rename("/mnt/dir/a", "/mnt/dir/b"); // stops here
	rmdir("/mnt/dir"); // stops here
	unlink("/mnt/dir/b"); // stops here
	rmdir("/mnt/dir"); // stops here
	return 0;
}
