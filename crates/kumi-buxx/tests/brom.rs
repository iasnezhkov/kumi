// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
use kumi_buxx::sim::SimBrom;
use kumi_buxx::*;
use kumi_install::spv::{self, Verdict};
use kumi_install::{RestoreMode, execute, plan_install, plan_restore_stock};
use kumi_layout::*;

fn hex(text: &str) -> Vec<u8> {
    (0..text.len()).step_by(2).map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap()).collect()
}

#[test]
fn frames_match_the_python_and_kotlin_vectors() {
    let handshake = encode(cmd::HANDSHAKE, 1, &[]);
    assert_eq!(&handshake[..14], hex("425558581001000100006db58de8").as_slice());
    assert!(handshake[14..].iter().all(|b| *b == 0));
    let mut area = [0u8; 16];
    area[0..4].copy_from_slice(&0x4800_0000u32.to_le_bytes());
    area[4..8].copy_from_slice(&512u32.to_le_bytes());
    area[8..12].copy_from_slice(&256u32.to_le_bytes());
    let read_area = encode(cmd::FLASH_AREA, 0, &area);
    assert_eq!(&read_area[..34], hex("42555858220100001000ad6b3fea00000048000200000001000000000000e6bb2ef5").as_slice());
    assert_eq!(&encode(cmd::FLASH_READ, 0, &[])[..14], hex("4255585823010000000059aaa16b").as_slice());
}

#[test]
fn decode_rejects_damage() {
    let good = encode(0x0223, 0, &[1, 2, 3]);
    assert_eq!(decode(&good, 0x0223, Some(3)).unwrap(), [1, 2, 3]);
    for index in [0, 4, 6, 9, 11, 15, 17] {
        let mut bad = good.clone();
        bad[index] ^= 1;
        assert!(decode(&bad, 0x0223, Some(3)).is_err(), "byte {index}");
    }
    assert_eq!(decode(&good, 0x0222, None), Err(Error::ReplyCommand { expected: 0x0222, got: 0x0223 }));
    assert_eq!(decode(&good, 0x0223, Some(4)), Err(Error::Length { expected: 4, got: 3 }));
}

#[test]
fn identity_reads_and_writes_are_locked_by_default() {
    let mut brom = Brom::new(SimBrom::new(&[]));
    let id = brom.require_chip().unwrap();
    assert_eq!(id.jedec, [0xC8, 0x65, 0x16]);
    assert_eq!(id.capacity(), Some(4 * 1024 * 1024));
    assert_eq!(id.block_protect_bits(), 0);
    assert_eq!(brom.erase_flash(0x3F_0000, SECTOR), Err(Error::WritesLocked));
    assert_eq!(brom.program_flash(0x3F_0000, &[0]), Err(Error::WritesLocked));
    let mut other = SimBrom::new(&[]);
    other.chip_id = 0x1234;
    assert_eq!(Brom::new(other).require_chip(), Err(Error::Chip(0x1234)));
    for (reply, id) in [
        ([0x16, 0x65, 0xEF, 0], [0xEF, 0x65, 0x16]),
        ([0x17, 0x65, 0xC8, 0], [0xC8, 0x65, 0x17]),
        ([0xC8, 0x65, 0x16, 0], [0x16, 0x65, 0xC8]),
    ] {
        let mut flash = SimBrom::new(&[]);
        flash.jedec_reply = reply;
        assert_eq!(Brom::new(flash).require_chip(), Err(Error::Flash(id)));
    }
}

#[test]
fn program_after_area_lands_at_the_area_address_in_any_order() {
    let mut brom = Brom::new(SimBrom::new(&[]));
    brom.unlock_flash_writes();
    brom.erase_flash(0x3F_0000, SECTOR).unwrap();
    brom.program_flash(0x3F_0100, &[0x11; 256]).unwrap();
    brom.program_flash(0x3F_0000, &[0x22; 256]).unwrap();
    let mut back = vec![0u8; 0x300];
    brom.read_flash(0x3F_0000, &mut back).unwrap();
    assert!(back[..256].iter().all(|b| *b == 0x22));
    assert!(back[256..512].iter().all(|b| *b == 0x11));
    assert!(back[512..].iter().all(|b| *b == 0xFF));
    assert_eq!(brom.program_flash(0x3F_0000, &[0xFF; 4]), Err(Error::ProgramStatus(1)));
    assert_eq!(brom.read_flash(FLASH_SIZE - 4, &mut [0u8; 8]), Err(Error::Range));
}

#[test]
fn ram_load_run_and_backup() {
    let image: Vec<u8> = (0..3_000u32).map(|i| i as u8).collect();
    let mut brom = Brom::new(SimBrom::new(&[0xAB; 0x1000]));
    brom.load_and_run(0x2000, &image).unwrap();
    let mut report = vec![0u8; 512];
    brom.read_ram(0x8000, &mut report).unwrap();
    let backup = brom.backup(0x2_0000, |_| {}).unwrap();
    assert!(backup[..0x1000].iter().all(|b| *b == 0xAB));
    brom.reboot().unwrap();
    let sim = brom.into_link();
    assert_eq!(sim.executed, [0x2000]);
    assert_eq!(&sim.ram[0x2000..0x2000 + image.len()], image.as_slice());
    assert_eq!(sim.reboots, 1);
}

fn stock() -> Vec<u8> {
    let mut image = vec![0xFFu8; 0x4_0000];
    let len = 0x2_0000;
    for (i, b) in image[..len].iter_mut().enumerate() {
        *b = (i * 7 + 3) as u8;
    }
    for (i, w) in [0x0A40_006Fu32, spv::MAGIC, 0, 0x4F41_4D41, 0, 0, 0, 1].iter().enumerate() {
        image[4 * i..4 * i + 4].copy_from_slice(&w.to_le_bytes());
    }
    image[0x28..0x2C].copy_from_slice(&(len as u32).to_le_bytes());
    let sum = spv::image_sum(&image[..len]);
    image[0x2C..0x30].copy_from_slice(&sum.to_le_bytes());
    image
}

#[test]
fn install_and_restore_run_end_to_end_through_the_protocol() {
    use ed25519_dalek::{Signer, SigningKey};
    let signer = SigningKey::from_bytes(&[5; 32]);
    let public = signer.verifying_key().to_bytes();
    let mut code = vec![0u8; 0x2_000];
    code[0x100..0x120].copy_from_slice(&public);
    let stage0 = spv::build_stage0(&code, STAGE0.size as usize).unwrap();
    let payload = vec![0x5A; 0x3_000];
    let mut header = ImageHeader {
        flags: 0,
        version: [0, 1, 0],
        generation: 1,
        payload_size: payload.len() as u32,
        entry: 0,
        segments: [Some(Segment { kind: SegmentKind::Copy, destination: 0, offset: 0, size: 0x3_000 }), None, None, None],
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
    let stock = stock();
    let mut brom = Brom::new(SimBrom::new(&stock));
    brom.require_chip().unwrap();
    brom.unlock_flash_writes();
    let backup = brom.backup(FLASH_SIZE, |_| {}).unwrap();
    let install = plan_install(
        &backup[..0x4_0000],
        &backup[..0x1000],
        &stage0,
        &image,
        &public,
        &kumi_install::GateEvidence::for_simulation(),
    )
    .unwrap();
    execute(&install, &mut brom, 0, |_, _| {}).unwrap();
    let flash = brom.into_link().flash;
    assert!(matches!(spv::brom_verdict(&flash), Verdict::Boots { alias_48: false, .. }));
    assert_eq!(&flash[..stage0.len()], stage0.as_slice());
    let mut brom = Brom::new(SimBrom::new(&flash));
    brom.unlock_flash_writes();
    let restore = plan_restore_stock(&backup[..0x4_0000], RestoreMode::Exact, Some(&backup)).unwrap();
    execute(&restore, &mut brom, 0, |_, _| {}).unwrap();
    assert!(brom.into_link().flash == backup);
}

#[test]
fn reply_codes_match_the_case_boot_rom() {
    for (command, reply) in [
        (0x0110, 0x0210),
        (0x0120, 0x0220),
        (0x0121, 0x0221),
        (0x0122, 0x0222),
        (0x0123, 0x0223),
        (0x0124, 0x0224),
        (0x0125, 0x0225),
        (0x0126, 0x0226),
        (0x0128, 0x0228),
        (0x012C, 0x022C),
    ] {
        assert_eq!(reply_code(command), reply);
    }
    let mut packet = hex("42555858100200002c0024cede97");
    packet.extend_from_slice(&[0u8; 44]);
    packet.extend_from_slice(&crc32(&[0u8; 44]).to_le_bytes());
    assert_eq!(decode(&packet, reply_code(cmd::HANDSHAKE), Some(44)).unwrap().len(), 44);
}

struct Flaky {
    sim: SimBrom,
    reads: u32,
    corrupt_read: u32,
    corrupt_ram: bool,
}

impl Link for Flaky {
    fn transfer(&mut self, packet: &[u8]) -> Result<Vec<u8>, Error> {
        let command = u16::from_le_bytes([packet[4], packet[5]]);
        if command == cmd::FLASH_READ {
            self.reads += 1;
            if self.reads == self.corrupt_read {
                self.sim.flash[0] ^= 1;
                let reply = self.sim.transfer(packet);
                self.sim.flash[0] ^= 1;
                return reply;
            }
        }
        if command == cmd::RAM_READ && self.corrupt_ram {
            self.sim.ram[0x2000] ^= 1;
        }
        self.sim.transfer(packet)
    }
}

#[test]
fn unstable_reads_and_a_ram_mismatch_have_their_own_errors() {
    let flaky = Flaky { sim: SimBrom::new(&[0x5A; 0x1000]), reads: 0, corrupt_read: 1, corrupt_ram: false };
    let mut brom = Brom::new(flaky);
    assert_eq!(brom.backup(0x1_0000, |_| {}), Err(Error::ReadsDiffer));
    let flaky = Flaky { sim: SimBrom::new(&[]), reads: 0, corrupt_read: 0, corrupt_ram: true };
    let mut brom = Brom::new(flaky);
    assert_eq!(brom.load_and_run(0x2000, &[1, 2, 3, 4]), Err(Error::RamReadback));
    assert!(brom.into_link().sim.executed.is_empty());
}

struct Recorder {
    sim: SimBrom,
    commands: Vec<u16>,
}

impl Link for Recorder {
    fn transfer(&mut self, packet: &[u8]) -> Result<Vec<u8>, Error> {
        self.commands.push(u16::from_le_bytes([packet[4], packet[5]]));
        self.sim.transfer(packet)
    }
}

#[test]
fn every_session_opens_with_the_handshake_the_boot_rom_waits_for() {
    let mut raw = SimBrom::new(&[]);
    assert!(raw.transfer(&encode(cmd::JEDEC, 0, &[])).is_err());
    let mut brom = Brom::new(Recorder { sim: SimBrom::new(&[0x5A; 0x1000]), commands: Vec::new() });
    let mut head = [0u8; 16];
    brom.read_flash(0, &mut head).unwrap();
    assert_eq!(head, [0x5A; 16]);
    brom.reboot().unwrap();
    brom.read_flash(0, &mut head).unwrap();
    let commands = brom.into_link().commands;
    let handshakes: Vec<usize> = commands.iter().enumerate().filter(|(_, c)| **c == cmd::HANDSHAKE).map(|(i, _)| i).collect();
    let reboot = commands.iter().position(|c| *c == cmd::REBOOT).unwrap();
    assert_eq!(handshakes, [0, reboot + 1]);
}
