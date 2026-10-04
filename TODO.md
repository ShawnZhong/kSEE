# TODO

What is decided but not done yet. Done items leave this file (git log has
them).

## A Disk tab (designed, not built)

The host is the disk (kernel/disk.c), so it sees every transfer; the page
could show a call's disk traffic beside Stack, Tasks, Objects, Console.

- **Recorded:** for each transfer the host makes, its request's tag, sector,
  length, direction, the record ids where it was handed over (queue_rq) and
  done (the interrupt), and a write's bytes before and after (the disk is
  the host's: no trace records needed). ksee-host keeps the log; ksee-core
  gets a view of it: the requests in a span.
- **Shown:** each request of the call selected, in order: "read 1 KiB at
  block 34 · tag 3 · by prog(18)", from queue_rq to its interrupt (a click:
  the cursor there); a write, its changed bytes as a hex diff. The request
  in flight at the cursor marked, as the Stack follows the cursor.
- **Later:** blocks named by what ext4 keeps there (superblock, group
  descriptors, bitmaps, inode table, journal, a file's data), from the
  image's superblock read in ksee-core.
