// SPDX-License-Identifier: GPL-2.0-only
/*
 * The console: printk's console from setup_arch on, and /dev/console (a
 * plain character device: no tty layer, CONFIG_TTY=n), /init's stdin,
 * stdout and stderr. A write hands the program's bytes to the host as they
 * are, synchronously. A read waits for the host's input for its task (the
 * calls the page asks a program to make: user/prog.c reads them), which
 * the console's interrupt says may have come.
 */
#include <linux/console.h>
#include <linux/fs.h>
#include <linux/init.h>
#include <linux/major.h>
#include <linux/uaccess.h>
#include <linux/wait.h>
#include <asm/wasm.h>

static DECLARE_WAIT_QUEUE_HEAD(console_wait);

static void wasm_console_write_con(struct console *con, const char *s,
				   unsigned int n)
{
	wasm_console_write(s, n);
}

struct console wasm_console = {
	.name	= "wasm",
	.write	= wasm_console_write_con,
	.flags	= CON_PRINTBUFFER,
};

/*
 * A program's memory is kernel memory (NOMMU), which the host reads
 * directly: no copy, only the check that the bytes are the program's.
 */
static ssize_t console_dev_write(struct file *file, const char __user *buf,
				 size_t count, loff_t *ppos)
{
	if (!access_ok(buf, count))
		return -EFAULT;
	wasm_console_write((const char __force *)buf, count);
	return count;
}

/* As a write, the host writes the program's memory directly. */
static ssize_t console_dev_read(struct file *file, char __user *buf,
				size_t count, loff_t *ppos)
{
	unsigned int n;

	if (!access_ok(buf, count))
		return -EFAULT;
	return wait_event_interruptible(console_wait,
		(n = wasm_console_read((char __force *)buf, count))) ?: n;
}

/* Input may have come: its readers look. */
void wasm_console_interrupt(void)
{
	wake_up_interruptible(&console_wait);
}

static const struct file_operations console_dev_fops = {
	.write	= console_dev_write,
	.read	= console_dev_read,
	.llseek	= noop_llseek,
};

/*
 * /dev/console (5, 1), the tty layer's number: the node is the initramfs's
 * (no sysfs, no devtmpfs: nothing would see a device made here).
 */
static int __init console_dev_init(void)
{
	return register_chrdev(TTYAUX_MAJOR, "console", &console_dev_fops);
}
device_initcall(console_dev_init);
