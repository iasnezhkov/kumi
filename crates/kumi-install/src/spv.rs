// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
use kumi_layout::{FLASH_XIP_BASE, SPV_HEADER_RESERVED, SPV_MAGIC_AT, get32};

pub const MAGIC: u32 = 0x4056_5053;
pub const FLAG_CHECKSUM: u32 = 1;
pub const FLAG_ALIAS_48: u32 = 2;
pub const KUMI_USER_FLAGS: u32 = 0x4F41_4D41;
pub const HEADER_RESERVED: usize = SPV_HEADER_RESERVED as usize;
pub const JUMP_OVER_HEADER: u32 = 0x1000_006F;
pub const MAGIC_AT: usize = SPV_MAGIC_AT as usize;
const FLAGS_AT: usize = 0x0C;
const COUNT_AT: usize = 0x1C;
const CPU_ENTRY_AT: usize = 0x20;
const LENGTH_AT: usize = 0x28;
pub const CHECKSUM_AT: usize = 0x2C;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Boots { length: u32, checksummed: bool, alias_48: bool },
    Service,
}

pub fn brom_verdict(flash: &[u8]) -> Verdict {
    if flash.len() < 0x30 || get32(flash, MAGIC_AT) != MAGIC {
        return Verdict::Service;
    }
    let flags = get32(flash, FLAGS_AT);
    let length = get32(flash, LENGTH_AT);
    let alias_48 = flags & FLAG_ALIAS_48 != 0;
    if flags & FLAG_CHECKSUM == 0 {
        return Verdict::Boots { length, checksummed: false, alias_48 };
    }
    if length as usize > flash.len() || (length as usize) < 0x30 {
        return Verdict::Service;
    }
    if image_sum(&flash[..length as usize]) == get32(flash, CHECKSUM_AT) {
        Verdict::Boots { length, checksummed: true, alias_48 }
    } else {
        Verdict::Service
    }
}

pub fn is_kumi_stage0(image: &[u8]) -> bool {
    image.len() >= 0x30
        && get32(image, 0) == JUMP_OVER_HEADER
        && matches!(brom_verdict(image), Verdict::Boots { length, .. } if length <= kumi_layout::STAGE0.size)
}

pub fn image_sum(image: &[u8]) -> u32 {
    let total = image.iter().fold(0u32, |acc, b| acc.wrapping_add(u32::from(*b)));
    image[CHECKSUM_AT..CHECKSUM_AT + 4].iter().fold(total, |acc, b| acc.wrapping_sub(u32::from(*b)))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildError {
    HeaderNotReserved,
    TooLarge,
}

pub fn build_stage0(linked_at_48000000: &[u8], limit: usize) -> Result<Vec<u8>, BuildError> {
    if linked_at_48000000.len() < HEADER_RESERVED || linked_at_48000000[..HEADER_RESERVED].iter().any(|b| *b != 0) {
        return Err(BuildError::HeaderNotReserved);
    }
    if linked_at_48000000.len() > limit {
        return Err(BuildError::TooLarge);
    }
    let mut image = linked_at_48000000.to_vec();
    let words = [JUMP_OVER_HEADER, MAGIC, 0, KUMI_USER_FLAGS, 0, 0, 0, 1];
    for (i, word) in words.iter().enumerate() {
        image[4 * i..4 * i + 4].copy_from_slice(&word.to_le_bytes());
    }
    image[CPU_ENTRY_AT..CPU_ENTRY_AT + 4].copy_from_slice(b"CPU\0");
    image[CPU_ENTRY_AT + 4..CPU_ENTRY_AT + 8].copy_from_slice(&FLASH_XIP_BASE.to_le_bytes());
    let length = image.len() as u32;
    image[LENGTH_AT..LENGTH_AT + 4].copy_from_slice(&length.to_le_bytes());
    image[COUNT_AT..COUNT_AT + 4].copy_from_slice(&1u32.to_le_bytes());
    let sum = image_sum(&image);
    image[CHECKSUM_AT..CHECKSUM_AT + 4].copy_from_slice(&sum.to_le_bytes());
    Ok(image)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verdicts_follow_the_bootrom_rules() {
        let mut code = vec![0u8; 0x400];
        code[0x100..].iter_mut().enumerate().for_each(|(i, b)| *b = (i * 7) as u8);
        let image = build_stage0(&code, 0x1_0000).unwrap();
        assert_eq!(brom_verdict(&image), Verdict::Boots { length: 0x400, checksummed: true, alias_48: false });
        let mut flash = image.clone();
        flash.resize(0x2000, 0xFF);
        assert!(matches!(brom_verdict(&flash), Verdict::Boots { .. }));
        let mut damaged = image.clone();
        damaged[0x300] ^= 1;
        assert_eq!(brom_verdict(&damaged), Verdict::Service);
        assert_eq!(brom_verdict(&vec![0xFF; 0x1000]), Verdict::Service);
        let mut unchecked = image.clone();
        unchecked[FLAGS_AT] &= !1;
        assert!(matches!(brom_verdict(&unchecked), Verdict::Boots { checksummed: false, .. }));
        let mut long = image;
        long[LENGTH_AT..LENGTH_AT + 4].copy_from_slice(&0x0100_0000u32.to_le_bytes());
        assert_eq!(brom_verdict(&long), Verdict::Service);
    }

    #[test]
    fn builder_refuses_bad_inputs() {
        assert_eq!(build_stage0(&[1; 0x200], 0x1_0000), Err(BuildError::HeaderNotReserved));
        assert_eq!(build_stage0(&[0; 0x20], 0x1_0000), Err(BuildError::HeaderNotReserved));
        assert_eq!(build_stage0(&[0; 0x2000], 0x1000), Err(BuildError::TooLarge));
    }
}
