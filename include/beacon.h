/* Original declarations for the supported public Beacon C API subset.
 * SPDX-License-Identifier: MIT OR Apache-2.0 */
#ifndef BOF2EXE_BEACON_H
#define BOF2EXE_BEACON_H
#include <stddef.h>
#ifdef __cplusplus
extern "C" {
#endif
#define BOF_IMPORT __declspec(dllimport)
#define CALLBACK_OUTPUT 0
#define CALLBACK_ERROR 13
#define CALLBACK_OUTPUT_OEM 30
#define CALLBACK_OUTPUT_UTF8 32
typedef struct { char *original; char *buffer; int length; int size; } datap;
typedef struct { char *original; char *buffer; int length; int size; } formatp;
BOF_IMPORT void BeaconDataParse(datap *, char *, int);
BOF_IMPORT char *BeaconDataPtr(datap *, int);
BOF_IMPORT int BeaconDataInt(datap *);
BOF_IMPORT short BeaconDataShort(datap *);
BOF_IMPORT int BeaconDataLength(datap *);
BOF_IMPORT char *BeaconDataExtract(datap *, int *);
BOF_IMPORT void BeaconOutput(int, char *, int);
BOF_IMPORT void BeaconPrintf(int, char *, ...);
BOF_IMPORT void BeaconFormatAlloc(formatp *, int);
BOF_IMPORT void BeaconFormatReset(formatp *);
BOF_IMPORT void BeaconFormatFree(formatp *);
BOF_IMPORT void BeaconFormatAppend(formatp *, char *, int);
BOF_IMPORT void BeaconFormatPrintf(formatp *, char *, ...);
BOF_IMPORT char *BeaconFormatToString(formatp *, int *);
BOF_IMPORT void BeaconFormatInt(formatp *, int);
BOF_IMPORT int toWideChar(char *, wchar_t *, int);
#ifdef __cplusplus
}
#endif
#endif
