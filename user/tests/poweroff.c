// Power the machine off (tests/web.js's last check).
#include <sys/reboot.h>

int main(void)
{
	return reboot(RB_POWER_OFF);
}
