// SPDX-License-Identifier: GPL-2.0-only
#include <linux/init.h>
#include <linux/memblock.h>
#include <linux/mm.h>

void __init arch_zone_limits_init(unsigned long *max_zone_pfns)
{
	max_zone_pfns[ZONE_NORMAL] = max_low_pfn;
}
