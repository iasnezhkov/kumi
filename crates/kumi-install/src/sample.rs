// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
use ed25519_dalek::{Signer, SigningKey};
use kumi_layout::{FLAG_SAFE_MODE_CAPABLE, ImageHeader, SIGNED_HEADER_LEN, STAGE0, Segment, SegmentKind};

use crate::plan::STOCK_SPAN;
use crate::spv;

pub const STOCK_LEN: usize = 0x2_02AE;

pub fn stock() -> Vec<u8> {
    let mut image = vec![0xFFu8; STOCK_SPAN as usize];
    for (i, b) in image[..STOCK_LEN].iter_mut().enumerate() {
        *b = (i.wrapping_mul(2_654_435_761) >> 13) as u8;
    }
    let words = [0x0A40_006Fu32, spv::MAGIC, 0x4800_1000, 0x4F41_4D41, 0x6000, 0, 0, 4];
    for (i, w) in words.iter().enumerate() {
        image[4 * i..4 * i + 4].copy_from_slice(&w.to_le_bytes());
    }
    image[0x20..0x24].copy_from_slice(b"CPU\0");
    image[0x24..0x28].copy_from_slice(&kumi_layout::FLASH_XIP_BASE.to_le_bytes());
    image[0x28..0x2C].copy_from_slice(&(STOCK_LEN as u32).to_le_bytes());
    let sum = spv::image_sum(&image[..STOCK_LEN]);
    image[0x2C..0x30].copy_from_slice(&sum.to_le_bytes());
    image
}

pub fn stage0(public: &[u8; 32]) -> Vec<u8> {
    let mut code = vec![0u8; 0x6_400];
    for (i, b) in code[0x100..].iter_mut().enumerate() {
        *b = (i * 13 + 5) as u8;
    }
    code[0x400..0x420].copy_from_slice(public);
    spv::build_stage0(&code, STAGE0.size as usize).expect("the sample stage0 fits its region")
}

pub fn image(signer: &SigningKey, version: [u8; 3], generation: u32) -> Vec<u8> {
    let payload: Vec<u8> = (0..0x9_000u32).map(|i| ((i % 253) as u8) ^ version[2]).collect();
    let mut header = ImageHeader {
        flags: FLAG_SAFE_MODE_CAPABLE,
        version,
        generation,
        payload_size: payload.len() as u32,
        entry: 0x100,
        segments: [
            Some(Segment { kind: SegmentKind::Copy, destination: 0, offset: 0, size: 0x8_000 }),
            Some(Segment { kind: SegmentKind::Copy, destination: 0x0010_2000, offset: 0x8_000, size: 0x1_000 }),
            None,
            None,
        ],
        key_id: kumi_boot_core::key_id(&signer.verifying_key()),
        digest: [0; 32],
        signature: [0; 64],
    };
    let mut signed = [0u8; SIGNED_HEADER_LEN];
    signed.copy_from_slice(&header.encode()[..SIGNED_HEADER_LEN]);
    header.digest = kumi_boot_core::image_digest(&signed, &payload);
    header.signature = signer.sign(&header.digest).to_bytes();
    let mut image = header.encode().to_vec();
    image.extend_from_slice(&payload);
    image
}
