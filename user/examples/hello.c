// example: What happens when a program prints?
// question: printf ends in write(1, ...): how do the bytes get from the program to the console?
// stop ksys_write: The syscall: the fd (1, the console) turns into the open file, a struct file.
// stop vfs_write: The VFS: is the file open for writing? Then the file's own write: a tty's.
// stop n_tty_write: The tty's line discipline: turn "\n" into "\r\n", and pass the bytes to the driver.
// stop wasm_tty_write: The console driver (arch/wasm/kernel/console.c): hand the bytes to the host, which shows them.
#include <stdio.h>

int main(void)
{
	printf("hello\n"); // stops here
	return 0;
}
