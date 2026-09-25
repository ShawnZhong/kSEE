// example: ext4: what makes a directory?
// question: mkdir() on ext4 makes a new inode and a block of directory entries. Where do they come from?
// stop ext4_mkdir: The directory operation: everything it changes goes into one journal transaction.
// stop __ext4_new_inode: A free inode: find one in the inode bitmap of a block group, and mark it used.
// stop ext4_init_new_dir: The new directory's first block, with its own two entries: "." and "..".
// stop add_dirent_to_buf: The name "notes": into free space in a block of the parent directory, next to the other names.
#include <sys/stat.h>

int main(void)
{
	mkdir("/mnt/notes", 0755); // stops here
	return 0;
}
