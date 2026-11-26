// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
use crate::flags::flags;

flags! {
    EventMask {
        KEYS = 0, "keys";
        LOG = 1, "log";
        AUDIO = 2, "audio";
        UART = 3, "uart";
    }
}
