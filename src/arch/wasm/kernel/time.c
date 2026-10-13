// SPDX-License-Identifier: GPL-2.0-only
/*
 * Time: the clocksource reads the host's clock (virtual: it moves on as
 * the kernel reads it, and the host skips it ahead while the CPU idles);
 * the one-shot clockevent publishes its deadline in wasm_machine,
 * which the host turns into WASM_IRQ_TIMER once it passes.
 */
#include <linux/clockchips.h>
#include <linux/clocksource.h>
#include <linux/init.h>
#include <linux/sched/clock.h>
#include <linux/time.h>
#include <asm/wasm.h>

static u64 wasm_clocksource_read(struct clocksource *cs)
{
	return wasm_time_ns();
}

static struct clocksource wasm_clocksource = {
	.name	= "wasm",
	.rating	= 300,
	.read	= wasm_clocksource_read,
	.mask	= CLOCKSOURCE_MASK(64),
	.flags	= CLOCK_SOURCE_IS_CONTINUOUS,
};

static int wasm_set_next_event(unsigned long delta,
			       struct clock_event_device *ce)
{
	WRITE_ONCE(wasm_machine.timer_deadline, wasm_time_ns() + delta);
	return 0;
}

static int wasm_set_state_shutdown(struct clock_event_device *ce)
{
	WRITE_ONCE(wasm_machine.timer_deadline, U64_MAX);
	return 0;
}

static struct clock_event_device wasm_clockevent = {
	.name			= "wasm",
	.features		= CLOCK_EVT_FEAT_ONESHOT,
	.rating			= 300,
	.set_next_event		= wasm_set_next_event,
	/* (No event, stopped or shut down: no deadline for the host.) */
	.set_state_shutdown	= wasm_set_state_shutdown,
	.set_state_oneshot_stopped = wasm_set_state_shutdown,
};

void wasm_timer_interrupt(void)
{
	wasm_clockevent.event_handler(&wasm_clockevent);
}

unsigned long long sched_clock(void)
{
	return wasm_time_ns();
}

void __init time_init(void)
{
	clocksource_register_hz(&wasm_clocksource, NSEC_PER_SEC);
	wasm_clockevent.cpumask = cpu_possible_mask;
	clockevents_config_and_register(&wasm_clockevent, NSEC_PER_SEC,
					1000, ULONG_MAX);
}
