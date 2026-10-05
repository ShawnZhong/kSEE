// SPDX-License-Identifier: GPL-2.0-only
/*
 * The console: output only, to the host, synchronously (programs get the
 * host's input otherwise: see the doorbell, irq.c). It is printk's console
 * from setup_arch on, and /dev/console (a plain character device: no tty
 * layer, CONFIG_TTY=n) is /init's stdin, stdout and stderr: a write hands
 * the program's bytes to the host as they are, a read is at its end.
 */
#include <linux/console.h>
#include <linux/device.h>
#include <linux/fs.h>
#include <linux/init.h>
#include <linux/major.h>
#include <linux/uaccess.h>
#include <asm/wasm.h>

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

static ssize_t console_dev_read(struct file *file, char __user *buf,
				size_t count, loff_t *ppos)
{
	return 0;
}

static const struct file_operations console_dev_fops = {
	.write	= console_dev_write,
	.read	= console_dev_read,
	.llseek	= noop_llseek,
};

/* /dev/console (5, 1), as the tty layer would have made it. */
static int __init console_dev_init(void)
{
	struct class *cls;
	int ret;

	ret = register_chrdev(TTYAUX_MAJOR, "console", &console_dev_fops);
	if (ret)
		return ret;
	cls = class_create("console");
	if (IS_ERR(cls))
		return PTR_ERR(cls);
	return PTR_ERR_OR_ZERO(device_create(cls, NULL, MKDEV(TTYAUX_MAJOR, 1),
					     NULL, "console"));
}
device_initcall(console_dev_init);
