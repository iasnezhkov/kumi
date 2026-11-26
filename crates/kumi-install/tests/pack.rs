// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
use std::collections::HashMap;

use ed25519_dalek::SigningKey;
use embedded_storage::nor_flash::{ErrorType, NorFlash, NorFlashError, NorFlashErrorKind, ReadNorFlash};
use kumi_boot_core::stage0::{DiagAreas, Memory, Outcome, Stage0};
use kumi_install::elf::{self, ElfError};
use kumi_install::pack::*;
use kumi_install::sim::SimFlash;
use kumi_install::spv::{self, BuildError, Verdict};
use kumi_install::{GateEvidence, execute, plan_install};
use kumi_layout::*;

type Seg = (u32, u32, Vec<u8>, u32);

fn elf_bytes(entry: u32, segments: &[Seg]) -> Vec<u8> {
    let mut out = vec![0u8; 52 + 32 * segments.len()];
    out[0..4].copy_from_slice(b"\x7fELF");
    out[4] = 1;
    out[5] = 1;
    out[6] = 1;
    out[16..18].copy_from_slice(&2u16.to_le_bytes());
    out[18..20].copy_from_slice(&243u16.to_le_bytes());
    out[20..24].copy_from_slice(&1u32.to_le_bytes());
    out[24..28].copy_from_slice(&entry.to_le_bytes());
    out[28..32].copy_from_slice(&52u32.to_le_bytes());
    out[40..42].copy_from_slice(&52u16.to_le_bytes());
    out[42..44].copy_from_slice(&32u16.to_le_bytes());
    out[44..46].copy_from_slice(&(segments.len() as u16).to_le_bytes());
    for (i, (vaddr, paddr, data, mem)) in segments.iter().enumerate() {
        let offset = out.len() as u32;
        let at = 52 + 32 * i;
        for (j, word) in [1, offset, *vaddr, *paddr, data.len() as u32, *mem, 7, 4].iter().enumerate() {
            out[at + 4 * j..at + 4 * j + 4].copy_from_slice(&word.to_le_bytes());
        }
        out.extend_from_slice(data);
    }
    out
}

fn signer() -> SigningKey {
    SigningKey::from_bytes(&[21; 32])
}

fn stage0_elf(public: &[u8; 32]) -> Vec<u8> {
    let mut boot = vec![0u8; 0x200];
    boot[0x100..0x110].copy_from_slice(&[0x13; 16]);
    let mut ram: Vec<u8> = (0..0x3000u32).map(|i| (i * 7 + 1) as u8).collect();
    ram[0x800..0x820].copy_from_slice(public);
    elf_bytes(
        0x4800_0100,
        &[(0x4800_0000, 0x4800_0000, boot, 0x200), (0x12_8000, 0x4800_0200, ram, 0x3000), (0x13_1000, 0x13_1000, vec![], 0x400)],
    )
}

fn code() -> Vec<u8> {
    (0..0x4000u32).map(|i| (i % 251) as u8).collect()
}

fn data() -> Vec<u8> {
    (0..0x800u32).map(|i| (i % 13) as u8 | 0x80).collect()
}

fn firmware_elf() -> Vec<u8> {
    elf_bytes(0x40, &[(0, 0, code(), 0x4000), (0x10_2000, 0x10_2000, data(), 0x1800)])
}

const SPEC: ImageSpec = ImageSpec { flags: FLAG_SAFE_MODE_CAPABLE, version: [0, 2, 0], generation: 1 };

#[test]
fn elf_reader_takes_load_segments_and_rejects_foreign_files() {
    let parsed = elf::parse(&firmware_elf()).unwrap();
    assert_eq!(parsed.entry, 0x40);
    assert_eq!(parsed.segments.len(), 2);
    assert_eq!(parsed.segments[1].data, data());
    assert_eq!(parsed.segments[1].mem_size, 0x1800);
    assert_eq!(elf::parse(b"not an elf at all, just some bytes long enough to look like a header...."), Err(ElfError::NotElf));
    let mut wide = firmware_elf();
    wide[4] = 2;
    assert_eq!(elf::parse(&wide), Err(ElfError::NotRiscv32LittleEndian));
    let mut arm = firmware_elf();
    arm[18] = 40;
    assert_eq!(elf::parse(&arm), Err(ElfError::NotRiscv32LittleEndian));
    let whole = firmware_elf();
    assert_eq!(elf::parse(&whole[..whole.len() - 10]), Err(ElfError::SegmentOutsideFile));
    let shrunk = elf_bytes(0, &[(0, 0, vec![1; 8], 4)]);
    assert_eq!(elf::parse(&shrunk), Err(ElfError::MemoryShorterThanFile));
}

#[test]
fn stage0_packing_produces_what_the_boot_rom_starts() {
    let public = signer().verifying_key().to_bytes();
    let parsed = elf::parse(&stage0_elf(&public)).unwrap();
    let image = pack_stage0(&parsed, Some(&public)).unwrap();
    assert_eq!(spv::brom_verdict(&image), Verdict::Boots { length: 0x3200, checksummed: true, alias_48: false });
    assert_eq!(get32(&image, 0), spv::JUMP_OVER_HEADER);
    assert_eq!(&image[0x100..0x110], &[0x13; 16]);
    assert_eq!(image[0x200..], parsed.segments[1].data[..]);
    assert_eq!(pack_stage0(&parsed, Some(&[9; 32])), Err(PackError::Stage0LacksKey));
    let mut moved = parsed.clone();
    moved.entry = 0x4800_0000;
    assert_eq!(pack_stage0(&moved, None), Err(PackError::Stage0Entry));
    let mut misplaced = parsed.clone();
    misplaced.segments[1].paddr = 0x4800_0400;
    assert_eq!(pack_stage0(&misplaced, None), Err(PackError::Stage0RamImage));
    let mut dirty = parsed;
    dirty.segments[0].data[8] = 1;
    assert_eq!(pack_stage0(&dirty, None), Err(PackError::Stage0(BuildError::HeaderNotReserved)));
}

#[test]
fn image_packing_signs_copies_and_zeroes() {
    let parsed = elf::parse(&firmware_elf()).unwrap();
    let image = pack_image(&parsed, &SPEC, &signer()).unwrap();
    let (header, check) = check_image(&image, &signer().verifying_key()).unwrap();
    assert_eq!(check, ImageCheck::Valid);
    assert_eq!(header.entry, 0x40);
    let segments: Vec<(SegmentKind, u32, u32)> = header.segments().map(|s| (s.kind, s.destination, s.size)).collect();
    assert_eq!(
        segments,
        [(SegmentKind::Copy, 0, 0x4000), (SegmentKind::Copy, 0x10_2000, 0x800), (SegmentKind::Zero, 0x10_2800, 0x1000)]
    );
    let mut tampered = image.clone();
    tampered[HEADER_SIZE + 5] ^= 1;
    assert_eq!(check_image(&tampered, &signer().verifying_key()).unwrap().1, ImageCheck::BadDigest);
    let other = SigningKey::from_bytes(&[22; 32]).verifying_key();
    assert_eq!(check_image(&image, &other).unwrap().1, ImageCheck::WrongKey);
    let crowded = elf_bytes(0x40, &[(0, 0, vec![1; 4], 8), (0x100, 0x100, vec![1; 4], 8), (0x200, 0x200, vec![1; 4], 4)]);
    assert_eq!(pack_image(&elf::parse(&crowded).unwrap(), &SPEC, &signer()), Err(PackError::TooManySegments));
    let stray = elf_bytes(0x40, &[(0, 0, vec![1; 0x100], 0x100), (0x20_0000, 0x20_0000, vec![2; 0x100], 0x100)]);
    assert_eq!(
        pack_image(&elf::parse(&stray).unwrap(), &SPEC, &signer()),
        Err(PackError::Header(HeaderError::SegmentDestination))
    );
    let empty = elf_bytes(0x40, &[]);
    assert_eq!(pack_image(&elf::parse(&empty).unwrap(), &SPEC, &signer()), Err(PackError::NoSegments));
}

#[test]
fn keys_round_trip_through_hex() {
    let key = signer();
    let secret = hex(&key.to_bytes());
    assert_eq!(signing_key(&format!("{secret}\n")).unwrap().to_bytes(), key.to_bytes());
    assert_eq!(verifying_key(&hex(key.verifying_key().as_bytes())).unwrap(), key.verifying_key());
    assert_eq!(parse_hex32("12"), Err(PackError::Key));
    assert_eq!(parse_hex32(&"zz".repeat(32)), Err(PackError::Key));
}

#[derive(Debug)]
struct Never;

impl NorFlashError for Never {
    fn kind(&self) -> NorFlashErrorKind {
        NorFlashErrorKind::Other
    }
}

struct Nor(Vec<u8>);

impl ErrorType for Nor {
    type Error = Never;
}

impl ReadNorFlash for Nor {
    const READ_SIZE: usize = 1;

    fn read(&mut self, offset: u32, bytes: &mut [u8]) -> Result<(), Never> {
        bytes.copy_from_slice(&self.0[offset as usize..offset as usize + bytes.len()]);
        Ok(())
    }

    fn capacity(&self) -> usize {
        self.0.len()
    }
}

impl NorFlash for Nor {
    const WRITE_SIZE: usize = 1;
    const ERASE_SIZE: usize = 4096;

    fn erase(&mut self, from: u32, to: u32) -> Result<(), Never> {
        self.0[from as usize..to as usize].fill(0xFF);
        Ok(())
    }

    fn write(&mut self, offset: u32, bytes: &[u8]) -> Result<(), Never> {
        for (i, b) in bytes.iter().enumerate() {
            self.0[offset as usize + i] &= *b;
        }
        Ok(())
    }
}

#[derive(Default)]
struct Ram(HashMap<u32, u8>);

impl Memory for Ram {
    fn write(&mut self, address: u32, data: &[u8]) {
        for (i, b) in data.iter().enumerate() {
            self.0.insert(address + i as u32, *b);
        }
    }

    fn zero(&mut self, address: u32, len: u32) {
        for i in 0..len {
            self.0.insert(address + i, 0);
        }
    }
}

impl Ram {
    fn read(&self, address: u32, len: u32) -> Vec<u8> {
        (0..len).map(|i| self.0.get(&(address + i)).copied().unwrap_or(0xEE)).collect()
    }
}

fn fake_stock() -> Vec<u8> {
    let len = 0x2_02AE;
    let mut image = vec![0xFFu8; 0x4_0000];
    for (i, b) in image[..len].iter_mut().enumerate() {
        *b = (i.wrapping_mul(2_654_435_761) >> 13) as u8;
    }
    let words = [0x0A40_006Fu32, spv::MAGIC, 0x4800_1000, 0x4F41_4D41, 0x6000, 0, 0, 4];
    for (i, w) in words.iter().enumerate() {
        image[4 * i..4 * i + 4].copy_from_slice(&w.to_le_bytes());
    }
    image[0x28..0x2C].copy_from_slice(&(len as u32).to_le_bytes());
    let sum = spv::image_sum(&image[..len]);
    image[0x2C..0x30].copy_from_slice(&sum.to_le_bytes());
    image
}

#[test]
fn packed_artifacts_install_and_stage0_jumps_into_the_packed_image() {
    let key = signer();
    let public = key.verifying_key().to_bytes();
    let stage0 = pack_stage0(&elf::parse(&stage0_elf(&public)).unwrap(), Some(&public)).unwrap();
    let image = pack_image(&elf::parse(&firmware_elf()).unwrap(), &SPEC, &key).unwrap();
    let stock = fake_stock();
    let plan = plan_install(&stock, &stock, &stage0, &image, &public, &GateEvidence::for_simulation()).unwrap();
    let mut flash = SimFlash::new(&stock);
    execute(&plan, &mut flash, 0, |_, _| {}).unwrap();
    assert!(matches!(spv::brom_verdict(&flash.data), Verdict::Boots { checksummed: true, .. }));
    assert_eq!(&flash.data[..stage0.len()], stage0.as_slice());
    let verifying = key.verifying_key();
    let mut ram = Ram::default();
    let mut diag = DiagAreas { current: [0u8; 256], last_crash: [0u8; 256] };
    let (outcome, report, _) = Stage0 { key: Some(&verifying), dry_run: false }.run(Nor(flash.data.clone()), &mut ram, &mut diag);
    let Outcome::Jump { entry, info } = outcome else { panic!("{outcome:?}") };
    assert_eq!((entry, info.slot, info.image_version), (0x40, SlotId::A, [0, 2, 0]));
    assert!(report.stored);
    assert_eq!(ram.read(0, 0x4000), code());
    assert_eq!(ram.read(0x10_2000, 0x800), data());
    assert_eq!(ram.read(0x10_2800, 0x1000), vec![0; 0x1000]);
}

#[test]
fn the_dry_run_on_the_real_stock_dump_predicts_gate_g2() {
    let Ok(path) = std::env::var("KUMI_STOCK_DUMP") else { return };
    let dump = std::fs::read(path).unwrap();
    let verifying = signer().verifying_key();
    let mut ram = Ram::default();
    let mut diag = DiagAreas { current: [0x5Au8; 256], last_crash: [0x5Au8; 256] };
    let flash = Nor(dump.clone());
    let (outcome, report, flash) = Stage0 { key: Some(&verifying), dry_run: true }.run(flash, &mut ram, &mut diag);
    assert_eq!(outcome, Outcome::BootRom(kumi_boot_core::stage0::BootRomReason::NoBootableSlot));
    assert!(!report.journal_found);
    assert_eq!(report.after.boot_count, 1);
    assert!(flash.0 == dump);
    assert!(ram.0.is_empty());
    assert_eq!((diag.current, diag.last_crash), ([0x5A; 256], [0x5A; 256]));
    let encoded = kumi_boot_core::stage0::encode_report(&outcome, &report);
    let hex: String = encoded.iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(hex, G2_ON_STOCK);
}

const G2_ON_STOCK: &str = "4b5330520a0000000000000000000000000000000000000000000000000000000000000000000000000000000000000001000000000000000000000034ba9135";
