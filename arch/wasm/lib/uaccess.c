// SPDX-License-Identifier: GPL-2.0-only
/*
 * A user string is kernel memory (NOMMU: see asm/uaccess.h), so it is
 * measured and copied as one: not word by word, as lib/strncpy_from_user.c
 * reads memory it may not be able to touch. It may run up to where memory
 * ends (high_memory), not past it.
 */
#include <linux/minmax.h>
#include <linux/string.h>
#include <linux/uaccess.h>

/* The bytes at `src` memory has, up to `count`; 0 if none. */
static long user_room(const char __user *src, long count)
{
	if (count <= 0 || !access_ok(src, 1))
		return 0;
	return min_t(unsigned long, count, (unsigned long)high_memory - (unsigned long)src);
}

long strncpy_from_user(char *dst, const char __user *src, long count)
{
	long room = user_room(src, count), n;

	if (count <= 0)
		return 0;
	n = strnlen((const char __force *)src, room);
	if (!room || (n == room && room < count))
		return -EFAULT;
	memcpy(dst, (const void __force *)src, n < count ? n + 1 : count);
	return n;
}

long strnlen_user(const char __user *src, long count)
{
	long room = user_room(src, count), n;

	if (!room)
		return 0;
	n = strnlen((const char __force *)src, room);
	if (n == room && room < count)
		return 0;
	return n < count ? n + 1 : count + 1;
}
