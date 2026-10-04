// SPDX-License-Identifier: GPL-2.0-only
/*
 * Executables are position-independent wasm modules that import the
 * kernel's memory (NOMMU: a program's pointers are kernel addresses). The
 * host compiles the module; the kernel maps it pages for its data and its
 * stack; the host instantiates it there, and gives its _start as an entry
 * in the kernel's function table; the task calls it when it returns to
 * user mode (see process.c's wasm_enter_user).
 */
#include <linux/binfmts.h>
#include <linux/fs.h>
#include <linux/init.h>
#include <linux/mm.h>
#include <linux/mman.h>
#include <linux/sched.h>
#include <linux/slab.h>
#include <linux/vmalloc.h>
#include <asm/wasm.h>

#define WASM_STACK_SIZE	(64 * 1024)

static int load_wasm_binary(struct linux_binprm *bprm)
{
	loff_t size = i_size_read(file_inode(bprm->file));
	loff_t pos = 0;
	unsigned long base, len, entry, clone_entry = 0;
	long data = 0;
	void *image;
	int ret;

	if (memcmp(bprm->buf, "\0asm\1\0\0\0", 8))
		return -ENOEXEC;
	if (size > INT_MAX)
		return -E2BIG;

	image = kvmalloc(size, GFP_KERNEL);
	if (!image)
		return -ENOMEM;
	ret = kernel_read(bprm->file, image, size, &pos);
	if (ret == size) {
		data = wasm_user_load(image, size);
		ret = data < 0 ? data : 0;
	} else if (ret >= 0) {
		ret = -EIO;
	}
	kvfree(image);
	if (ret)
		return ret;

	ret = begin_new_exec(bprm);
	if (ret)
		return ret;
	setup_new_exec(bprm);

	/* Its data, then its stack (growing down from the end). */
	len = PAGE_ALIGN(data) + WASM_STACK_SIZE;
	base = vm_mmap(NULL, 0, len, PROT_READ | PROT_WRITE,
		       MAP_PRIVATE | MAP_ANONYMOUS, 0);
	if (IS_ERR_VALUE(base))
		return base;
	current->mm->start_data = base;
	current->mm->end_data = base + data;
	current->mm->start_stack = base + len;
	entry = wasm_user_start(base, base + len, &clone_entry);
	if (!entry)
		return -ENOEXEC;

	finalize_exec(bprm);
	current->thread.entry = (void (*)(void))entry;
	current->thread.clone_entry = (void (*)(void))clone_entry;
	return 0;
}

static struct linux_binfmt wasm_format = {
	.load_binary	= load_wasm_binary,
};

static int __init init_wasm_binfmt(void)
{
	register_binfmt(&wasm_format);
	return 0;
}
core_initcall(init_wasm_binfmt);
