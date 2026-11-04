#include "beacon.h"
extern "C" int first();
extern "C" int second();
extern "C" void go(char *args, int length) {
    (void)args; (void)length;
    BeaconPrintf(CALLBACK_OUTPUT, "first=%d second=%d\n", first(), second());
}
