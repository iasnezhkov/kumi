// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
use crate::regs;

pub const OTP_BASE: usize = 0x2000_0010;
pub const OTP_WORDS: usize = 8;

const SERIAL_KEY_0: u64 = 0x736f_6d65_7073_6575 ^ 0x6b75_6d69_7573_6231;
const SERIAL_KEY_1: u64 = 0x646f_7261_6e64_6f6d ^ 0x7365_7269_616c_7631;

const fn sip_round(mut v0: u64, mut v1: u64, mut v2: u64, mut v3: u64) -> (u64, u64, u64, u64) {
    v0 = v0.wrapping_add(v1);
    v1 = v1.rotate_left(13) ^ v0;
    v0 = v0.rotate_left(32);
    v2 = v2.wrapping_add(v3);
    v3 = v3.rotate_left(16) ^ v2;
    v0 = v0.wrapping_add(v3);
    v3 = v3.rotate_left(21) ^ v0;
    v2 = v2.wrapping_add(v1);
    v1 = v1.rotate_left(17) ^ v2;
    v2 = v2.rotate_left(32);
    (v0, v1, v2, v3)
}

pub const fn derive_serial(otp: [u32; OTP_WORDS]) -> Option<[u8; 8]> {
    let mut non_zero = false;
    let mut i = 0;
    while i < OTP_WORDS {
        if otp[i] != 0 {
            non_zero = true;
        }
        i += 1;
    }
    if !non_zero {
        return None;
    }
    let mut v0 = 0x736f_6d65_7073_6575 ^ SERIAL_KEY_0;
    let mut v1 = 0x646f_7261_6e64_6f6d ^ SERIAL_KEY_1;
    let mut v2 = 0x6c79_6765_6e65_7261 ^ SERIAL_KEY_0;
    let mut v3 = 0x7465_6462_7974_6573 ^ SERIAL_KEY_1;
    let mut pair = 0;
    while pair < OTP_WORDS / 2 {
        let m = (otp[2 * pair] as u64) | ((otp[2 * pair + 1] as u64) << 32);
        v3 ^= m;
        (v0, v1, v2, v3) = sip_round(v0, v1, v2, v3);
        (v0, v1, v2, v3) = sip_round(v0, v1, v2, v3);
        v0 ^= m;
        pair += 1;
    }
    let last = ((OTP_WORDS * 4) as u64) << 56;
    v3 ^= last;
    (v0, v1, v2, v3) = sip_round(v0, v1, v2, v3);
    (v0, v1, v2, v3) = sip_round(v0, v1, v2, v3);
    v0 ^= last;
    v2 ^= 0xff;
    (v0, v1, v2, v3) = sip_round(v0, v1, v2, v3);
    (v0, v1, v2, v3) = sip_round(v0, v1, v2, v3);
    (v0, v1, v2, v3) = sip_round(v0, v1, v2, v3);
    (v0, v1, v2, v3) = sip_round(v0, v1, v2, v3);
    let hash = v0 ^ v1 ^ v2 ^ v3;
    Some(hash.to_be_bytes())
}

pub const fn format_serial_hex(serial: [u8; 8]) -> [u8; 16] {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = [0u8; 16];
    let mut i = 0;
    while i < 8 {
        out[2 * i] = HEX[(serial[i] >> 4) as usize];
        out[2 * i + 1] = HEX[(serial[i] & 0x0f) as usize];
        i += 1;
    }
    out
}

pub unsafe fn read_otp() -> [u32; OTP_WORDS] {
    let mut out = [0u32; OTP_WORDS];
    for (i, slot) in out.iter_mut().enumerate() {
        *slot = unsafe { regs::read(OTP_BASE + 4 * i) };
    }
    out
}

pub unsafe fn read_serial() -> Option<[u8; 8]> {
    derive_serial(unsafe { read_otp() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_otp_returns_none() {
        assert_eq!(derive_serial([0; OTP_WORDS]), None);
    }

    #[test]
    fn distinct_otps_produce_distinct_hex_serials() {
        let a = derive_serial([0x400d_0002, 0x4901_4400, 0xaedb_00c6, 0x0600_0002, 0, 0, 0xb200_0000, 0]).unwrap();
        let b = derive_serial([0x400d_0002, 0, 0, 0, 0, 0, 0, 0]).unwrap();
        assert_ne!(a, b);
        let hex = format_serial_hex(a);
        assert!(hex.iter().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_lowercase()));
    }
}
