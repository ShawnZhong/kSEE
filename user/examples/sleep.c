// example: Where does the time go when a program sleeps?
// question: nanosleep() for 30 ms takes 30 ms, but the kernel works for a tiny part of it. What happens in between?
// stop hrtimer_nanosleep: Set a high-resolution timer for 30 ms from now.
// stop schedule: Nothing to do until then: give the CPU away. The idle task (swapper) runs, and waits for an interrupt.
// stop hrtimer_wakeup, any task: 30 ms later, the timer's interrupt: wake the sleeper up (it is runnable again).
#include <stdio.h>
#include <time.h>

int main(void)
{
	struct timespec ts = { .tv_sec = 0, .tv_nsec = 30 * 1000 * 1000 };

	printf("sleeping for 30 ms\n");
	nanosleep(&ts, NULL);	// stops here
	printf("awake\n");
	return 0;
}
