// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
#pragma once
#include <stdint.h>

#define USBDEVFS_URB_TYPE_INTERRUPT 1
#define USBDEVFS_URB_TYPE_BULK 3
#define USBDEVFS_CONTROL 0x5500UL
#define USBDEVFS_SUBMITURB 0x5501UL
#define USBDEVFS_DISCARDURB 0x5502UL
#define USBDEVFS_REAPURB 0x5503UL
#define USBDEVFS_REAPURBNDELAY 0x5504UL
#define USBDEVFS_CLAIMINTERFACE 0x5505UL
#define USBDEVFS_RELEASEINTERFACE 0x5506UL
#define USBDEVFS_DISCONNECT_CLAIM 0x5507UL

struct usbdevfs_ctrltransfer {
    uint8_t bRequestType;
    uint8_t bRequest;
    uint16_t wValue;
    uint16_t wIndex;
    uint16_t wLength;
    uint32_t timeout;
    void *data;
};

struct usbdevfs_urb {
    unsigned char type;
    unsigned char endpoint;
    int status;
    unsigned int flags;
    void *buffer;
    int buffer_length;
    int actual_length;
    int start_frame;
    int number_of_packets;
    int error_count;
    unsigned int signr;
    void *usercontext;
};

struct usbdevfs_disconnect_claim {
    unsigned int interface;
    unsigned int flags;
    char driver[256];
};
