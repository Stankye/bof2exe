#include "beacon.h"
void go(char *args, int length) {
    datap parser;
    int count;
    char *name;
    BeaconDataParse(&parser, args, length);
    count = BeaconDataInt(&parser);
    name = BeaconDataExtract(&parser, NULL);
    if (!name) {
        BeaconPrintf(CALLBACK_ERROR, "expected int and str arguments\n");
        return;
    }
    BeaconPrintf(CALLBACK_OUTPUT, "hello %s: count=%d\n", name, count);
}
