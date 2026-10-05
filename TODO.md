# TODO

What is decided but not done yet. Done items leave this file (git log has
them).

## The Disk tab

- **Blocks named** by what ext4 keeps there (superblock, group
  descriptors, bitmaps, inode table, journal, a file's data), from the
  image's superblock read in ksee-core: "write 1 KiB at sector 36" says
  "the journal" or "inode 12's data".
