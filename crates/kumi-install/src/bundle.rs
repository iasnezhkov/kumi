// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use kumi_layout::{HEADER_SIZE, ImageHeader, SLOT_A};

use crate::plan::{PlanError, check_image, check_stage0};

pub const MAGIC: [u8; 8] = *b"KUMIPKG1";
pub const HEAD: usize = 48;
pub const SIGNATURE: usize = 64;
pub const BUILD_LEN: usize = 24;
pub const EXTENSION: &str = "kumi";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BundleError {
    Truncated,
    Magic,
    Lengths,
    Key,
    Signature,
    Build,
    Stage0(PlanError),
    Image(PlanError),
    VersionMismatch,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bundle {
    pub version: [u8; 3],
    pub generation: u32,
    pub build: String,
    pub stage0: Vec<u8>,
    pub image: Vec<u8>,
}

fn image_identity(image: &[u8]) -> Option<([u8; 3], u32)> {
    let raw: [u8; HEADER_SIZE] = image.get(..HEADER_SIZE)?.try_into().ok()?;
    let header = ImageHeader::decode(&raw, SLOT_A.size).ok()?;
    Some((header.version, header.generation))
}

fn get32(data: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]])
}

pub fn build(stage0: &[u8], image: &[u8], build: &str, key: &SigningKey) -> Result<Vec<u8>, BundleError> {
    let public = key.verifying_key().to_bytes();
    check_stage0(stage0, &public).map_err(BundleError::Stage0)?;
    check_image(image, &public).map_err(BundleError::Image)?;
    let (version, generation) = image_identity(image).ok_or(BundleError::Image(PlanError::ImageHeader))?;
    if build.len() > BUILD_LEN || build.bytes().any(|b| b == 0) {
        return Err(BundleError::Build);
    }
    let mut out = Vec::with_capacity(HEAD + stage0.len() + image.len() + SIGNATURE);
    out.extend_from_slice(&MAGIC);
    out.extend_from_slice(&version);
    out.push(0);
    out.extend_from_slice(&generation.to_le_bytes());
    out.extend_from_slice(&(stage0.len() as u32).to_le_bytes());
    out.extend_from_slice(&(image.len() as u32).to_le_bytes());
    let mut name = [0u8; BUILD_LEN];
    name[..build.len()].copy_from_slice(build.as_bytes());
    out.extend_from_slice(&name);
    out.extend_from_slice(stage0);
    out.extend_from_slice(image);
    let signature = key.sign(&out).to_bytes();
    out.extend_from_slice(&signature);
    Ok(out)
}

pub fn open(data: &[u8], public_key: &[u8; 32]) -> Result<Bundle, BundleError> {
    if data.len() < HEAD + SIGNATURE {
        return Err(BundleError::Truncated);
    }
    if data[..8] != MAGIC {
        return Err(BundleError::Magic);
    }
    let stage0_len = get32(data, 16) as usize;
    let image_len = get32(data, 20) as usize;
    let body_len = HEAD.checked_add(stage0_len).and_then(|n| n.checked_add(image_len)).ok_or(BundleError::Lengths)?;
    if body_len.checked_add(SIGNATURE) != Some(data.len()) {
        return Err(BundleError::Lengths);
    }
    let key = VerifyingKey::from_bytes(public_key).map_err(|_| BundleError::Key)?;
    let signature = Signature::from_bytes(data[body_len..].try_into().map_err(|_| BundleError::Truncated)?);
    key.verify_strict(&data[..body_len], &signature).map_err(|_| BundleError::Signature)?;
    let stage0 = &data[HEAD..HEAD + stage0_len];
    let image = &data[HEAD + stage0_len..body_len];
    check_stage0(stage0, public_key).map_err(BundleError::Stage0)?;
    check_image(image, public_key).map_err(BundleError::Image)?;
    let version = [data[8], data[9], data[10]];
    let generation = get32(data, 12);
    if image_identity(image) != Some((version, generation)) {
        return Err(BundleError::VersionMismatch);
    }
    let name = &data[24..24 + BUILD_LEN];
    let end = name.iter().position(|b| *b == 0).unwrap_or(BUILD_LEN);
    let build = std::str::from_utf8(&name[..end]).map_err(|_| BundleError::Build)?.to_string();
    Ok(Bundle { version, generation, build, stage0: stage0.to_vec(), image: image.to_vec() })
}
