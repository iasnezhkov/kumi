// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
use kumi_icd::{StreamFormat, StreamKind};

use crate::clock::Family;

pub const BCLK_PER_FRAME: u32 = 64;
pub const I2S_CONTROL: u32 = 0x0808_0d94;
pub const I2S_CONTROL_DOP: u32 = 0x0888_0e94;
pub const I2S_CONTROL_NATIVE_DSD: u32 = 0x08c8_0f94;
pub const DSD_SILENCE_WORD: i32 = 0x6969_6969;
pub const DAC_MCLK_HZ: u32 = 24_576_000;
pub const FULL_SPEED_PACKET_LIMIT: u16 = 1023;
pub const HIGH_SPEED_PACKET_LIMIT: u16 = 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UsbSpeed {
    Full,
    High,
}

impl UsbSpeed {
    pub const fn intervals_per_second(self) -> u32 {
        match self {
            Self::Full => 1_000,
            Self::High => 8_000,
        }
    }

    pub const fn packet_limit(self) -> u16 {
        match self {
            Self::Full => FULL_SPEED_PACKET_LIMIT,
            Self::High => HIGH_SPEED_PACKET_LIMIT,
        }
    }

    pub const fn packets_per_second(self, interval: u8) -> u32 {
        let exponent = if interval == 0 {
            0
        } else if interval > 16 {
            15
        } else {
            interval - 1
        };
        let packets = self.intervals_per_second() >> exponent;
        if packets == 0 { 1 } else { packets }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SampleFormat {
    S16,
    S24,
    S32,
}

impl SampleFormat {
    pub const ALL: [Self; 3] = [Self::S16, Self::S24, Self::S32];

    pub const fn subslot_bytes(self) -> u8 {
        match self {
            Self::S16 => 2,
            Self::S24 => 3,
            Self::S32 => 4,
        }
    }

    pub const fn bits(self) -> u8 {
        self.subslot_bytes() * 8
    }

    pub const fn frame_bytes(self) -> usize {
        self.subslot_bytes() as usize * 2
    }

    pub const fn alternate(self) -> u8 {
        match self {
            Self::S16 => 1,
            Self::S24 => 2,
            Self::S32 => 3,
        }
    }

    pub const fn from_alternate(alternate: u8) -> Option<Self> {
        match alternate {
            1 => Some(Self::S16),
            2 => Some(Self::S24),
            3 => Some(Self::S32),
            _ => None,
        }
    }

    pub const fn from_bits(bits: u8) -> Option<Self> {
        match bits {
            16 => Some(Self::S16),
            24 => Some(Self::S24),
            32 => Some(Self::S32),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PcmProfile {
    hz: u32,
    rate_code: u8,
    asp_n: u16,
    asp_m: u16,
    i2s_divider: u8,
}

const fn profile(hz: u32, rate_code: u8, asp_n: u16, asp_m: u16, i2s_divider: u8) -> PcmProfile {
    PcmProfile { hz, rate_code, asp_n, asp_m, i2s_divider }
}

pub const PROFILES: [PcmProfile; 8] = [
    profile(44_100, 1, 147, 1280, 16),
    profile(48_000, 2, 1, 8, 16),
    profile(88_200, 3, 147, 640, 8),
    profile(96_000, 4, 1, 4, 8),
    profile(176_400, 5, 147, 320, 4),
    profile(192_000, 6, 1, 2, 4),
    profile(352_800, 7, 147, 160, 2),
    profile(384_000, 8, 1, 1, 2),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormatError {
    Rate(u32),
    Bits(u8),
    Channels(u8),
    Kind(StreamKind),
    PacketTooLarge { required: u16, limit: u16 },
}

impl PcmProfile {
    pub const fn from_hz(hz: u32) -> Option<Self> {
        let mut i = 0;
        while i < PROFILES.len() {
            if PROFILES[i].hz == hz {
                return Some(PROFILES[i]);
            }
            i += 1;
        }
        None
    }

    pub const fn hz(self) -> u32 {
        self.hz
    }

    pub const fn family(self) -> Family {
        match Family::of_rate(self.hz) {
            Some(family) => family,
            None => Family::Rate48k,
        }
    }

    pub const fn dac_rate_code(self) -> u8 {
        self.rate_code
    }

    pub const fn asp_n(self) -> u16 {
        self.asp_n
    }

    pub const fn asp_m(self) -> u16 {
        self.asp_m
    }

    pub const fn i2s_divider(self) -> u8 {
        self.i2s_divider
    }

    pub const fn i2s_clock_word(self) -> u32 {
        ((self.i2s_divider as u32 - 1) << 4) | 3
    }

    pub const fn bclk_hz(self) -> u32 {
        self.hz * BCLK_PER_FRAME
    }

    pub const fn nominal_frames_per_interval_q16(self, speed: UsbSpeed) -> u32 {
        (((self.hz as u64) << 16) / speed.intervals_per_second() as u64) as u32
    }

    pub const fn max_packet_bytes(self, speed: UsbSpeed, format: SampleFormat) -> u16 {
        self.packet_bytes(speed.intervals_per_second(), format)
    }

    pub const fn packet_bytes(self, packets_per_second: u32, format: SampleFormat) -> u16 {
        let frames = (self.hz * 1001).div_ceil(packets_per_second * 1000);
        (frames as usize * format.frame_bytes()) as u16
    }

    pub const fn fits(self, speed: UsbSpeed, format: SampleFormat) -> bool {
        self.max_packet_bytes(speed, format) <= speed.packet_limit()
    }

    pub fn check_packet(self, speed: UsbSpeed, format: SampleFormat) -> Result<u16, FormatError> {
        let required = self.max_packet_bytes(speed, format);
        let limit = speed.packet_limit();
        if required > limit { Err(FormatError::PacketTooLarge { required, limit }) } else { Ok(required) }
    }

    pub const fn stream_format(self, format: SampleFormat) -> StreamFormat {
        StreamFormat { kind: StreamKind::Pcm, rate_hz: self.hz, bits: format.bits(), channels: 2 }
    }

    pub fn from_stream_format(stream: &StreamFormat) -> Result<(Self, SampleFormat), FormatError> {
        if stream.kind != StreamKind::Pcm {
            return Err(FormatError::Kind(stream.kind));
        }
        if stream.channels != 2 {
            return Err(FormatError::Channels(stream.channels));
        }
        let format = SampleFormat::from_bits(stream.bits).ok_or(FormatError::Bits(stream.bits))?;
        let profile = Self::from_hz(stream.rate_hz).ok_or(FormatError::Rate(stream.rate_hz))?;
        Ok((profile, format))
    }
}

pub fn rates_for(speed: UsbSpeed) -> impl Iterator<Item = PcmProfile> {
    PROFILES.into_iter().filter(move |p| SampleFormat::ALL.iter().all(|f| p.fits(speed, *f)))
}

pub fn rates_at(speed: UsbSpeed, packets_per_second: u32, max_hz: u32) -> impl Iterator<Item = PcmProfile> {
    let limit = speed.packet_limit();
    PROFILES
        .into_iter()
        .filter(move |p| p.hz() <= max_hz && SampleFormat::ALL.iter().all(|f| p.packet_bytes(packets_per_second, *f) <= limit))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DsdRate {
    Dsd64,
    Dsd128,
    Dsd256,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DsdMode {
    Dop,
    Native,
}

impl DsdMode {
    pub const fn i2s_control(self) -> u32 {
        match self {
            Self::Dop => I2S_CONTROL_DOP,
            Self::Native => I2S_CONTROL_NATIVE_DSD,
        }
    }

    pub const fn stream_kind(self) -> StreamKind {
        match self {
            Self::Dop => StreamKind::Dop,
            Self::Native => StreamKind::NativeDsd,
        }
    }
}

impl DsdRate {
    pub const fn from_dop_hz(hz: u32) -> Option<Self> {
        match hz {
            176_400 => Some(Self::Dsd64),
            352_800 => Some(Self::Dsd128),
            _ => None,
        }
    }

    pub const fn from_native_hz(hz: u32) -> Option<Self> {
        match hz {
            88_200 => Some(Self::Dsd64),
            176_400 => Some(Self::Dsd128),
            352_800 => Some(Self::Dsd256),
            _ => None,
        }
    }

    pub const fn bit_rate_hz(self) -> u32 {
        match self {
            Self::Dsd64 => 2_822_400,
            Self::Dsd128 => 5_644_800,
            Self::Dsd256 => 11_289_600,
        }
    }

    pub const fn dac_path2(self) -> u8 {
        match self {
            Self::Dsd64 => 0x13,
            Self::Dsd128 => 0x17,
            Self::Dsd256 => 0x1b,
        }
    }

    pub const fn i2s_divider(self) -> u8 {
        match self {
            Self::Dsd64 => 16,
            Self::Dsd128 => 8,
            Self::Dsd256 => 4,
        }
    }

    pub const fn i2s_clock_word(self) -> u32 {
        ((self.i2s_divider() as u32 - 1) << 4) | 3
    }

    pub const fn stream_format(self, mode: DsdMode) -> StreamFormat {
        StreamFormat { kind: mode.stream_kind(), rate_hz: self.bit_rate_hz(), bits: 1, channels: 2 }
    }
}

pub fn max_packet_for(speed: UsbSpeed, format: SampleFormat) -> u16 {
    rates_for(speed).map(|p| p.max_packet_bytes(speed, format)).max().unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::vec::Vec;

    #[test]
    fn dac_ratios_reproduce_64fs_from_the_dac_master_clock() {
        let expected = [
            [1, 0x93, 0, 0, 5],
            [2, 1, 0, 8, 0],
            [3, 0x93, 0, 0x80, 2],
            [4, 1, 0, 4, 0],
            [5, 0x93, 0, 0x40, 1],
            [6, 1, 0, 2, 0],
            [7, 0x93, 0, 0xa0, 0],
            [8, 1, 0, 1, 0],
        ];
        for (p, bytes) in PROFILES.into_iter().zip(expected) {
            let actual = [p.dac_rate_code(), p.asp_n() as u8, (p.asp_n() >> 8) as u8, p.asp_m() as u8, (p.asp_m() >> 8) as u8];
            assert_eq!(actual, bytes);
            assert_eq!(u64::from(p.hz()) * 64 * u64::from(p.asp_m()), u64::from(DAC_MCLK_HZ) * u64::from(p.asp_n()));
        }
    }

    #[test]
    fn i2s_dividers_come_from_the_family_master_clock() {
        let words = [0xf3, 0xf3, 0x73, 0x73, 0x33, 0x33, 0x13, 0x13];
        for (p, word) in PROFILES.into_iter().zip(words) {
            assert_eq!(p.bclk_hz() * u32::from(p.i2s_divider()), p.family().master_hz());
            assert_eq!(p.i2s_clock_word(), word);
        }
        assert_eq!(PcmProfile::from_hz(44_100).unwrap().family(), Family::Rate44k1);
        assert_eq!(PcmProfile::from_hz(384_000).unwrap().family(), Family::Rate48k);
        for hz in [0, 32_000, 50_000, 705_600, 768_000, 1_411_200, 2_822_400, u32::MAX] {
            assert_eq!(PcmProfile::from_hz(hz), None);
        }
    }

    #[test]
    fn packet_sizes_include_feedback_headroom() {
        let fs32 = [360, 392, 712, 776, 1416, 1544, 2832, 3080];
        let hs32 = [48, 56, 96, 104, 184, 200, 360, 392];
        for ((p, full), high) in PROFILES.into_iter().zip(fs32).zip(hs32) {
            assert_eq!(p.max_packet_bytes(UsbSpeed::Full, SampleFormat::S32), full);
            assert_eq!(p.max_packet_bytes(UsbSpeed::High, SampleFormat::S32), high);
        }
        assert_eq!(PcmProfile::from_hz(48_000).unwrap().max_packet_bytes(UsbSpeed::Full, SampleFormat::S24), 294);
        assert_eq!(
            PcmProfile::from_hz(192_000).unwrap().check_packet(UsbSpeed::Full, SampleFormat::S32),
            Err(FormatError::PacketTooLarge { required: 1544, limit: 1023 })
        );
        assert_eq!(PcmProfile::from_hz(192_000).unwrap().check_packet(UsbSpeed::Full, SampleFormat::S16), Ok(772));
    }

    #[test]
    fn full_speed_offers_rates_every_format_fits_and_high_speed_offers_all() {
        let full: Vec<u32> = rates_for(UsbSpeed::Full).map(PcmProfile::hz).collect();
        assert_eq!(full, [44_100, 48_000, 88_200, 96_000]);
        assert_eq!(rates_for(UsbSpeed::High).count(), 8);
        assert_eq!(max_packet_for(UsbSpeed::Full, SampleFormat::S32), 776);
        assert_eq!(max_packet_for(UsbSpeed::High, SampleFormat::S32), 392);
        assert_eq!(max_packet_for(UsbSpeed::High, SampleFormat::S16), 196);
    }

    #[test]
    fn a_one_millisecond_high_speed_interval_sizes_packets_like_full_speed_within_the_high_speed_limit() {
        assert_eq!(
            (UsbSpeed::High.packets_per_second(1), UsbSpeed::High.packets_per_second(4), UsbSpeed::Full.packets_per_second(1)),
            (8_000, 1_000, 1_000)
        );
        assert_eq!((UsbSpeed::Full.packets_per_second(0), UsbSpeed::Full.packets_per_second(40)), (1_000, 1));
        let p96 = PcmProfile::from_hz(96_000).unwrap();
        assert_eq!(
            SampleFormat::ALL.map(|f| p96.packet_bytes(1_000, f)),
            [388, 582, 776],
            "97 frames per millisecond at 96 kHz plus the 0.1 % feedback headroom"
        );
        let p44 = PcmProfile::from_hz(44_100).unwrap();
        assert_eq!(p44.packet_bytes(1_000, SampleFormat::S32), 360);
        let high: Vec<u32> = rates_at(UsbSpeed::High, 1_000, 96_000).map(PcmProfile::hz).collect();
        assert_eq!(high, [44_100, 48_000, 88_200, 96_000]);
        let all: Vec<u32> = rates_at(UsbSpeed::High, 1_000, u32::MAX).map(PcmProfile::hz).collect();
        assert_eq!(all, [44_100, 48_000, 88_200, 96_000], "176.4 kHz S32 needs 1416 bytes per millisecond");
        assert_eq!(rates_at(UsbSpeed::High, 8_000, u32::MAX).count(), 8);
        assert_eq!(rates_at(UsbSpeed::Full, 1_000, 48_000).count(), 2);
    }

    #[test]
    fn feedback_nominals_per_interval() {
        let p48 = PcmProfile::from_hz(48_000).unwrap();
        assert_eq!(p48.nominal_frames_per_interval_q16(UsbSpeed::Full), 48 << 16);
        assert_eq!(p48.nominal_frames_per_interval_q16(UsbSpeed::High), 6 << 16);
        assert_eq!(PcmProfile::from_hz(44_100).unwrap().nominal_frames_per_interval_q16(UsbSpeed::Full), 0x2c_1999);
        assert_eq!(PcmProfile::from_hz(384_000).unwrap().nominal_frames_per_interval_q16(UsbSpeed::High), 48 << 16);
    }

    #[test]
    fn protocol_stream_formats_round_trip() {
        for p in PROFILES {
            for f in SampleFormat::ALL {
                let stream = p.stream_format(f);
                assert_eq!(PcmProfile::from_stream_format(&stream), Ok((p, f)));
                assert_eq!(SampleFormat::from_alternate(f.alternate()), Some(f));
            }
        }
        let mut stream = PROFILES[0].stream_format(SampleFormat::S16);
        stream.kind = StreamKind::Dop;
        assert_eq!(PcmProfile::from_stream_format(&stream), Err(FormatError::Kind(StreamKind::Dop)));
        stream.kind = StreamKind::Pcm;
        stream.channels = 1;
        assert_eq!(PcmProfile::from_stream_format(&stream), Err(FormatError::Channels(1)));
        stream.channels = 2;
        stream.bits = 20;
        assert_eq!(PcmProfile::from_stream_format(&stream), Err(FormatError::Bits(20)));
        stream.bits = 16;
        stream.rate_hz = 768_000;
        assert_eq!(PcmProfile::from_stream_format(&stream), Err(FormatError::Rate(768_000)));
        assert_eq!(SampleFormat::from_alternate(4), None);
    }

    #[test]
    fn dsd_profiles_match_stock_dividers_path_codes_and_control_words() {
        assert_eq!(DsdRate::from_dop_hz(176_400), Some(DsdRate::Dsd64));
        assert_eq!(DsdRate::from_dop_hz(352_800), Some(DsdRate::Dsd128));
        assert_eq!(DsdRate::from_dop_hz(88_200), None);
        assert_eq!(DsdRate::from_dop_hz(192_000), None);
        assert_eq!(DsdRate::from_native_hz(88_200), Some(DsdRate::Dsd64));
        assert_eq!(DsdRate::from_native_hz(176_400), Some(DsdRate::Dsd128));
        assert_eq!(DsdRate::from_native_hz(352_800), Some(DsdRate::Dsd256));
        assert_eq!(DsdRate::from_native_hz(44_100), None);
        assert_eq!([DsdRate::Dsd64, DsdRate::Dsd128, DsdRate::Dsd256].map(DsdRate::dac_path2), [0x13, 0x17, 0x1b]);
        assert_eq!([DsdRate::Dsd64, DsdRate::Dsd128, DsdRate::Dsd256].map(DsdRate::i2s_clock_word), [0xf3, 0x73, 0x33]);
        for rate in [DsdRate::Dsd64, DsdRate::Dsd128, DsdRate::Dsd256] {
            assert_eq!(rate.bit_rate_hz() * u32::from(rate.i2s_divider()), Family::Rate44k1.master_hz());
        }
        assert_eq!((DsdMode::Dop.i2s_control(), DsdMode::Native.i2s_control()), (0x0888_0e94, 0x08c8_0f94));
        assert_eq!(
            DsdRate::Dsd64.stream_format(DsdMode::Dop),
            StreamFormat { kind: StreamKind::Dop, rate_hz: 2_822_400, bits: 1, channels: 2 }
        );
        assert_eq!(
            DsdRate::Dsd256.stream_format(DsdMode::Native),
            StreamFormat { kind: StreamKind::NativeDsd, rate_hz: 11_289_600, bits: 1, channels: 2 }
        );
    }
}
