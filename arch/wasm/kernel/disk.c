// SPDX-License-Identifier: GPL-2.0-only
/*
 * The disk, /dev/wda: a block device the host keeps (its blocks, the
 * build's ext4 image), driven as a disk controller is. The driver hands it
 * each request's segments (disk_submit) and the request (disk_queue), and
 * returns: the task that wants the blocks waits for them. The host moves
 * them when the CPU next idles, and raises WASM_IRQ_DISK; the interrupt
 * ends the requests it has done (disk_done), which wakes their waiters.
 */
#include <linux/blk-mq.h>
#include <linux/blkdev.h>
#include <linux/init.h>
#include <asm/wasm.h>

static struct blk_mq_tag_set wasm_disk_set;

static blk_status_t wasm_disk_queue_rq(struct blk_mq_hw_ctx *hctx,
				       const struct blk_mq_queue_data *bd)
{
	struct request *rq = bd->rq;
	sector_t sector = blk_rq_pos(rq);
	bool write = op_is_write(req_op(rq));
	struct req_iterator it;
	struct bio_vec bv;

	blk_mq_start_request(rq);
	rq_for_each_segment(bv, rq, it) {
		wasm_disk_submit(sector, bvec_virt(&bv), bv.bv_len, write);
		sector += bv.bv_len >> SECTOR_SHIFT;
	}
	wasm_disk_queue(rq);
	return BLK_STS_OK;
}

/* WASM_IRQ_DISK: the requests the host has done, ended (irq.c). */
void wasm_disk_interrupt(void)
{
	struct request *rq;

	while ((rq = wasm_disk_done()))
		blk_mq_end_request(rq, BLK_STS_OK);
}

static const struct blk_mq_ops wasm_disk_ops = {
	.queue_rq = wasm_disk_queue_rq,
};

static const struct block_device_operations wasm_disk_fops = {
	.owner = THIS_MODULE,
};

static int __init wasm_disk_init(void)
{
	struct gendisk *disk;
	int err;

	if (!wasm_disk_sectors())
		return 0;
	wasm_disk_set.ops = &wasm_disk_ops;
	wasm_disk_set.nr_hw_queues = 1;
	wasm_disk_set.queue_depth = 16;
	wasm_disk_set.numa_node = NUMA_NO_NODE;
	err = blk_mq_alloc_tag_set(&wasm_disk_set);
	if (err)
		return err;
	disk = blk_mq_alloc_disk(&wasm_disk_set, NULL, NULL);
	if (IS_ERR(disk)) {
		blk_mq_free_tag_set(&wasm_disk_set);
		return PTR_ERR(disk);
	}
	disk->flags |= GENHD_FL_NO_PART;
	disk->fops = &wasm_disk_fops;
	strscpy(disk->disk_name, "wda");
	set_capacity(disk, wasm_disk_sectors());
	err = add_disk(disk);
	if (err) {
		put_disk(disk);
		blk_mq_free_tag_set(&wasm_disk_set);
	}
	return err;
}
device_initcall(wasm_disk_init);
