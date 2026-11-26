// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
use kumi_layout::{BOOT_JOURNAL, FLASH_SIZE, HEADER_SIZE, ImageHeader, PAGE, SECTOR, SLOT_A, SLOT_B, STAGE0, STOCK_COPY};
use sha2::{Digest, Sha256};

use crate::gates::{Gate, GateEvidence, REQUIRED_FOR_INSTALL};
use crate::spv::{self, Verdict};

pub const STOCK_SPAN: u32 = 0x4_0000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BootAfter {
    Stock,
    Service,
    Kumi,
    Unchanged,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Op {
    Erase { offset: u32, len: u32 },
    Program { offset: u32, data: Vec<u8> },
    Verify { offset: u32, len: u32, sha256: [u8; 32] },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step {
    pub unit: usize,
    pub label: String,
    pub op: Op,
    pub boot_after: BootAfter,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlanKind {
    Install,
    RestoreStock,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub kind: PlanKind,
    pub steps: Vec<Step>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlanError {
    BackupSize,
    BackupNotBootable,
    BackupUnchecked,
    BackupIsKumi,
    Stage0NotBootable,
    Stage0Unchecked,
    Stage0Alias,
    Stage0TooLarge,
    ImageHeader,
    ImageTooLarge,
    ImageSignature,
    Stage0LacksKey,
    LiveHead,
    GateMissing(Gate),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RestoreMode {
    StockOnly,
    Exact,
}

pub fn sha256(data: &[u8]) -> [u8; 32] {
    Sha256::digest(data).into()
}

fn check_stock(stock: &[u8]) -> Result<(), PlanError> {
    if stock.len() != STOCK_SPAN as usize {
        return Err(PlanError::BackupSize);
    }
    if spv::is_kumi_stage0(stock) {
        return Err(PlanError::BackupIsKumi);
    }
    match spv::brom_verdict(stock) {
        Verdict::Boots { checksummed: true, .. } => Ok(()),
        Verdict::Boots { checksummed: false, .. } => Err(PlanError::BackupUnchecked),
        Verdict::Service => Err(PlanError::BackupNotBootable),
    }
}

pub(crate) fn check_stage0(stage0: &[u8], public_key: &[u8; 32]) -> Result<(), PlanError> {
    if stage0.len() as u32 > STAGE0.size {
        return Err(PlanError::Stage0TooLarge);
    }
    match spv::brom_verdict(stage0) {
        Verdict::Boots { checksummed: true, alias_48: false, length } if length as usize == stage0.len() => {}
        Verdict::Boots { checksummed: false, .. } => return Err(PlanError::Stage0Unchecked),
        Verdict::Boots { alias_48: true, .. } => return Err(PlanError::Stage0Alias),
        _ => return Err(PlanError::Stage0NotBootable),
    }
    if !stage0.windows(32).any(|w| w == public_key) {
        return Err(PlanError::Stage0LacksKey);
    }
    Ok(())
}

pub(crate) fn check_image(image: &[u8], public_key: &[u8; 32]) -> Result<(), PlanError> {
    if image.len() as u32 > SLOT_A.size {
        return Err(PlanError::ImageTooLarge);
    }
    let mut header = [0u8; HEADER_SIZE];
    header.copy_from_slice(image.get(..HEADER_SIZE).ok_or(PlanError::ImageHeader)?);
    let parsed = ImageHeader::decode(&header, SLOT_A.size).map_err(|_| PlanError::ImageHeader)?;
    if HEADER_SIZE + parsed.payload_size as usize != image.len() {
        return Err(PlanError::ImageHeader);
    }
    let key = ed25519_dalek::VerifyingKey::from_bytes(public_key).map_err(|_| PlanError::ImageSignature)?;
    let mut signed = [0u8; kumi_layout::SIGNED_HEADER_LEN];
    signed.copy_from_slice(&header[..kumi_layout::SIGNED_HEADER_LEN]);
    let digest = kumi_boot_core::image_digest(&signed, &image[HEADER_SIZE..]);
    if digest != parsed.digest || parsed.key_id != kumi_boot_core::key_id(&key) {
        return Err(PlanError::ImageSignature);
    }
    key.verify_strict(&digest, &ed25519_dalek::Signature::from_bytes(&parsed.signature))
        .map_err(|_| PlanError::ImageSignature)?;
    Ok(())
}

struct Builder {
    steps: Vec<Step>,
    unit: usize,
}

impl Builder {
    fn new() -> Self {
        Self { steps: Vec::new(), unit: 0 }
    }

    fn unit(&mut self) {
        self.unit += 1;
    }

    fn push(&mut self, label: &str, op: Op, boot_after: BootAfter) {
        self.steps.push(Step { unit: self.unit, label: label.to_string(), op, boot_after });
    }

    fn erase(&mut self, label: &str, offset: u32, len: u32, boot_after: BootAfter) {
        let end = offset + len.div_ceil(SECTOR) * SECTOR;
        for at in (offset..end).step_by(SECTOR as usize) {
            self.push(label, Op::Erase { offset: at, len: SECTOR }, boot_after);
        }
    }

    fn disarm(&mut self, label: &str) {
        self.push(label, Op::Program { offset: spv::MAGIC_AT as u32, data: vec![0; 4] }, BootAfter::Service);
        self.erase(label, 0, SECTOR, BootAfter::Service);
    }

    fn program(&mut self, label: &str, offset: u32, data: &[u8], boot_after: BootAfter) {
        for (i, chunk) in data.chunks(PAGE as usize).enumerate() {
            if chunk.iter().all(|b| *b == 0xFF) {
                continue;
            }
            self.push(label, Op::Program { offset: offset + i as u32 * PAGE, data: chunk.to_vec() }, boot_after);
        }
    }

    fn program_header_last(&mut self, label: &str, offset: u32, data: &[u8], last_boot: BootAfter) {
        let pages: Vec<(usize, &[u8])> = data.chunks(PAGE as usize).enumerate().collect();
        for (i, chunk) in pages.iter().rev() {
            if chunk.iter().all(|b| *b == 0xFF) {
                continue;
            }
            let mut page = chunk.to_vec();
            if *i == 0 {
                page[spv::CHECKSUM_AT..spv::CHECKSUM_AT + 4].fill(0xFF);
            }
            self.push(label, Op::Program { offset: offset + *i as u32 * PAGE, data: page }, BootAfter::Service);
        }
        self.unit();
        let sum = data[spv::CHECKSUM_AT..spv::CHECKSUM_AT + 4].to_vec();
        self.push(
            "header checksum, written last",
            Op::Program { offset: offset + spv::CHECKSUM_AT as u32, data: sum },
            last_boot,
        );
    }

    fn verify(&mut self, label: &str, offset: u32, data: &[u8], boot_after: BootAfter) {
        self.push(label, Op::Verify { offset, len: data.len() as u32, sha256: sha256(data) }, boot_after);
    }
}

pub fn plan_install(
    stock: &[u8],
    live: &[u8],
    stage0: &[u8],
    image: &[u8],
    public_key: &[u8; 32],
    gates: &GateEvidence,
) -> Result<Plan, PlanError> {
    if let Some(gate) = gates.missing(&REQUIRED_FOR_INSTALL) {
        return Err(PlanError::GateMissing(gate));
    }
    check_stock(stock)?;
    check_stage0(stage0, public_key)?;
    check_image(image, public_key)?;
    if live.len() < SECTOR as usize || live.len() > stock.len() {
        return Err(PlanError::LiveHead);
    }
    let stock_runs = stock[..live.len()] == *live;
    let mut b = Builder::new();
    let early = if stock_runs {
        BootAfter::Stock
    } else {
        b.disarm("disarm the current header");
        b.unit();
        BootAfter::Service
    };
    b.erase("stock copy: erase", STOCK_COPY.offset, STOCK_COPY.size, early);
    b.program("stock copy: program", STOCK_COPY.offset, stock, early);
    b.verify("stock copy: verify", STOCK_COPY.offset, stock, early);
    b.unit();
    b.erase("slot A: erase", SLOT_A.offset, image.len() as u32, early);
    b.program("slot A: program", SLOT_A.offset, image, early);
    b.verify("slot A: verify", SLOT_A.offset, image, early);
    b.erase("slot B: invalidate", SLOT_B.offset, SECTOR, early);
    b.unit();
    if stock_runs {
        b.disarm("disarm stock header");
        b.unit();
    }
    b.erase("stage0 body and journal: erase", SECTOR, STOCK_SPAN - SECTOR, BootAfter::Service);
    let body = stage0.get(SECTOR as usize..).unwrap_or(&[]);
    b.program("stage0 body: program", SECTOR, body, BootAfter::Service);
    if !body.is_empty() {
        b.verify("stage0 body: verify", SECTOR, body, BootAfter::Service);
    }
    b.verify("journal: verify erased", BOOT_JOURNAL.offset, &vec![0xFF; BOOT_JOURNAL.size as usize], BootAfter::Service);
    b.unit();
    let head = &stage0[..stage0.len().min(SECTOR as usize)];
    b.program_header_last("stage0 first sector, header page last", 0, head, BootAfter::Kumi);
    b.verify("stage0: verify", 0, stage0, BootAfter::Kumi);
    Ok(Plan { kind: PlanKind::Install, steps: b.steps })
}

pub fn plan_restore_stock(stock: &[u8], mode: RestoreMode, full_backup: Option<&[u8]>) -> Result<Plan, PlanError> {
    check_stock(stock)?;
    if mode == RestoreMode::Exact {
        let full = full_backup.ok_or(PlanError::BackupSize)?;
        if full.len() != FLASH_SIZE as usize || full[..STOCK_SPAN as usize] != *stock {
            return Err(PlanError::BackupSize);
        }
    }
    let mut b = Builder::new();
    b.disarm("disarm current header");
    b.unit();
    b.erase("stock body: erase", SECTOR, STOCK_SPAN - SECTOR, BootAfter::Service);
    b.program("stock body: program", SECTOR, &stock[SECTOR as usize..], BootAfter::Service);
    b.verify("stock body: verify", SECTOR, &stock[SECTOR as usize..], BootAfter::Service);
    b.unit();
    b.program_header_last("stock first sector, header page last", 0, &stock[..SECTOR as usize], BootAfter::Stock);
    b.verify("stock: verify", 0, stock, BootAfter::Stock);
    if let (RestoreMode::Exact, Some(full)) = (mode, full_backup) {
        b.unit();
        let tail = &full[STOCK_SPAN as usize..];
        b.erase("rest of flash: erase", STOCK_SPAN, FLASH_SIZE - STOCK_SPAN, BootAfter::Stock);
        b.program("rest of flash: program original bytes", STOCK_SPAN, tail, BootAfter::Stock);
        b.verify("whole flash: verify against the backup", 0, full, BootAfter::Stock);
    }
    Ok(Plan { kind: PlanKind::RestoreStock, steps: b.steps })
}

impl Plan {
    pub fn first_step_of_unit(&self, unit: usize) -> usize {
        self.steps.iter().position(|s| s.unit == unit).unwrap_or(self.steps.len())
    }

    pub fn describe(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut last: Option<(&str, BootAfter)> = None;
        let mut count = 0;
        for step in &self.steps {
            let key = (step.label.as_str(), step.boot_after);
            if last == Some(key) {
                count += 1;
                continue;
            }
            if let Some((label, boot)) = last {
                out.push(format!("{label} ×{count} → boots {boot:?}"));
            }
            last = Some(key);
            count = 1;
        }
        if let Some((label, boot)) = last {
            out.push(format!("{label} ×{count} → boots {boot:?}"));
        }
        out
    }
}
