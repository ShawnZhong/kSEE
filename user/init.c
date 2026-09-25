// /init: the first process. It mounts the ext4 disk (the image /disk.img,
// through the loop device, on /mnt), says it is ready, and sleeps: the
// kernel runs the page's programs itself (linux/arch/wasm/kernel/run.c).
#include <fcntl.h>
#include <stdio.h>
#include <sys/ioctl.h>
#include <sys/mount.h>
#include <unistd.h>
#include <linux/loop.h>

int main(void)
{
	struct loop_config config = { 0 };
	int loop;

	mount("devtmpfs", "/dev", "devtmpfs", 0, NULL);
	config.fd = open("/disk.img", O_RDWR);
	loop = open("/dev/loop0", O_RDWR);
	if (config.fd < 0 || loop < 0 || ioctl(loop, LOOP_CONFIGURE, &config) ||
	    mount("/dev/loop0", "/mnt", "ext4", 0, NULL))
		printf("init: no ext4 disk on /mnt\n");
	printf("init: ready for the page's programs\n");
	for (;;)
		sleep(3600);
}
