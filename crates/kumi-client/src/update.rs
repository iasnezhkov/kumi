// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
use std::fmt;

use kumi_icd::{
    SlotInfo, SlotState, SlotTable, UPDATE_BLOCK, UpdateBegin, UpdateBlock, UpdateChunk, UpdateFinish, UpdateManifest, Version,
};
use kumi_layout::{HEADER_SIZE, ImageHeader, SLOT_A};
use sha2::{Digest, Sha256};

use crate::{Client, ClientError, Transport};

#[derive(Debug, PartialEq)]
#[non_exhaustive]
pub enum UpdateError<E> {
    Client(ClientError<E>),
    Image,
    NoFreeSlot,
    Acknowledged { expected: u32, got: u32 },
}

impl<E: fmt::Display> fmt::Display for UpdateError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UpdateError::Client(error) => error.fmt(f),
            UpdateError::Image => f.write_str("the image is not a signed Kumi slot image"),
            UpdateError::NoFreeSlot => f.write_str("the case has no slot it may write: confirm or reboot the trial first"),
            UpdateError::Acknowledged { expected, got } => write!(f, "the case stored {got} bytes where {expected} were sent"),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for UpdateError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            UpdateError::Client(error) => Some(error),
            _ => None,
        }
    }
}

impl<E> From<ClientError<E>> for UpdateError<E> {
    fn from(error: ClientError<E>) -> Self {
        UpdateError::Client(error)
    }
}

pub fn manifest_for(image: &[u8], table: &SlotTable) -> Option<UpdateManifest> {
    let raw: [u8; HEADER_SIZE] = image.get(..HEADER_SIZE)?.try_into().ok()?;
    let header = ImageHeader::decode(&raw, SLOT_A.size).ok()?;
    if HEADER_SIZE + header.payload_size as usize != image.len() {
        return None;
    }
    let target = inactive_slot(table)?;
    let mut signature = [[0u8; 32]; 2];
    signature[0].copy_from_slice(&header.signature[..32]);
    signature[1].copy_from_slice(&header.signature[32..]);
    Some(UpdateManifest {
        slot: target,
        version: Version { major: header.version[0], minor: header.version[1], patch: header.version[2] },
        size: image.len() as u32,
        sha256: Sha256::digest(image).into(),
        signature,
    })
}

pub fn inactive_slot(table: &SlotTable) -> Option<kumi_icd::Slot> {
    let booted = table.booted?;
    table.slots.iter().find(|s| s.slot != booted && s.state != SlotState::Trial).map(|s| s.slot)
}

impl<T: Transport> Client<T> {
    pub fn install_update(
        &mut self,
        image: &[u8],
        mut progress: impl FnMut(u32, u32),
    ) -> Result<SlotInfo, UpdateError<T::Error>> {
        let table = self.slots()?;
        let manifest = manifest_for(image, &table).ok_or(if inactive_slot(&table).is_none() {
            UpdateError::NoFreeSlot
        } else {
            UpdateError::Image
        })?;
        self.call_case::<UpdateBegin, _>(&manifest)?;
        let total = image.len() as u32;
        for (i, chunk) in image.chunks(UPDATE_BLOCK).enumerate() {
            let offset = (i * UPDATE_BLOCK) as u32;
            let block = UpdateBlock { offset, data: heapless::Vec::from_slice(chunk).map_err(|_| UpdateError::Image)? };
            let stored = self.call_case::<UpdateChunk, _>(&block)?;
            let expected = offset + chunk.len() as u32;
            if stored != expected {
                return Err(UpdateError::Acknowledged { expected, got: stored });
            }
            progress(expected, total);
        }
        Ok(self.call_case::<UpdateFinish, _>(&())?)
    }
}
