// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
use ed25519_dalek::{Signer, SigningKey, VerifyingKey};
use kumi_layout::{
    FLASH_XIP_BASE, HEADER_SIZE, HeaderError, ImageHeader, MAX_SEGMENTS, SIGNED_HEADER_LEN, SLOT_A, STAGE0, STAGE0_BOOT_ENTRY,
    STAGE0_RAM_IMAGE_OFFSET, STAGE0_XIP_ENTRY, Segment, SegmentKind,
};

use crate::elf::{Elf, ElfError, FlattenError, flatten_by_load_address};
use crate::spv::{self, BuildError};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PackError {
    Elf(ElfError),
    Flatten(FlattenError),
    Stage0(BuildError),
    Stage0Entry,
    Stage0RamImage,
    Stage0LacksKey,
    NoSegments,
    TooManySegments,
    Header(HeaderError),
    Key,
}

impl From<ElfError> for PackError {
    fn from(e: ElfError) -> Self {
        PackError::Elf(e)
    }
}

impl From<FlattenError> for PackError {
    fn from(e: FlattenError) -> Self {
        PackError::Flatten(e)
    }
}

pub fn pack_stage0(elf: &Elf, public_key: Option<&[u8; 32]>) -> Result<Vec<u8>, PackError> {
    if elf.entry != STAGE0_XIP_ENTRY {
        return Err(PackError::Stage0Entry);
    }
    let ram_lma = FLASH_XIP_BASE + STAGE0_RAM_IMAGE_OFFSET;
    if !elf.segments.iter().any(|s| s.paddr == ram_lma && s.vaddr == STAGE0_BOOT_ENTRY && !s.data.is_empty()) {
        return Err(PackError::Stage0RamImage);
    }
    let flat = flatten_by_load_address(elf, FLASH_XIP_BASE, STAGE0.size)?;
    let image = spv::build_stage0(&flat, STAGE0.size as usize).map_err(PackError::Stage0)?;
    if let Some(key) = public_key
        && !image.windows(32).any(|w| w == key)
    {
        return Err(PackError::Stage0LacksKey);
    }
    Ok(image)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageSpec {
    pub flags: u16,
    pub version: [u8; 3],
    pub generation: u32,
}

pub fn pack_image(elf: &Elf, spec: &ImageSpec, key: &SigningKey) -> Result<Vec<u8>, PackError> {
    let mut payload = Vec::new();
    let mut segments = Vec::new();
    for s in &elf.segments {
        if !s.data.is_empty() {
            let offset = payload.len() as u32;
            segments.push(Segment { kind: SegmentKind::Copy, destination: s.vaddr, offset, size: s.data.len() as u32 });
            payload.extend_from_slice(&s.data);
        }
        let zeroed = s.mem_size - s.data.len() as u32;
        if zeroed > 0 {
            let destination = s.vaddr + s.data.len() as u32;
            segments.push(Segment { kind: SegmentKind::Zero, destination, offset: 0, size: zeroed });
        }
    }
    if segments.is_empty() {
        return Err(PackError::NoSegments);
    }
    if segments.len() > MAX_SEGMENTS {
        return Err(PackError::TooManySegments);
    }
    let mut table = [None; MAX_SEGMENTS];
    for (slot, segment) in table.iter_mut().zip(segments) {
        *slot = Some(segment);
    }
    let mut header = ImageHeader {
        flags: spec.flags,
        version: spec.version,
        generation: spec.generation,
        payload_size: payload.len() as u32,
        entry: elf.entry,
        segments: table,
        key_id: kumi_boot_core::key_id(&key.verifying_key()),
        digest: [0; 32],
        signature: [0; 64],
    };
    let mut signed = [0u8; SIGNED_HEADER_LEN];
    signed.copy_from_slice(&header.encode()[..SIGNED_HEADER_LEN]);
    header.digest = kumi_boot_core::image_digest(&signed, &payload);
    header.signature = key.sign(&header.digest).to_bytes();
    let encoded = header.encode();
    ImageHeader::decode(&encoded, SLOT_A.size).map_err(PackError::Header)?;
    let mut image = encoded.to_vec();
    image.extend_from_slice(&payload);
    Ok(image)
}

pub fn parse_hex32(text: &str) -> Result<[u8; 32], PackError> {
    let text = text.trim();
    if text.len() != 64 || !text.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err(PackError::Key);
    }
    let mut out = [0u8; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[2 * i..2 * i + 2], 16).map_err(|_| PackError::Key)?;
    }
    Ok(out)
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn signing_key(secret_hex: &str) -> Result<SigningKey, PackError> {
    Ok(SigningKey::from_bytes(&parse_hex32(secret_hex)?))
}

pub fn verifying_key(public_hex: &str) -> Result<VerifyingKey, PackError> {
    VerifyingKey::from_bytes(&parse_hex32(public_hex)?).map_err(|_| PackError::Key)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageCheck {
    Valid,
    WrongKey,
    BadDigest,
    BadSignature,
}

pub fn check_image(image: &[u8], key: &VerifyingKey) -> Result<(ImageHeader, ImageCheck), PackError> {
    let mut raw = [0u8; HEADER_SIZE];
    raw.copy_from_slice(image.get(..HEADER_SIZE).ok_or(PackError::Header(HeaderError::Magic))?);
    let header = ImageHeader::decode(&raw, SLOT_A.size).map_err(PackError::Header)?;
    let payload =
        image.get(HEADER_SIZE..HEADER_SIZE + header.payload_size as usize).ok_or(PackError::Header(HeaderError::TooLarge))?;
    if header.key_id != kumi_boot_core::key_id(key) {
        return Ok((header, ImageCheck::WrongKey));
    }
    let mut signed = [0u8; SIGNED_HEADER_LEN];
    signed.copy_from_slice(&raw[..SIGNED_HEADER_LEN]);
    if kumi_boot_core::image_digest(&signed, payload) != header.digest {
        return Ok((header, ImageCheck::BadDigest));
    }
    let signature = ed25519_dalek::Signature::from_bytes(&header.signature);
    if key.verify_strict(&header.digest, &signature).is_err() {
        return Ok((header, ImageCheck::BadSignature));
    }
    Ok((header, ImageCheck::Valid))
}
