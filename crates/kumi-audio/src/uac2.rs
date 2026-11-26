// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
use crate::flow::feedback_bytes;
use crate::pcm::{PcmProfile, SampleFormat, UsbSpeed, rates_at};

pub const CLASS_AUDIO: u8 = 0x01;
pub const SUBCLASS_CONTROL: u8 = 0x01;
pub const SUBCLASS_STREAMING: u8 = 0x02;
pub const PROTOCOL_V2: u8 = 0x20;
pub const INTERFACE: u8 = 0x04;
pub const ENDPOINT: u8 = 0x05;
pub const INTERFACE_ASSOCIATION: u8 = 0x0b;
pub const CS_INTERFACE: u8 = 0x24;
pub const CS_ENDPOINT: u8 = 0x25;
pub const CLOCK_SOURCE_ID: u8 = 1;
pub const INPUT_TERMINAL_ID: u8 = 3;
pub const OUTPUT_TERMINAL_ID: u8 = 4;
pub const FEATURE_UNIT_ID: u8 = 10;
pub const CATEGORY_PRO_AUDIO: u8 = 0x0a;
pub const CLOCK_INTERNAL_PROGRAMMABLE: u8 = 0x03;
pub const CLOCK_CONTROLS: u8 = 0x07;
pub const TERMINAL_USB_STREAMING: u16 = 0x0101;
pub const TERMINAL_HEADPHONES: u16 = 0x0302;
pub const FORMAT_PCM: u32 = 1;
pub const FORMAT_RAW_DATA: u32 = 1 << 31;
pub const CHANNELS_FRONT_LEFT_RIGHT: u32 = 3;
pub const ISO_ASYNC_DATA: u8 = 0x05;
pub const ISO_FEEDBACK: u8 = 0x11;
pub const RAW_ALTERNATE: u8 = 4;
pub const REQUEST_CUR: u8 = 1;
pub const REQUEST_RANGE: u8 = 2;
pub const CLASS_INTERFACE_IN: u8 = 0xa1;
pub const CLASS_INTERFACE_OUT: u8 = 0x21;
pub const SAMPLING_FREQUENCY: u8 = 1;
pub const CLOCK_VALID: u8 = 2;
pub const MUTE: u8 = 1;
pub const VOLUME: u8 = 2;
pub const VOLUME_MIN: i16 = -60 * 256;
pub const VOLUME_MAX: i16 = 0;
pub const VOLUME_STEP: i16 = 128;
pub const INITIAL_RATE: u32 = 48_000;
pub const INITIAL_VOLUME: i16 = -20 * 256;
pub const AC_TOTAL: u16 = 9 + 8 + 17 + 12 + 18;
pub const MAX_FUNCTION_BYTES: usize = 8 + 9 + AC_TOTAL as usize + 9 + 4 * 53;
pub const MAX_RANGE_BYTES: usize = 2 + 12 * 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Config {
    pub speed: UsbSpeed,
    pub control_interface: u8,
    pub out_endpoint: u8,
    pub feedback_endpoint: u8,
    pub raw_alternate: bool,
    pub data_interval: u8,
    pub max_rate_hz: u32,
}

impl Config {
    pub const fn streaming_interface(&self) -> u8 {
        self.control_interface + 1
    }

    pub const fn alternates(&self) -> u8 {
        if self.raw_alternate { RAW_ALTERNATE } else { 3 }
    }

    pub const fn packets_per_second(&self) -> u32 {
        self.speed.packets_per_second(self.data_interval)
    }

    pub fn rates(&self) -> impl Iterator<Item = PcmProfile> {
        rates_at(self.speed, self.packets_per_second(), self.max_rate_hz)
    }

    pub fn supports(&self, rate: u32) -> bool {
        self.rates().any(|p| p.hz() == rate)
    }

    pub fn max_packet(&self, alternate: u8) -> Option<u16> {
        let format = if alternate == RAW_ALTERNATE && self.raw_alternate {
            SampleFormat::S32
        } else {
            SampleFormat::from_alternate(alternate)?
        };
        let packets = self.packets_per_second();
        self.rates().map(|p| p.packet_bytes(packets, format)).max()
    }

    pub fn largest_packet(&self) -> u16 {
        (1..=self.alternates()).filter_map(|alternate| self.max_packet(alternate)).max().unwrap_or(0)
    }

    pub const fn feedback_interval(&self) -> u8 {
        match self.speed {
            UsbSpeed::Full => 1,
            UsbSpeed::High => 4,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DescriptorTooLarge;

struct Writer<'a> {
    out: &'a mut [u8],
    at: usize,
}

impl Writer<'_> {
    fn put(&mut self, bytes: &[u8]) -> Result<(), DescriptorTooLarge> {
        let end = self.at + bytes.len();
        self.out.get_mut(self.at..end).ok_or(DescriptorTooLarge)?.copy_from_slice(bytes);
        self.at = end;
        Ok(())
    }
}

const fn le16(value: u16) -> [u8; 2] {
    value.to_le_bytes()
}

const fn le32(value: u32) -> [u8; 4] {
    value.to_le_bytes()
}

struct Alternate {
    number: u8,
    subslot: u8,
    formats: u32,
    channels: u32,
    packet: u16,
}

fn alternate(w: &mut Writer, config: &Config, alt: Alternate) -> Result<(), DescriptorTooLarge> {
    let f = le32(alt.formats);
    let c = le32(alt.channels);
    let p = le16(alt.packet);
    w.put(&[9, INTERFACE, config.streaming_interface(), alt.number, 2, CLASS_AUDIO, SUBCLASS_STREAMING, PROTOCOL_V2, 0])?;
    w.put(&[16, CS_INTERFACE, 0x01, INPUT_TERMINAL_ID, 0, 0x01, f[0], f[1], f[2], f[3], 2, c[0], c[1], c[2], c[3], 0])?;
    w.put(&[6, CS_INTERFACE, 0x02, 0x01, alt.subslot, alt.subslot * 8])?;
    w.put(&[7, ENDPOINT, config.out_endpoint, ISO_ASYNC_DATA, p[0], p[1], config.data_interval])?;
    w.put(&[8, CS_ENDPOINT, 0x01, 0, 0, 2, 2, 0])?;
    let size = le16(feedback_bytes(config.speed));
    w.put(&[7, ENDPOINT, config.feedback_endpoint, ISO_FEEDBACK, size[0], size[1], config.feedback_interval()])
}

pub fn function_descriptor(config: &Config, out: &mut [u8]) -> Result<usize, DescriptorTooLarge> {
    let mut w = Writer { out, at: 0 };
    let total = le16(AC_TOTAL);
    let usb = le16(TERMINAL_USB_STREAMING);
    let phones = le16(TERMINAL_HEADPHONES);
    let channels = le32(CHANNELS_FRONT_LEFT_RIGHT);
    w.put(&[8, INTERFACE_ASSOCIATION, config.control_interface, 2, CLASS_AUDIO, 0, PROTOCOL_V2, 0])?;
    w.put(&[9, INTERFACE, config.control_interface, 0, 0, CLASS_AUDIO, SUBCLASS_CONTROL, PROTOCOL_V2, 0])?;
    w.put(&[9, CS_INTERFACE, 0x01, 0x00, 0x02, CATEGORY_PRO_AUDIO, total[0], total[1], 0])?;
    w.put(&[8, CS_INTERFACE, 0x0a, CLOCK_SOURCE_ID, CLOCK_INTERNAL_PROGRAMMABLE, CLOCK_CONTROLS, 0, 0])?;
    w.put(&[
        17,
        CS_INTERFACE,
        0x02,
        INPUT_TERMINAL_ID,
        usb[0],
        usb[1],
        0,
        CLOCK_SOURCE_ID,
        2,
        channels[0],
        channels[1],
        channels[2],
        channels[3],
        0,
        0,
        0,
        0,
    ])?;
    w.put(&[12, CS_INTERFACE, 0x03, OUTPUT_TERMINAL_ID, phones[0], phones[1], 0, FEATURE_UNIT_ID, CLOCK_SOURCE_ID, 0, 0, 0])?;
    w.put(&[18, CS_INTERFACE, 0x06, FEATURE_UNIT_ID, INPUT_TERMINAL_ID, 0x03, 0, 0, 0, 0x0c, 0, 0, 0, 0x0c, 0, 0, 0, 0])?;
    w.put(&[9, INTERFACE, config.streaming_interface(), 0, 0, CLASS_AUDIO, SUBCLASS_STREAMING, PROTOCOL_V2, 0])?;
    for format in SampleFormat::ALL {
        let number = format.alternate();
        let packet = config.max_packet(number).unwrap_or(0);
        let subslot = format.subslot_bytes();
        alternate(
            &mut w,
            config,
            Alternate { number, subslot, formats: FORMAT_PCM, channels: CHANNELS_FRONT_LEFT_RIGHT, packet },
        )?;
    }
    if config.raw_alternate {
        let packet = config.max_packet(RAW_ALTERNATE).unwrap_or(0);
        alternate(
            &mut w,
            config,
            Alternate { number: RAW_ALTERNATE, subslot: 4, formats: FORMAT_RAW_DATA, channels: 0, packet },
        )?;
    }
    Ok(w.at)
}

pub struct Walk<'a> {
    bytes: &'a [u8],
}

impl<'a> Walk<'a> {
    pub const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes }
    }
}

impl<'a> Iterator for Walk<'a> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<&'a [u8]> {
        let length = usize::from(*self.bytes.first()?);
        if length < 2 || length > self.bytes.len() {
            self.bytes = &[];
            return None;
        }
        let (head, tail) = self.bytes.split_at(length);
        self.bytes = tail;
        Some(head)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Setup {
    pub request_type: u8,
    pub request: u8,
    pub value: u16,
    pub index: u16,
    pub length: u16,
}

impl Setup {
    pub const fn parse(raw: [u8; 8]) -> Self {
        Self {
            request_type: raw[0],
            request: raw[1],
            value: u16::from_le_bytes([raw[2], raw[3]]),
            index: u16::from_le_bytes([raw[4], raw[5]]),
            length: u16::from_le_bytes([raw[6], raw[7]]),
        }
    }

    pub const fn interface(&self) -> u8 {
        self.index as u8
    }

    pub const fn entity(&self) -> u8 {
        (self.index >> 8) as u8
    }

    pub const fn selector(&self) -> u8 {
        (self.value >> 8) as u8
    }

    pub const fn channel(&self) -> u8 {
        self.value as u8
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RequestError {
    NotAudio,
    Stall,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    Rate(u32),
    Mute(bool),
    Volume { channel: u8, value: i16 },
    Alternate(u8),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct State {
    pub rate: u32,
    pub mute: bool,
    pub volume: [i16; 2],
    pub alternate: u8,
}

pub struct Control {
    config: Config,
    state: State,
    clock_valid: bool,
}

const fn valid_volume(value: i16) -> bool {
    value >= VOLUME_MIN && value <= VOLUME_MAX && value % VOLUME_STEP == 0
}

fn reply(payload: &[u8], setup: &Setup, buffer: &mut [u8]) -> Result<usize, RequestError> {
    let length = payload.len().min(usize::from(setup.length));
    buffer.get_mut(..length).ok_or(RequestError::Stall)?.copy_from_slice(&payload[..length]);
    Ok(length)
}

impl Control {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            state: State { rate: INITIAL_RATE, mute: false, volume: [INITIAL_VOLUME; 2], alternate: 0 },
            clock_valid: true,
        }
    }

    pub const fn config(&self) -> &Config {
        &self.config
    }

    pub const fn state(&self) -> State {
        self.state
    }

    pub fn profile(&self) -> Option<PcmProfile> {
        PcmProfile::from_hz(self.state.rate)
    }

    pub fn format(&self) -> Option<SampleFormat> {
        SampleFormat::from_alternate(self.state.alternate)
    }

    pub fn set_clock_valid(&mut self, valid: bool) {
        self.clock_valid = valid;
    }

    pub fn reset(&mut self) {
        self.state.alternate = 0;
    }

    fn own(&self, setup: &Setup, direction: u8) -> Result<(), RequestError> {
        if setup.request_type != direction || setup.interface() != self.config.control_interface {
            Err(RequestError::NotAudio)
        } else {
            Ok(())
        }
    }

    pub fn set_alternate(&mut self, interface: u8, alternate: u8) -> Result<Option<Change>, RequestError> {
        if interface == self.config.control_interface {
            return if alternate == 0 { Ok(None) } else { Err(RequestError::Stall) };
        }
        if interface != self.config.streaming_interface() {
            return Err(RequestError::NotAudio);
        }
        if alternate > self.config.alternates() {
            return Err(RequestError::Stall);
        }
        self.state.alternate = alternate;
        Ok(Some(Change::Alternate(alternate)))
    }

    pub fn control_in(&self, setup: &Setup, buffer: &mut [u8]) -> Result<usize, RequestError> {
        self.own(setup, CLASS_INTERFACE_IN)?;
        match (setup.entity(), setup.request, setup.selector(), setup.channel()) {
            (CLOCK_SOURCE_ID, REQUEST_CUR, SAMPLING_FREQUENCY, 0) => reply(&self.state.rate.to_le_bytes(), setup, buffer),
            (CLOCK_SOURCE_ID, REQUEST_CUR, CLOCK_VALID, 0) => reply(&[u8::from(self.clock_valid)], setup, buffer),
            (CLOCK_SOURCE_ID, REQUEST_RANGE, SAMPLING_FREQUENCY, 0) => {
                let mut range = [0u8; MAX_RANGE_BYTES];
                let mut count = 0u16;
                for (i, profile) in self.config.rates().enumerate() {
                    let hz = profile.hz().to_le_bytes();
                    range[2 + i * 12..6 + i * 12].copy_from_slice(&hz);
                    range[6 + i * 12..10 + i * 12].copy_from_slice(&hz);
                    count += 1;
                }
                range[..2].copy_from_slice(&count.to_le_bytes());
                reply(&range[..2 + usize::from(count) * 12], setup, buffer)
            }
            (FEATURE_UNIT_ID, REQUEST_CUR, MUTE, 0) => reply(&[u8::from(self.state.mute)], setup, buffer),
            (FEATURE_UNIT_ID, REQUEST_CUR, VOLUME, channel @ 1..=2) => {
                reply(&self.state.volume[usize::from(channel - 1)].to_le_bytes(), setup, buffer)
            }
            (FEATURE_UNIT_ID, REQUEST_RANGE, VOLUME, 1..=2) => {
                let mut range = [0u8; 8];
                range[..2].copy_from_slice(&1u16.to_le_bytes());
                range[2..4].copy_from_slice(&VOLUME_MIN.to_le_bytes());
                range[4..6].copy_from_slice(&VOLUME_MAX.to_le_bytes());
                range[6..8].copy_from_slice(&VOLUME_STEP.to_le_bytes());
                reply(&range, setup, buffer)
            }
            _ => Err(RequestError::Stall),
        }
    }

    pub fn control_out(&mut self, setup: &Setup, data: &[u8]) -> Result<Change, RequestError> {
        self.own(setup, CLASS_INTERFACE_OUT)?;
        if usize::from(setup.length) != data.len() || setup.request != REQUEST_CUR {
            return Err(RequestError::Stall);
        }
        match (setup.entity(), setup.selector(), setup.channel(), data) {
            (CLOCK_SOURCE_ID, SAMPLING_FREQUENCY, 0, &[a, b, c, d]) => {
                let rate = u32::from_le_bytes([a, b, c, d]);
                if !self.config.supports(rate) {
                    return Err(RequestError::Stall);
                }
                self.state.rate = rate;
                Ok(Change::Rate(rate))
            }
            (FEATURE_UNIT_ID, MUTE, 0, &[value]) if value <= 1 => {
                self.state.mute = value != 0;
                Ok(Change::Mute(value != 0))
            }
            (FEATURE_UNIT_ID, VOLUME, channel @ 1..=2, &[low, high]) => {
                let value = i16::from_le_bytes([low, high]);
                if !valid_volume(value) {
                    return Err(RequestError::Stall);
                }
                self.state.volume[usize::from(channel - 1)] = value;
                Ok(Change::Volume { channel, value })
            }
            _ => Err(RequestError::Stall),
        }
    }

    pub fn host_volume_centi_db(&self) -> [i16; 2] {
        self.state.volume.map(|value| ((i32::from(value) * 100) / 256) as i16)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::vec::Vec;

    fn config(speed: UsbSpeed, raw: bool) -> Config {
        Config {
            speed,
            control_interface: 2,
            out_endpoint: 0x07,
            feedback_endpoint: 0x83,
            raw_alternate: raw,
            data_interval: 1,
            max_rate_hz: 384_000,
        }
    }

    fn runtime() -> Config {
        Config {
            speed: UsbSpeed::High,
            control_interface: 2,
            out_endpoint: 0x07,
            feedback_endpoint: 0x84,
            raw_alternate: false,
            data_interval: 4,
            max_rate_hz: 96_000,
        }
    }

    fn setup(request_type: u8, request: u8, value: u16, index: u16, length: u16) -> Setup {
        let mut raw = [request_type, request, 0, 0, 0, 0, 0, 0];
        raw[2..4].copy_from_slice(&value.to_le_bytes());
        raw[4..6].copy_from_slice(&index.to_le_bytes());
        raw[6..8].copy_from_slice(&length.to_le_bytes());
        Setup::parse(raw)
    }

    fn descriptors(c: &Config) -> Vec<u8> {
        let mut out = [0u8; MAX_FUNCTION_BYTES];
        let n = function_descriptor(c, &mut out).unwrap();
        out[..n].to_vec()
    }

    #[test]
    fn the_function_walks_cleanly_and_sizes_every_endpoint_for_its_speed() {
        for (speed, raw, total, packet32, feedback) in [
            (UsbSpeed::Full, false, 249, 776, [3, 0, 1]),
            (UsbSpeed::High, false, 249, 392, [4, 0, 4]),
            (UsbSpeed::High, true, 302, 392, [4, 0, 4]),
        ] {
            let c = config(speed, raw);
            let bytes = descriptors(&c);
            assert_eq!(bytes.len(), total);
            let parts: Vec<&[u8]> = Walk::new(&bytes).collect();
            assert_eq!(parts.iter().map(|p| p.len()).sum::<usize>(), total);
            let ac_start = parts.iter().position(|p| p[1] == CS_INTERFACE && p[2] == 1).unwrap();
            let ac: usize = parts[ac_start..ac_start + 5].iter().map(|p| p.len()).sum();
            assert_eq!(ac, usize::from(AC_TOTAL));
            assert_eq!(u16::from_le_bytes([parts[ac_start][5 + 1], parts[ac_start][5 + 2]]), AC_TOTAL);
            let mut alternate = 0;
            let mut seen = Vec::new();
            for p in &parts {
                if p[1] == INTERFACE && p[2] == 3 {
                    alternate = p[3];
                }
                if p[1] == ENDPOINT && p[2] == 0x07 {
                    seen.push(alternate);
                    assert_eq!(u16::from_le_bytes([p[4], p[5]]), c.max_packet(alternate).unwrap());
                    assert_eq!(p[3], ISO_ASYNC_DATA);
                }
                if p[1] == ENDPOINT && p[2] == 0x83 {
                    assert_eq!(&p[3..7], &[ISO_FEEDBACK, feedback[0], feedback[1], feedback[2]]);
                }
            }
            assert_eq!(seen, if raw { [1, 2, 3, 4].to_vec() } else { [1, 2, 3].to_vec() });
            assert_eq!(c.max_packet(3), Some(packet32));
            assert_eq!(c.max_packet(4), if raw { Some(packet32) } else { None });
            assert_eq!(parts[0], &[8, INTERFACE_ASSOCIATION, 2, 2, 1, 0, 0x20, 0]);
            assert!(parts.iter().any(|p| *p == [8, CS_INTERFACE, 0x0a, 1, 0x03, 0x07, 0, 0]));
        }
        assert_eq!(function_descriptor(&config(UsbSpeed::High, true), &mut [0; 301]), Err(DescriptorTooLarge));
    }

    #[test]
    fn the_terminal_and_unit_descriptors_mirror_the_stock_topology() {
        let bytes = descriptors(&config(UsbSpeed::High, true));
        let parts: Vec<&[u8]> = Walk::new(&bytes).collect();
        let stock: [&[u8]; 6] = [
            &[0x11, 0x24, 0x02, 0x03, 0x01, 0x01, 0x00, 0x01, 0x02, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
            &[0x0c, 0x24, 0x03, 0x04, 0x02, 0x03, 0x00, 0x0a, 0x01, 0x00, 0x00, 0x00],
            &[0x12, 0x24, 0x06, 0x0a, 0x03, 0x03, 0x00, 0x00, 0x00, 0x0c, 0x00, 0x00, 0x00, 0x0c, 0x00, 0x00, 0x00, 0x00],
            &[0x10, 0x24, 0x01, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x80, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00],
            &[0x08, 0x25, 0x01, 0x00, 0x00, 0x02, 0x02, 0x00],
            &[0x07, 0x05, 0x83, 0x11, 0x04, 0x00, 0x04],
        ];
        for descriptor in stock {
            assert!(parts.contains(&descriptor), "{descriptor:02x?}");
        }
        assert!(parts.contains(&&[0x09, 0x24, 0x01, 0x00, 0x02, 0x0a, 0x40, 0x00, 0x00][..]));
    }

    #[test]
    fn the_runtime_function_serves_one_packet_per_millisecond_up_to_96_khz() {
        let c = runtime();
        assert_eq!(c.packets_per_second(), 1_000);
        assert_eq!([1, 2, 3].map(|alternate| c.max_packet(alternate)), [Some(388), Some(582), Some(776)]);
        assert_eq!(c.largest_packet(), 776);
        assert!(c.supports(44_100) && c.supports(96_000) && !c.supports(176_400) && !c.supports(192_000));
        let bytes = descriptors(&c);
        let parts: Vec<&[u8]> = Walk::new(&bytes).collect();
        let data: Vec<&[u8]> = parts.iter().copied().filter(|p| p[1] == ENDPOINT && p[2] == 0x07).collect();
        assert_eq!(data, [[7, 5, 7, 5, 0x84, 1, 4], [7, 5, 7, 5, 0x46, 2, 4], [7, 5, 7, 5, 0x08, 3, 4]]);
        assert!(parts.iter().all(|p| p[1] != ENDPOINT || p[2] != 0x84 || *p == [7, 5, 0x84, 0x11, 4, 0, 4]));
        let mut control = Control::new(c);
        let mut buffer = [0u8; 128];
        assert_eq!(control.control_in(&setup(0xa1, 2, 0x0100, 0x0102, 255), &mut buffer), Ok(50));
        assert_eq!(buffer[38..42], 96_000u32.to_le_bytes());
        let set = setup(0x21, 1, 0x0100, 0x0102, 4);
        assert_eq!(control.control_out(&set, &192_000u32.to_le_bytes()), Err(RequestError::Stall));
        assert_eq!(control.control_out(&set, &88_200u32.to_le_bytes()), Ok(Change::Rate(88_200)));
        let full = Config { speed: UsbSpeed::Full, data_interval: 1, ..c };
        assert_eq!((full.packets_per_second(), full.max_packet(3)), (1_000, Some(776)));
    }

    #[test]
    fn walk_stops_on_a_bad_length() {
        assert_eq!(Walk::new(&[3, 1, 2, 1, 0]).count(), 1);
        assert_eq!(Walk::new(&[9, 4]).count(), 0);
        assert_eq!(Walk::new(&[]).count(), 0);
    }

    #[test]
    fn clock_requests_report_the_rate_list_validity_and_accept_only_listed_rates() {
        let mut control = Control::new(config(UsbSpeed::Full, false));
        let mut buffer = [0u8; 128];
        assert_eq!(control.control_in(&setup(0xa1, 1, 0x0100, 0x0102, 4), &mut buffer), Ok(4));
        assert_eq!(buffer[..4], 48_000u32.to_le_bytes());
        assert_eq!(control.control_in(&setup(0xa1, 1, 0x0200, 0x0102, 1), &mut buffer), Ok(1));
        assert_eq!(buffer[0], 1);
        control.set_clock_valid(false);
        control.control_in(&setup(0xa1, 1, 0x0200, 0x0102, 1), &mut buffer).unwrap();
        assert_eq!(buffer[0], 0);
        assert_eq!(control.control_in(&setup(0xa1, 2, 0x0100, 0x0102, 2), &mut buffer), Ok(2));
        assert_eq!(buffer[..2], [4, 0]);
        assert_eq!(control.control_in(&setup(0xa1, 2, 0x0100, 0x0102, 255), &mut buffer), Ok(50));
        assert_eq!(buffer[2..6], 44_100u32.to_le_bytes());
        assert_eq!(buffer[6..10], 44_100u32.to_le_bytes());
        assert_eq!(buffer[10..14], [0; 4]);
        assert_eq!(buffer[38..42], 96_000u32.to_le_bytes());
        let set = setup(0x21, 1, 0x0100, 0x0102, 4);
        assert_eq!(control.control_out(&set, &44_100u32.to_le_bytes()), Ok(Change::Rate(44_100)));
        assert_eq!(control.state().rate, 44_100);
        assert_eq!(control.control_out(&set, &192_000u32.to_le_bytes()), Err(RequestError::Stall));
        let mut high = Control::new(config(UsbSpeed::High, false));
        assert_eq!(high.control_out(&set, &192_000u32.to_le_bytes()), Ok(Change::Rate(192_000)));
        assert_eq!(high.control_in(&setup(0xa1, 2, 0x0100, 0x0102, 255), &mut buffer), Ok(98));
        assert_eq!(high.control_out(&set, &768_000u32.to_le_bytes()), Err(RequestError::Stall));
        assert_eq!(high.control_out(&set, &[0; 3]), Err(RequestError::Stall));
    }

    #[test]
    fn feature_unit_requests_validate_mute_and_volume() {
        let mut control = Control::new(config(UsbSpeed::Full, false));
        let mut buffer = [0u8; 16];
        assert_eq!(control.control_in(&setup(0xa1, 2, 0x0201, 0x0a02, 8), &mut buffer), Ok(8));
        assert_eq!(buffer[..8], [1, 0, 0x00, 0xc4, 0, 0, 0x80, 0]);
        assert_eq!(control.control_in(&setup(0xa1, 1, 0x0202, 0x0a02, 2), &mut buffer), Ok(2));
        assert_eq!(i16::from_le_bytes([buffer[0], buffer[1]]), INITIAL_VOLUME);
        let volume = setup(0x21, 1, 0x0202, 0x0a02, 2);
        assert_eq!(control.control_out(&volume, &(-129i16).to_le_bytes()), Err(RequestError::Stall));
        assert_eq!(control.control_out(&volume, &(-15_488i16).to_le_bytes()), Err(RequestError::Stall));
        assert_eq!(control.control_out(&volume, &(-640i16).to_le_bytes()), Ok(Change::Volume { channel: 2, value: -640 }));
        assert_eq!(control.host_volume_centi_db(), [-2_000, -250]);
        let mute = setup(0x21, 1, 0x0100, 0x0a02, 1);
        assert_eq!(control.control_out(&mute, &[1]), Ok(Change::Mute(true)));
        assert_eq!(control.control_out(&mute, &[2]), Err(RequestError::Stall));
        control.control_in(&setup(0xa1, 1, 0x0100, 0x0a02, 1), &mut buffer).unwrap();
        assert_eq!(buffer[0], 1);
        assert_eq!(control.control_out(&setup(0x21, 1, 0x0203, 0x0a02, 2), &[0, 0]), Err(RequestError::Stall));
        assert_eq!(control.control_in(&setup(0xa1, 1, 0x0200, 0x0a02, 2), &mut buffer), Err(RequestError::Stall));
    }

    #[test]
    fn other_interfaces_and_alternates_are_routed_or_refused() {
        let mut control = Control::new(config(UsbSpeed::High, false));
        let mut buffer = [0u8; 8];
        assert_eq!(control.control_in(&setup(0xa1, 1, 0x0100, 0x0101, 4), &mut buffer), Err(RequestError::NotAudio));
        assert_eq!(control.control_in(&setup(0x81, 10, 0, 0x0102, 1), &mut buffer), Err(RequestError::NotAudio));
        assert_eq!(control.set_alternate(2, 0), Ok(None));
        assert_eq!(control.set_alternate(2, 1), Err(RequestError::Stall));
        assert_eq!(control.set_alternate(3, 3), Ok(Some(Change::Alternate(3))));
        assert_eq!(control.format(), Some(SampleFormat::S32));
        assert_eq!(control.set_alternate(3, 4), Err(RequestError::Stall));
        assert_eq!(control.set_alternate(0, 1), Err(RequestError::NotAudio));
        control.reset();
        assert_eq!(control.state().alternate, 0);
        assert_eq!(control.profile(), PcmProfile::from_hz(48_000));
        let mut raw = Control::new(config(UsbSpeed::High, true));
        assert_eq!(raw.set_alternate(3, 4), Ok(Some(Change::Alternate(4))));
        assert_eq!(raw.format(), None);
    }
}
