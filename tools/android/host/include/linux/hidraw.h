// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
#pragma once
#include <stdint.h>

#define HIDIOCGRAWINFO 0x4803UL

struct hidraw_devinfo {
    uint32_t bustype;
    int16_t vendor;
    int16_t product;
};
