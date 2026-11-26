// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
use ed25519_dalek::{Signature, VerifyingKey};
use embedded_storage::nor_flash::ReadNorFlash;
use kumi_layout::{HEADER_SIZE, HeaderError, ImageHeader, SIGNED_HEADER_LEN, SlotId};
use sha2::{Digest, Sha256};

pub const CHUNK: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VerifyError<E> {
    Flash(E),
    Header(HeaderError),
    Key,
    Digest,
    Signature,
}

pub fn key_id(key: &VerifyingKey) -> [u8; 8] {
    let hash = Sha256::digest(key.as_bytes());
    let mut id = [0u8; 8];
    id.copy_from_slice(&hash[..8]);
    id
}

pub fn image_digest(signed_header: &[u8; SIGNED_HEADER_LEN], payload: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(signed_header);
    hasher.update(payload);
    hasher.finalize().into()
}

pub fn verify_slot<F: ReadNorFlash>(
    flash: &mut F,
    slot: SlotId,
    key: &VerifyingKey,
) -> Result<ImageHeader, VerifyError<F::Error>> {
    verify_slot_raw(flash, slot, key).map(|(header, _)| header)
}

pub fn verify_header<F: ReadNorFlash>(
    flash: &mut F,
    slot: SlotId,
    key: &VerifyingKey,
) -> Result<(ImageHeader, [u8; HEADER_SIZE]), VerifyError<F::Error>> {
    let region = slot.region();
    let mut header_bytes = [0u8; HEADER_SIZE];
    flash.read(region.offset, &mut header_bytes).map_err(VerifyError::Flash)?;
    let header = ImageHeader::decode(&header_bytes, region.size).map_err(VerifyError::Header)?;
    if header.key_id != key_id(key) {
        return Err(VerifyError::Key);
    }
    key.verify_strict(&header.digest, &Signature::from_bytes(&header.signature)).map_err(|_| VerifyError::Signature)?;
    Ok((header, header_bytes))
}

#[inline(never)]
pub(crate) fn stream_payload<F: ReadNorFlash>(
    flash: &mut F,
    slot: SlotId,
    header: &ImageHeader,
    header_bytes: &[u8; HEADER_SIZE],
    on_chunk: &mut dyn FnMut(u32, &[u8]),
) -> Result<bool, F::Error> {
    let base = slot.region().offset;
    let mut hasher = Sha256::new();
    hasher.update(&header_bytes[..SIGNED_HEADER_LEN]);
    let mut chunk = [0u8; CHUNK];
    let mut offset = 0u32;
    while offset < header.payload_size {
        let len = CHUNK.min((header.payload_size - offset) as usize);
        flash.read(base + HEADER_SIZE as u32 + offset, &mut chunk[..len])?;
        hasher.update(&chunk[..len]);
        on_chunk(offset, &chunk[..len]);
        offset += len as u32;
    }
    let digest: [u8; 32] = hasher.finalize().into();
    Ok(digest == header.digest)
}

pub fn verify_slot_raw<F: ReadNorFlash>(
    flash: &mut F,
    slot: SlotId,
    key: &VerifyingKey,
) -> Result<(ImageHeader, [u8; HEADER_SIZE]), VerifyError<F::Error>> {
    let (header, header_bytes) = verify_header(flash, slot, key)?;
    if !stream_payload(flash, slot, &header, &header_bytes, &mut |_, _| {}).map_err(VerifyError::Flash)? {
        return Err(VerifyError::Digest);
    }
    Ok((header, header_bytes))
}
