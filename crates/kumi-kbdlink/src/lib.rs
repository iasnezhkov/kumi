// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
#![cfg_attr(not(test), no_std)]

use heapless::Vec;
pub use kumi_icd::StreamKind;
use kumi_keymap::{Input, KeyboardReport, ROLLOVER};

pub const SYNC: u8 = 0x4B;
pub const GROUP_SET: u8 = 0x01;
pub const GROUP_GET: u8 = 0x80;
pub const MAX_FRAME: usize = 64;
pub const MAX_LENGTH: u8 = (MAX_FRAME - 4) as u8;
pub const REPORT_CONSUMER: u8 = 3;
pub const REPORT_KEYBOARD: u8 = 4;
pub const STOCK_VERSION: &[u8; 5] = b"0.0.1";

pub mod command {
    pub const MEMORY: u8 = 0x00;
    pub const MEDIA_0: u8 = 0x01;
    pub const OUTPUT_MUTE: u8 = 0x02;
    pub const MEDIA_1: u8 = 0x03;
    pub const MEDIA_2: u8 = 0x04;
    pub const MEDIA_3: u8 = 0x05;
    pub const MEDIA_4: u8 = 0x06;
    pub const LEGACY_VOLUME: u8 = 0x07;
    pub const CAPTURE_TRIM_0: u8 = 0x08;
    pub const CAPTURE_TRIM_1: u8 = 0x09;
    pub const MUTE_ALL: u8 = 0x0A;
    pub const MODE_MAP: u8 = 0x0B;
    pub const MODE_C: u8 = 0x0C;
    pub const MODE_D: u8 = 0x0D;
    pub const MODE_E: u8 = 0x0E;
    pub const EQ_PROFILE: u8 = 0x0F;
    pub const EQ_SETTINGS: u8 = 0x10;
    pub const VERSION: u8 = 0x11;
    pub const SAMPLE_RATE: u8 = 0x12;
    pub const STREAM_STATUS: u8 = 0x13;
    pub const HID_RAW: u8 = 0x17;
    pub const USB_CONFIG: u8 = 0x1A;
    pub const USB_IDENTITY: u8 = 0x1B;
    pub const USB_SERIAL: u8 = 0x1C;
    pub const USB_PRODUCT: u8 = 0x1D;
    pub const USB_MANUFACTURER: u8 = 0x1E;
    pub const HID_RELAY: u8 = 0x64;
    pub const RESET_PLAIN: u8 = 0xFE;
    pub const RESET_SERVICE: u8 = 0xFF;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame<'a> {
    pub group: u8,
    pub command: u8,
    pub body: &'a [u8],
    pub trailer: [u8; 2],
}

impl<'a> Frame<'a> {
    pub fn data_sum(&self) -> u16 {
        sum16(self.body)
    }

    pub fn checksum(&self) -> Checksum {
        let sum = self.data_sum();
        if self.trailer == sum.to_le_bytes() {
            Checksum::LittleEndian
        } else if self.trailer == sum.to_be_bytes() {
            Checksum::BigEndian
        } else {
            Checksum::Mismatch
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Checksum {
    LittleEndian,
    BigEndian,
    Mismatch,
}

pub fn sum16(bytes: &[u8]) -> u16 {
    bytes.iter().fold(0u16, |acc, b| acc.wrapping_add(u16::from(*b)))
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    pub frames: u32,
    pub skipped_bytes: u32,
    pub bad_lengths: u32,
    pub checksum_mismatches: u32,
    pub unknown_commands: u32,
}

pub struct Receiver {
    buffer: [u8; MAX_FRAME],
    used: usize,
    pub stats: Stats,
}

impl Default for Receiver {
    fn default() -> Self {
        Self::new()
    }
}

impl Receiver {
    pub const fn new() -> Self {
        Self {
            buffer: [0; MAX_FRAME],
            used: 0,
            stats: Stats { frames: 0, skipped_bytes: 0, bad_lengths: 0, checksum_mismatches: 0, unknown_commands: 0 },
        }
    }

    pub fn reset(&mut self) {
        self.used = 0;
    }

    fn resync_tail(&mut self, len: usize) {
        for start in 1..len {
            if self.buffer[start] != SYNC {
                continue;
            }
            if start + 1 < len && self.buffer[start + 1] != GROUP_SET && self.buffer[start + 1] != GROUP_GET {
                continue;
            }
            self.buffer.copy_within(start..len, 0);
            self.used = len - start;
            return;
        }
        self.used = 0;
    }

    pub fn push(&mut self, byte: u8) -> Option<Frame<'_>> {
        if self.used == 0 && byte != SYNC {
            self.stats.skipped_bytes = self.stats.skipped_bytes.wrapping_add(1);
            return None;
        }
        if self.used == 1 && byte != GROUP_SET && byte != GROUP_GET {
            self.stats.skipped_bytes = self.stats.skipped_bytes.wrapping_add(2);
            self.used = usize::from(byte == SYNC);
            return None;
        }
        self.buffer[self.used] = byte;
        self.used += 1;
        if self.used == 4 && !(2..=MAX_LENGTH).contains(&byte) {
            self.stats.bad_lengths = self.stats.bad_lengths.wrapping_add(1);
            self.resync_tail(4);
            return None;
        }
        if self.used >= 4 && self.used == usize::from(self.buffer[3]) + 4 {
            self.used = 0;
            self.stats.frames = self.stats.frames.wrapping_add(1);
            let length = usize::from(self.buffer[3]);
            let frame = Frame {
                group: self.buffer[1],
                command: self.buffer[2],
                body: &self.buffer[4..2 + length],
                trailer: [self.buffer[2 + length], self.buffer[3 + length]],
            };
            if frame.checksum() == Checksum::Mismatch {
                self.stats.checksum_mismatches = self.stats.checksum_mismatches.wrapping_add(1);
            }
            return Some(frame);
        }
        None
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HidReport {
    Keyboard(KeyboardReport),
    Consumer(u8),
    Other { id: u8 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Message<'a> {
    Hid(HidReport),
    RawHid(&'a [u8]),
    Media(u8),
    OutputMute(bool),
    LegacyVolume(u8),
    CaptureTrim(i16),
    MuteAll(bool),
    Mode { field: u8, value: u8 },
    EqProfile(u8),
    EqSettings(&'a [u8]),
    UsbConfig(u8),
    UsbIdentity { vendor: u16, product: u16 },
    UsbString { which: u8, bytes: &'a [u8] },
    MemoryWrite { address: u32, value: u32 },
    Reset { service: bool },
    Query(u8),
    Unknown { group: u8, command: u8 },
}

pub fn parse_hid(bytes: &[u8]) -> Option<HidReport> {
    let (&id, rest) = bytes.split_first()?;
    match id {
        REPORT_KEYBOARD if rest.len() >= 2 + ROLLOVER => {
            let mut keys = [0u8; ROLLOVER];
            keys.copy_from_slice(&rest[2..2 + ROLLOVER]);
            Some(HidReport::Keyboard(KeyboardReport { modifiers: rest[0], keys }))
        }
        REPORT_CONSUMER if !rest.is_empty() => Some(HidReport::Consumer(rest[0])),
        REPORT_KEYBOARD | REPORT_CONSUMER => None,
        id => Some(HidReport::Other { id }),
    }
}

fn s16(bytes: &[u8]) -> Option<i16> {
    Some(i16::from_le_bytes([*bytes.first()?, *bytes.get(1)?]))
}

fn u16le(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes([*bytes.get(at)?, *bytes.get(at + 1)?]))
}

fn u32le(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes([*bytes.get(at)?, *bytes.get(at + 1)?, *bytes.get(at + 2)?, *bytes.get(at + 3)?]))
}

pub fn decode<'a>(frame: &Frame<'a>) -> Message<'a> {
    use command::*;
    let b = frame.body;
    let unknown = Message::Unknown { group: frame.group, command: frame.command };
    if frame.group == GROUP_GET {
        return Message::Query(frame.command);
    }
    let first = b.first().copied();
    match frame.command {
        HID_RELAY => parse_hid(b).map_or(unknown, Message::Hid),
        HID_RAW => Message::RawHid(b),
        MEDIA_0 => Message::Media(0),
        MEDIA_1 => Message::Media(1),
        MEDIA_2 => Message::Media(2),
        MEDIA_3 => Message::Media(3),
        MEDIA_4 => Message::Media(4),
        OUTPUT_MUTE => first.map_or(unknown, |v| Message::OutputMute(v != 0)),
        LEGACY_VOLUME => b.get(1).map_or(unknown, |v| Message::LegacyVolume(v.wrapping_neg())),
        CAPTURE_TRIM_0 | CAPTURE_TRIM_1 => s16(b).map_or(unknown, Message::CaptureTrim),
        MUTE_ALL => first.map_or(unknown, |v| Message::MuteAll(v != 0)),
        MODE_C | MODE_D | MODE_E => first.map_or(unknown, |value| Message::Mode { field: frame.command, value }),
        EQ_PROFILE => first.map_or(unknown, Message::EqProfile),
        EQ_SETTINGS => Message::EqSettings(b),
        USB_CONFIG => first.map_or(unknown, Message::UsbConfig),
        USB_IDENTITY => match (u16le(b, 0), u16le(b, 2)) {
            (Some(vendor), Some(product)) => Message::UsbIdentity { vendor, product },
            _ => unknown,
        },
        USB_SERIAL | USB_PRODUCT | USB_MANUFACTURER => Message::UsbString { which: frame.command, bytes: b },
        MEMORY => match (u32le(b, 0), u32le(b, 4)) {
            (Some(address), Some(value)) => Message::MemoryWrite { address, value },
            _ => unknown,
        },
        RESET_PLAIN => Message::Reset { service: false },
        RESET_SERVICE => Message::Reset { service: true },
        _ => unknown,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CaseState {
    pub sample_rate_hz: u32,
    pub stream: Option<StreamKind>,
    pub pcm_bits: u8,
    pub legacy_volume: u8,
    pub capture_trim: i16,
    pub eq_profile: u8,
    pub mode_c: u8,
    pub mode_d: u8,
    pub mode_e: u8,
    pub usb_mode: u8,
}

impl CaseState {
    fn stream_byte(&self) -> u8 {
        match self.stream {
            None | Some(StreamKind::Pcm) => 0,
            Some(StreamKind::Dop) => 1,
            Some(StreamKind::NativeDsd) => 2,
        }
    }

    fn width_byte(&self) -> u8 {
        let code: u8 = match self.pcm_bits {
            16 => 2,
            24 => 3,
            32 => 4,
            _ => 0,
        };
        (code << 4) | 2
    }

    fn mode_map(&self) -> u8 {
        match self.usb_mode {
            0 => 0,
            1 | 2 => 2,
            3 => 1,
            5 => 3,
            _ => 0xFF,
        }
    }
}

pub type Reply = Vec<u8, MAX_FRAME>;

pub fn reply(request: &Frame<'_>, state: &CaseState) -> Option<Reply> {
    use command::*;
    if request.group != GROUP_GET {
        return None;
    }
    let length = request.body.len() + 2;
    let mut frame: Reply = Vec::new();
    frame.extend_from_slice(&[SYNC, GROUP_GET, request.command, length as u8]).ok()?;
    frame.extend_from_slice(request.body).ok()?;
    let body = &mut frame[4..];
    let mut put = |at: usize, bytes: &[u8]| {
        for (i, byte) in bytes.iter().enumerate() {
            if let Some(slot) = body.get_mut(at + i) {
                *slot = *byte;
            }
        }
    };
    match request.command {
        VERSION => put(0, STOCK_VERSION),
        SAMPLE_RATE => put(0, &state.sample_rate_hz.to_le_bytes()),
        STREAM_STATUS => put(0, &[state.stream_byte(), state.width_byte()]),
        LEGACY_VOLUME => put(0, &[0, state.legacy_volume.wrapping_neg()]),
        CAPTURE_TRIM_0 | CAPTURE_TRIM_1 => put(0, &state.capture_trim.to_le_bytes()),
        MODE_MAP => put(0, &[state.mode_map()]),
        MODE_C => put(0, &[state.mode_c]),
        MODE_D => put(0, &[state.mode_d]),
        MODE_E => put(0, &[state.mode_e]),
        EQ_PROFILE => put(0, &[state.eq_profile]),
        _ => return None,
    }
    let sum = sum16(&frame[4..4 + length - 2]);
    frame.extend_from_slice(&sum.to_le_bytes()).ok()?;
    Some(frame)
}

pub fn apply_hid(input: &mut Input, report: &HidReport) -> bool {
    match report {
        HidReport::Keyboard(k) => {
            input.keyboard = *k;
            true
        }
        HidReport::Consumer(bits) => {
            input.consumer_bits = *bits;
            true
        }
        HidReport::Other { .. } => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(rx: &mut Receiver, bytes: &[u8]) -> std::vec::Vec<(u8, u8, std::vec::Vec<u8>, [u8; 2])> {
        let mut out = std::vec::Vec::new();
        for byte in bytes {
            if let Some(f) = rx.push(*byte) {
                out.push((f.group, f.command, f.body.to_vec(), f.trailer));
            }
        }
        out
    }

    const CAPTURED: [u8; 15] = [0x4B, 0x01, 0x64, 0x0B, 0x04, 0, 0, 0, 0, 0, 0, 0, 0, 0x00, 0x04];

    #[test]
    fn the_frame_captured_from_the_live_case_decodes_as_an_empty_keyboard_report() {
        let mut rx = Receiver::new();
        let frames = feed(&mut rx, &CAPTURED);
        assert_eq!(frames.len(), 1);
        let (group, command, body, trailer) = &frames[0];
        let frame = Frame { group: *group, command: *command, body, trailer: *trailer };
        assert_eq!(frame.checksum(), Checksum::BigEndian);
        assert_eq!(decode(&frame), Message::Hid(HidReport::Keyboard(KeyboardReport::default())));
        assert_eq!(rx.stats.checksum_mismatches, 0);
    }

    #[test]
    fn receiver_resynchronises_on_noise_and_rejects_bad_lengths() {
        let mut rx = Receiver::new();
        let mut stream = vec![0x00, 0x4B, 0x33, 0x4B, 0x80, 0x11, 0x00, 0x99];
        stream.extend_from_slice(&CAPTURED);
        stream.extend_from_slice(&[0x4B, 0x01, 0x64, 0x01]);
        stream.extend_from_slice(&CAPTURED);
        let frames = feed(&mut rx, &stream);
        assert_eq!(frames.len(), 2);
        assert!(rx.stats.skipped_bytes >= 3);
        assert_eq!(rx.stats.bad_lengths, 2);

        let mut rx2 = Receiver::new();
        let mut overlapped = vec![0x4B, 0x01];
        overlapped.extend_from_slice(&CAPTURED);
        overlapped.extend_from_slice(&[0x4B, 0x01, 0x64]);
        overlapped.extend_from_slice(&CAPTURED);
        let recovered = feed(&mut rx2, &overlapped);
        assert_eq!(recovered.len(), 2);
        assert_eq!(rx2.stats.bad_lengths, 2);
    }

    #[test]
    fn keyboard_and_consumer_reports_feed_the_keymap_input() {
        let mut input = Input::default();
        let report = parse_hid(&[4, 0x02, 0, 0x04, 0x05, 0, 0, 0, 0]).unwrap();
        assert!(apply_hid(&mut input, &report));
        assert_eq!(input.keyboard.modifiers, 0x02);
        assert_eq!(input.keyboard.keys, [0x04, 0x05, 0, 0, 0, 0]);
        assert!(apply_hid(&mut input, &parse_hid(&[3, 0b101]).unwrap()));
        assert_eq!(input.consumer_bits, 0b101);
        assert_eq!(parse_hid(&[4, 0, 0]), None);
        assert_eq!(parse_hid(&[9, 1, 2]), Some(HidReport::Other { id: 9 }));
    }

    #[test]
    fn controller_commands_decode() {
        let frame = |command: u8, body: &'static [u8]| Frame { group: GROUP_SET, command, body, trailer: [0, 0] };
        assert_eq!(decode(&frame(command::MEDIA_2, &[])), Message::Media(2));
        assert_eq!(decode(&frame(command::OUTPUT_MUTE, &[1])), Message::OutputMute(true));
        assert_eq!(decode(&frame(command::LEGACY_VOLUME, &[0, 0xC4])), Message::LegacyVolume(60));
        assert_eq!(decode(&frame(command::CAPTURE_TRIM_1, &[0x80, 0x07])), Message::CaptureTrim(1920));
        assert_eq!(decode(&frame(command::EQ_PROFILE, &[3])), Message::EqProfile(3));
        assert_eq!(decode(&frame(command::MODE_D, &[2])), Message::Mode { field: command::MODE_D, value: 2 });
        assert_eq!(
            decode(&frame(command::USB_IDENTITY, &[0x61, 0x06, 0x81, 0x08])),
            Message::UsbIdentity { vendor: 0x0661, product: 0x0881 }
        );
        assert_eq!(decode(&frame(command::RESET_SERVICE, &[])), Message::Reset { service: true });
        assert_eq!(
            decode(&frame(command::MEMORY, &[0, 0x28, 0x11, 0, 1, 0, 0, 0])),
            Message::MemoryWrite { address: 0x112800, value: 1 }
        );
        assert_eq!(decode(&frame(0x42, &[])), Message::Unknown { group: GROUP_SET, command: 0x42 });
        assert_eq!(decode(&Frame { group: GROUP_GET, command: 0x12, body: &[0; 4], trailer: [0, 0] }), Message::Query(0x12));
    }

    fn state() -> CaseState {
        CaseState {
            sample_rate_hz: 48_000,
            stream: Some(StreamKind::Pcm),
            pcm_bits: 24,
            legacy_volume: 60,
            capture_trim: 1920,
            eq_profile: 0,
            mode_c: 0,
            mode_d: 2,
            mode_e: 0,
            usb_mode: 1,
        }
    }

    #[test]
    fn replies_match_the_stock_sender_byte_for_byte() {
        let request = Frame { group: GROUP_GET, command: command::SAMPLE_RATE, body: &[0; 4], trailer: [0, 0] };
        let answer = reply(&request, &state()).unwrap();
        assert_eq!(answer.as_slice(), &[0x4B, 0x80, 0x12, 0x06, 0x80, 0xBB, 0x00, 0x00, 0x3B, 0x01]);
        let request = Frame { group: GROUP_GET, command: command::VERSION, body: &[0; 5], trailer: [0, 0] };
        let reply2 = reply(&request, &state()).unwrap();
        assert_eq!(&reply2[4..9], b"0.0.1");
        assert_eq!(&reply2[9..], &sum16(b"0.0.1").to_le_bytes());
        let request = Frame { group: GROUP_GET, command: command::STREAM_STATUS, body: &[0; 2], trailer: [0, 0] };
        assert_eq!(&reply(&request, &state()).unwrap()[4..6], &[0x00, 0x32]);
        let request = Frame { group: GROUP_GET, command: command::MODE_MAP, body: &[0], trailer: [0, 0] };
        assert_eq!(reply(&request, &state()).unwrap()[4], 2);
        let short = Frame { group: GROUP_GET, command: command::SAMPLE_RATE, body: &[0; 2], trailer: [0, 0] };
        assert_eq!(&reply(&short, &state()).unwrap()[4..6], &[0x80, 0xBB]);
        let unknown = Frame { group: GROUP_GET, command: 0x55, body: &[0; 2], trailer: [0, 0] };
        assert_eq!(reply(&unknown, &state()), None);
        let set = Frame { group: GROUP_SET, command: command::SAMPLE_RATE, body: &[0; 4], trailer: [0, 0] };
        assert_eq!(reply(&set, &state()), None);
    }

    #[test]
    fn replies_round_trip_through_the_receiver_with_a_little_endian_sum() {
        let request = Frame { group: GROUP_GET, command: command::CAPTURE_TRIM_0, body: &[0; 2], trailer: [0, 0] };
        let bytes = reply(&request, &state()).unwrap();
        let mut rx = Receiver::new();
        let frames = feed(&mut rx, &bytes);
        let (group, command, body, trailer) = &frames[0];
        let frame = Frame { group: *group, command: *command, body, trailer: *trailer };
        assert_eq!(frame.checksum(), Checksum::LittleEndian);
        assert_eq!(body.as_slice(), &1920i16.to_le_bytes());
    }
}
