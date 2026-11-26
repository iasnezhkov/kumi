// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
use crate::regs;

pub const OTP_BASE: usize = 0x2000_0010;
pub const OTP_WORDS: usize = 8;

const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

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
    let mut hash = FNV_OFFSET_BASIS;
    let mut w = 0;
    while w < OTP_WORDS {
        let bytes = otp[w].to_le_bytes();
        let mut b = 0;
        while b < 4 {
            hash ^= bytes[b] as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
            b += 1;
        }
        w += 1;
    }
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
