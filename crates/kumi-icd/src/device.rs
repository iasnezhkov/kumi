// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
use heapless::String;
use postcard_schema::Schema;
use serde::{Deserialize, Serialize};

use crate::flags::flags;

#[derive(Serialize, Deserialize, Schema, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    pub major: u8,
    pub minor: u8,
    pub patch: u8,
}

flags! {
    Capabilities {
        AUDIO_OUT = 0, "audio_out";
        AUDIO_IN = 1, "audio_in";
        DSD = 2, "dsd";
        PARAMETRIC_EQ = 3, "parametric_eq";
        KEYMAP = 4, "keymap";
        LAYERS = 5, "layers";
        BATTERY = 6, "battery";
        PERSISTENT_SETTINGS = 7, "persistent_settings";
        FIRMWARE_UPDATE = 8, "firmware_update";
        DEVELOPER = 9, "developer";
        LAB = 10, "lab";
    }
}

#[derive(Serialize, Deserialize, Schema, Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    pub protocol: u16,
    pub firmware: Version,
    pub build: String<24>,
    pub chip_id: u32,
    pub flash_jedec: [u8; 3],
    pub serial: Option<[u8; 8]>,
    pub running_from_ram: bool,
    pub capabilities: Capabilities,
}
