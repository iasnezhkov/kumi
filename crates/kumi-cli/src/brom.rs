// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
use embedded_storage::nor_flash::{ErrorType, NorFlash, NorFlashError, NorFlashErrorKind, ReadNorFlash};
use kumi_boot_core::journal::Journal;
use kumi_boot_core::policy;
use kumi_boot_core::stage0::BOOT_INFO_LEN;
use kumi_buxx::Brom;
use kumi_flasher::{self, Backup, BootAfter, BromLink, EXPECTED_CHIP, InstallError, Link, Outcome, RestoreMode, UsbBromLink};
use kumi_install::{GateEvidence, elf, sha256, spv};
use kumi_layout::{BOOT_INFO_ADDRESS, BOOT_JOURNAL, FLASH_SIZE, PAGE, RT40_WINDOW, SECTOR, STAGE0, SlotId};
use serde_json::{Value, json};

use super::{Failure, hex, hex_word, number};

pub const USAGE: &str = "brom identify | brom backup PATH | brom install PACKAGE BACKUP SHA256 PUBLIC-KEY-HEX [FROM] | \
brom restore BACKUP SHA256 [stock|exact] [FROM] | brom update-stage0 STAGE0-BIN PUBLIC-KEY-HEX | \
brom run BIN LOAD ENTRY [REPORT LEN] | brom run-elf ELF | brom prefer a|b | brom reboot";

const RAM_LIMIT: u32 = 0x0020_0000;
const RESERVED: [(u32, u32); 2] = [(0x0010_0000, 0x0010_2000), (0x0013_8000, 0x0014_0000)];

fn check_ram_range(address: u32, length: u32) -> Result<(), Failure> {
    if RT40_WINDOW.contains(address, length) {
        return Ok(());
    }
    let end = address.checked_add(length).ok_or_else(|| Failure { code: 2, message: "RAM range overflows u32".into() })?;
    if length == 0 || end > RAM_LIMIT {
        return Err(Failure { code: 2, message: "RAM range is outside 0x00000000..0x00200000 and RT40".into() });
    }
    for (start, stop) in RESERVED {
        if address < stop && start < end {
            return Err(Failure { code: 2, message: "RAM range overlaps boot ROM data or stack".into() });
        }
    }
    Ok(())
}

fn run_ram<L: Link>(link: L, path: &str, load: u32, entry: u32, report: Option<(u32, u32)>) -> Result<Value, Failure> {
    let image = std::fs::read(path).map_err(|e| Failure { code: 1, message: format!("{path}: {e}") })?;
    let length = u32::try_from(image.len()).map_err(|_| Failure { code: 2, message: "RAM image is too large".into() })?;
    check_ram_range(load, length)?;
    if let Some((addr, len)) = report {
        check_ram_range(addr, len)?;
    }
    let mut brom = Brom::new(link);
    let chip_id = brom.handshake().map_err(|e| Failure { code: 1, message: format!("boot ROM: {e:?}") })?;
    if chip_id != EXPECTED_CHIP {
        return Err(Failure { code: 1, message: format!("unexpected chip id {}", hex_word(chip_id)) });
    }
    brom.write_ram(load, &image).map_err(|e| Failure { code: 1, message: format!("boot ROM write_ram: {e:?}") })?;
    let mut echo = vec![0u8; image.len()];
    brom.read_ram(load, &mut echo).map_err(|e| Failure { code: 1, message: format!("boot ROM read_ram: {e:?}") })?;
    if echo != image {
        return Err(Failure { code: 1, message: "boot ROM: RAM readback mismatch".into() });
    }
    brom.execute(entry).map_err(|e| Failure { code: 1, message: format!("boot ROM execute: {e:?}") })?;
    let mut result = json!({
        "chip_id": hex_word(chip_id),
        "load": hex_word(load),
        "size": length,
        "entry": hex_word(entry),
    });
    if let Some((addr, len)) = report {
        let mut raw = vec![0u8; len as usize];
        brom.read_ram(addr, &mut raw).map_err(|e| Failure { code: 1, message: format!("boot ROM report read: {e:?}") })?;
        let words: Vec<String> = raw.chunks_exact(4).map(|c| hex_word(u32::from_le_bytes([c[0], c[1], c[2], c[3]]))).collect();
        result["report_address"] = json!(hex_word(addr));
        result["report_hex"] = json!(hex(&raw));
        result["report_words"] = json!(words);
    }
    Ok(result)
}

fn run_elf<L: Link>(link: L, path: &str) -> Result<Value, Failure> {
    let raw = std::fs::read(path).map_err(|e| Failure { code: 1, message: format!("{path}: {e}") })?;
    let parsed = elf::parse(&raw).map_err(|e| Failure { code: 2, message: format!("{path}: invalid ELF ({e:?})") })?;
    if parsed.entry == BOOT_INFO_ADDRESS {
        return Err(Failure { code: 2, message: "ELF entry overlaps BootInfo address".into() });
    }
    let mut loaded = Vec::new();
    for segment in &parsed.segments {
        if segment.data.is_empty() {
            continue;
        }
        let length =
            u32::try_from(segment.data.len()).map_err(|_| Failure { code: 2, message: "ELF segment too large".into() })?;
        check_ram_range(segment.paddr, length)?;
        loaded.push((segment.paddr, segment.data.as_slice(), segment.mem_size));
    }
    if loaded.is_empty() {
        return Err(Failure { code: 2, message: "ELF has no loadable segments".into() });
    }
    let mut brom = Brom::new(link);
    let chip_id = brom.handshake().map_err(|e| Failure { code: 1, message: format!("boot ROM: {e:?}") })?;
    if chip_id != EXPECTED_CHIP {
        return Err(Failure { code: 1, message: format!("unexpected chip id {}", hex_word(chip_id)) });
    }
    let mut summary = Vec::new();
    for (address, data, mem_size) in loaded {
        brom.write_ram(address, data).map_err(|e| Failure { code: 1, message: format!("boot ROM write_ram: {e:?}") })?;
        let mut echo = vec![0u8; data.len()];
        brom.read_ram(address, &mut echo).map_err(|e| Failure { code: 1, message: format!("boot ROM read_ram: {e:?}") })?;
        if echo != data {
            return Err(Failure { code: 1, message: format!("boot ROM: RAM readback mismatch at {}", hex_word(address)) });
        }
        summary.push(json!({
            "address": hex_word(address),
            "file_bytes": data.len(),
            "memory_bytes": mem_size,
            "sha256": hex(&sha256(data)),
        }));
    }
    let zeros = [0u8; BOOT_INFO_LEN];
    brom.write_ram(BOOT_INFO_ADDRESS, &zeros)
        .map_err(|e| Failure { code: 1, message: format!("boot ROM clear BootInfo: {e:?}") })?;
    let mut check = [0xFFu8; BOOT_INFO_LEN];
    brom.read_ram(BOOT_INFO_ADDRESS, &mut check)
        .map_err(|e| Failure { code: 1, message: format!("boot ROM verify BootInfo: {e:?}") })?;
    if check != zeros {
        return Err(Failure { code: 1, message: "boot ROM: BootInfo readback mismatch".into() });
    }
    brom.execute(parsed.entry).map_err(|e| Failure { code: 1, message: format!("boot ROM execute: {e:?}") })?;
    Ok(json!({
        "chip_id": hex_word(chip_id),
        "entry": hex_word(parsed.entry),
        "segments": summary,
    }))
}

#[derive(Debug)]
struct BromFlashError(kumi_buxx::Error);

impl NorFlashError for BromFlashError {
    fn kind(&self) -> NorFlashErrorKind {
        match self.0 {
            kumi_buxx::Error::Range => NorFlashErrorKind::OutOfBounds,
            _ => NorFlashErrorKind::Other,
        }
    }
}

struct BromNor<'a, L>(&'a mut Brom<L>);

impl<L> ErrorType for BromNor<'_, L> {
    type Error = BromFlashError;
}

impl<L: Link> ReadNorFlash for BromNor<'_, L> {
    const READ_SIZE: usize = 1;

    fn read(&mut self, offset: u32, bytes: &mut [u8]) -> Result<(), Self::Error> {
        self.0.read_flash(offset, bytes).map_err(BromFlashError)
    }

    fn capacity(&self) -> usize {
        FLASH_SIZE as usize
    }
}

impl<L: Link> NorFlash for BromNor<'_, L> {
    const WRITE_SIZE: usize = 1;
    const ERASE_SIZE: usize = SECTOR as usize;

    fn erase(&mut self, from: u32, to: u32) -> Result<(), Self::Error> {
        let len = to.checked_sub(from).ok_or(BromFlashError(kumi_buxx::Error::Range))?;
        self.0.erase_flash(from, len).map_err(BromFlashError)
    }

    fn write(&mut self, offset: u32, bytes: &[u8]) -> Result<(), Self::Error> {
        self.0.program_flash(offset, bytes).map_err(BromFlashError)
    }
}

fn slot_name(slot: SlotId) -> &'static str {
    match slot {
        SlotId::A => "A",
        SlotId::B => "B",
    }
}

fn prefer_slot<L: Link>(link: L, target: &str) -> Result<Value, Failure> {
    let slot = match target {
        "a" | "A" => SlotId::A,
        "b" | "B" => SlotId::B,
        _ => return Err(Failure { code: 2, message: "give slot as a or b".into() }),
    };
    let mut brom = Brom::new(link);
    let identity = brom.require_chip().map_err(|e| Failure { code: 1, message: format!("boot ROM: {e:?}") })?;
    brom.unlock_flash_writes();
    let mut journal =
        Journal::new(BromNor(&mut brom), BOOT_JOURNAL).map_err(|e| Failure { code: 1, message: format!("journal: {e:?}") })?;
    let before = journal
        .load()
        .map_err(|e| Failure { code: 1, message: format!("journal load: {e:?}") })?
        .ok_or_else(|| Failure { code: 1, message: "boot journal is empty".into() })?;
    let next = policy::prefer(&before, slot)
        .map_err(|e| Failure { code: 1, message: format!("slot {} is not bootable ({e:?})", slot_name(slot)) })?;
    let after = journal.store(&next).map_err(|e| Failure { code: 1, message: format!("journal store: {e:?}") })?;
    Ok(json!({
        "chip_id": hex_word(identity.chip_id),
        "previous_preferred": slot_name(before.preferred),
        "preferred": slot_name(after.preferred),
        "sequence": after.sequence,
        "boot_count": after.boot_count,
        "slot_a_generation": after.slot(SlotId::A).generation,
        "slot_b_generation": after.slot(SlotId::B).generation,
    }))
}

fn progress(done: u64, total: u64, label: &str) {
    eprintln!("{}", json!({"progress": label, "done": done, "total": total}));
}

fn from_step(argument: Option<String>) -> Result<usize, Failure> {
    argument.map_or(Ok(0), |text| {
        text.parse().map_err(|_| Failure { code: 2, message: "FROM is the step to resume at, a number".into() })
    })
}

fn parse_key(hex_key: &str) -> Result<[u8; 32], Failure> {
    if hex_key.len() != 64 || !hex_key.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(Failure { code: 2, message: "PUBLIC-KEY-HEX must be 64 hex digits".into() });
    }
    let mut key = [0u8; 32];
    for (i, slot) in key.iter_mut().enumerate() {
        *slot = u8::from_str_radix(&hex_key[2 * i..2 * i + 2], 16)
            .map_err(|_| Failure { code: 2, message: "invalid PUBLIC-KEY-HEX".into() })?;
    }
    Ok(key)
}

fn update_stage0<L: Link>(link: L, path: &str, key_hex: &str) -> Result<Value, Failure> {
    let key = parse_key(key_hex)?;
    let stage0 = std::fs::read(path).map_err(|e| Failure { code: 1, message: format!("{path}: {e}") })?;
    if stage0.len() as u32 > STAGE0.size || !spv::is_kumi_stage0(&stage0) {
        return Err(Failure { code: 2, message: "stage0 binary failed SPV header or size validation".into() });
    }
    match spv::brom_verdict(&stage0) {
        spv::Verdict::Boots { checksummed: true, alias_48: false, length } if length as usize == stage0.len() => {}
        _ => return Err(Failure { code: 2, message: "stage0 binary is not a checksummed Kumi boot image".into() }),
    }
    if !stage0.windows(32).any(|w| w == key) {
        return Err(Failure { code: 2, message: "stage0 binary does not embed the expected public key".into() });
    }
    let mut brom = Brom::new(link);
    let identity = brom.require_chip().map_err(|e| Failure { code: 1, message: format!("boot ROM: {e:?}") })?;
    let chip_id = identity.chip_id;
    let mut current_head = vec![0u8; STAGE0.size as usize];
    brom.read_flash(0, &mut current_head[..SECTOR as usize])
        .map_err(|e| Failure { code: 1, message: format!("boot ROM read head: {e:?}") })?;
    let head_len = kumi_layout::get32(&current_head, 0x28) as usize;
    if head_len <= STAGE0.size as usize && head_len > SECTOR as usize {
        brom.read_flash(SECTOR, &mut current_head[SECTOR as usize..head_len])
            .map_err(|e| Failure { code: 1, message: format!("boot ROM read stage0: {e:?}") })?;
    }
    if !spv::is_kumi_stage0(&current_head[..head_len.min(current_head.len())]) {
        return Err(Failure {
            code: 1,
            message: "refusing to overwrite non-Kumi flash head; use `brom install` with a stock backup".into(),
        });
    }
    brom.unlock_flash_writes();
    brom.program_flash(spv::MAGIC_AT as u32, &[0; 4])
        .map_err(|e| Failure { code: 1, message: format!("boot ROM disarm: {e:?}") })?;
    let erase_len = (stage0.len() as u32).div_ceil(SECTOR) * SECTOR;
    for at in (0..erase_len).step_by(SECTOR as usize) {
        brom.erase_flash(at, SECTOR).map_err(|e| Failure { code: 1, message: format!("boot ROM erase 0x{at:x}: {e:?}") })?;
    }
    let pages: Vec<(usize, &[u8])> = stage0.chunks(PAGE as usize).enumerate().collect();
    for (i, chunk) in pages.iter().rev() {
        if chunk.iter().all(|b| *b == 0xFF) {
            continue;
        }
        let mut page = chunk.to_vec();
        if *i == 0 {
            page[spv::CHECKSUM_AT..spv::CHECKSUM_AT + 4].fill(0xFF);
        }
        brom.program_flash(*i as u32 * PAGE, &page)
            .map_err(|e| Failure { code: 1, message: format!("boot ROM program page {i}: {e:?}") })?;
    }
    let mut pre = vec![0u8; stage0.len()];
    brom.read_flash(0, &mut pre).map_err(|e| Failure { code: 1, message: format!("boot ROM pre-verify: {e:?}") })?;
    let mut expected_pre = stage0.clone();
    expected_pre[spv::CHECKSUM_AT..spv::CHECKSUM_AT + 4].fill(0xFF);
    if pre != expected_pre {
        return Err(Failure { code: 1, message: "boot ROM pre-commit readback mismatch (checksum left unwritten)".into() });
    }
    brom.program_flash(spv::CHECKSUM_AT as u32, &stage0[spv::CHECKSUM_AT..spv::CHECKSUM_AT + 4])
        .map_err(|e| Failure { code: 1, message: format!("boot ROM commit checksum: {e:?}") })?;
    let mut final_read = vec![0u8; stage0.len()];
    brom.read_flash(0, &mut final_read).map_err(|e| Failure { code: 1, message: format!("boot ROM final verify: {e:?}") })?;
    if final_read != stage0 {
        return Err(Failure { code: 1, message: "boot ROM final readback mismatch".into() });
    }
    Ok(json!({
        "chip_id": hex_word(chip_id),
        "stage0_bytes": stage0.len(),
        "erased_bytes": erase_len,
        "sha256": hex(&sha256(&stage0)),
        "verified": true,
    }))
}

fn boot_name(boot: BootAfter) -> &'static str {
    match boot {
        BootAfter::Stock => "Stock",
        BootAfter::Service => "Service",
        BootAfter::Kumi => "Kumi",
        BootAfter::Unchanged => "Unchanged",
    }
}

fn flash_err(error: InstallError) -> Failure {
    let payload = match &error {
        InstallError::Storage(detail) | InstallError::BackupMissing(detail) => {
            json!({ "kind": error.kind(), "detail": detail })
        }
        InstallError::Interrupted(stop) => json!({
            "kind": error.kind(),
            "step": stop.step,
            "resume": stop.resume,
            "label": stop.label,
            "boots": [boot_name(stop.boots[0]), boot_name(stop.boots[1])],
        }),
        _ => json!({ "kind": error.kind() }),
    };
    Failure { code: 1, message: format!("boot ROM: {payload}") }
}

fn flash_outcome(result: Result<Outcome, InstallError>) -> Result<Value, Failure> {
    result.map(|done| json!({ "steps": done.steps as u64, "boots": boot_name(done.boots) })).map_err(flash_err)
}

fn run_with<L: Link>(
    prefix: &str,
    mut open: impl FnMut(bool) -> Result<L, Failure>,
    argument: impl Fn(usize) -> Option<String>,
) -> Result<Value, Failure> {
    let usage = || Failure { code: 2, message: format!("usage: {prefix} {USAGE}") };
    let mut report = progress;
    match argument(1).as_deref() {
        Some("identify") => kumi_flasher::identify(open(false)?)
            .map(|id| {
                json!({
                    "chip": id.chip_id,
                    "chipMatches": id.chip_id == EXPECTED_CHIP,
                    "jedec": hex(&id.jedec),
                    "protect": id.block_protect_bits(),
                    "capacity": id.capacity(),
                })
            })
            .map_err(flash_err),
        Some("backup") => {
            let path = argument(2).ok_or_else(usage)?;
            kumi_flasher::backup(open(false)?, &path, &mut report)
                .map(|r| {
                    json!({
                        "size": r.size,
                        "sha256": r.backup.sha256,
                        "stock": format!("{:?}", r.stock),
                    })
                })
                .map_err(flash_err)
        }
        Some("install") => {
            let (Some(package), Some(backup), Some(sha), Some(key)) = (argument(2), argument(3), argument(4), argument(5)) else {
                return Err(usage());
            };
            let package = std::fs::read(&package).map_err(|e| Failure { code: 1, message: format!("{package}: {e}") })?;
            let from = from_step(argument(6))?;
            let gates = GateEvidence::recorded();
            let stored = Backup { path: backup, sha256: sha };
            flash_outcome(kumi_flasher::install(open(true)?, &stored, &package, &key, &gates, from, &mut report))
        }
        Some("restore") => {
            let (Some(backup), Some(sha)) = (argument(2), argument(3)) else {
                return Err(usage());
            };
            let mode = match argument(4).as_deref() {
                None | Some("stock") => RestoreMode::StockOnly,
                Some("exact") => RestoreMode::Exact,
                Some(_) => return Err(usage()),
            };
            let from = from_step(argument(5))?;
            let stored = Backup { path: backup, sha256: sha };
            flash_outcome(kumi_flasher::restore_stock(open(true)?, &stored, mode, from, &mut report))
        }
        Some("update-stage0") => {
            let (Some(path), Some(key)) = (argument(2), argument(3)) else {
                return Err(usage());
            };
            update_stage0(open(true)?, &path, &key)
        }
        Some("run") => {
            let path = argument(2).ok_or_else(usage)?;
            let load = number(argument(3).as_deref(), "load address")?;
            let entry = number(argument(4).as_deref(), "entry address")?;
            let report = match (argument(5), argument(6)) {
                (None, None) => None,
                (Some(addr), Some(len)) => Some((number(Some(&addr), "report address")?, number(Some(&len), "report length")?)),
                _ => return Err(usage()),
            };
            run_ram(open(false)?, &path, load, entry, report)
        }
        Some("run-elf") => {
            let path = argument(2).ok_or_else(usage)?;
            run_elf(open(false)?, &path)
        }
        Some("prefer") => {
            let target = argument(2).ok_or_else(usage)?;
            prefer_slot(open(true)?, &target)
        }
        Some("reboot") => kumi_flasher::reboot(open(false)?).map(|()| Value::Null).map_err(flash_err),
        _ => Err(usage()),
    }
}

pub fn run(
    serial: &str,
    helper: &str,
    usb_path: &str,
    deadline: u32,
    argument: impl Fn(usize) -> Option<String>,
) -> Result<Value, Failure> {
    run_with(
        "kumi --adb SERIAL --helper PATH --usb-path PATH",
        |writes| {
            BromLink::phone(serial, helper, usb_path, deadline, writes).map_err(|e| Failure { code: 1, message: e.to_string() })
        },
        argument,
    )
}

pub fn run_usb(selector: Option<(u8, u8)>, argument: impl Fn(usize) -> Option<String>) -> Result<Value, Failure> {
    run_with(
        "kumi --usb [--usb-device BUS:ADDR]",
        |_| UsbBromLink::open(selector).map_err(|e| Failure { code: 1, message: format!("--usb: {e}") }),
        argument,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use kumi_boot_core::journal::{BootState, SlotRecord, SlotStatus};
    use kumi_buxx::sim::SimBrom;

    #[test]
    fn prefer_slot_switches_journal_without_touching_slots() {
        let mut sim = SimBrom::new(&[]);
        let initial = BootState {
            sequence: 7,
            boot_count: 69,
            preferred: SlotId::A,
            slots: [
                SlotRecord { status: SlotStatus::Confirmed, attempts: 0, generation: 28 },
                SlotRecord { status: SlotStatus::Confirmed, attempts: 0, generation: 26 },
            ],
            unhealthy_starts: 0,
            bootrom_pending: false,
        };
        let encoded = initial.encode();
        let start = BOOT_JOURNAL.offset as usize;
        sim.flash[start..start + encoded.len()].copy_from_slice(&encoded);
        let result = prefer_slot(sim, "b").unwrap();
        assert_eq!(result["previous_preferred"], "A");
        assert_eq!(result["preferred"], "B");
        assert_eq!(result["sequence"], 8);
        assert_eq!(result["slot_b_generation"], 26);
    }
}
