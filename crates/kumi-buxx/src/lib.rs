// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
use kumi_layout::{FLASH_SIZE, FlashTarget, SECTOR, TargetError, crc32, get32};

pub const VENDOR_ID: u16 = 0x0666;
pub const PRODUCT_ID: u16 = 0x0888;
pub const PACKET: usize = 512;
pub const HEADER: usize = 14;
pub const CHUNK: u32 = 256;
pub const FLASH_ALIAS: u32 = 0x4800_0000;
pub const EXPECTED_CHIP: u32 = 0x4901_4480;
pub const FLASH_MANUFACTURER: u8 = 0xC8;
pub const HANDSHAKE_LEN: usize = 44;

pub mod cmd {
    pub const HANDSHAKE: u16 = 0x0110;
    pub const FLASH_ERASE: u16 = 0x0120;
    pub const FLASH_PROGRAM: u16 = 0x0121;
    pub const FLASH_AREA: u16 = 0x0122;
    pub const FLASH_READ: u16 = 0x0123;
    pub const REBOOT: u16 = 0x0124;
    pub const JEDEC: u16 = 0x0125;
    pub const STATUS: u16 = 0x0126;
    pub const RAM_WRITE_AREA: u16 = 0x0128;
    pub const RAM_WRITE: u16 = 0x0129;
    pub const RAM_READ_AREA: u16 = 0x012A;
    pub const RAM_READ: u16 = 0x012B;
    pub const EXECUTE: u16 = 0x012C;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    Link(String),
    Header,
    HeaderCrc,
    ReplyCommand { expected: u16, got: u16 },
    Length { expected: usize, got: usize },
    PayloadCrc,
    Truncated,
    ProgramStatus(u8),
    RamReadback,
    ReadsDiffer,
    Range,
    Chip(u32),
    Flash([u8; 3]),
    WritesLocked,
}

pub trait Link {
    fn transfer(&mut self, packet: &[u8]) -> Result<Vec<u8>, Error>;
}

pub fn encode(command: u16, flags: u8, payload: &[u8]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(PACKET);
    frame.extend_from_slice(b"BUXX");
    frame.extend_from_slice(&command.to_le_bytes());
    frame.push(0);
    frame.push(flags);
    frame.extend_from_slice(&(payload.len() as u16).to_le_bytes());
    let header_crc = crc32(&frame);
    frame.extend_from_slice(&header_crc.to_le_bytes());
    if !payload.is_empty() {
        frame.extend_from_slice(payload);
        frame.extend_from_slice(&crc32(payload).to_le_bytes());
    }
    if frame.len() < PACKET {
        frame.resize(PACKET, 0);
    }
    frame
}

pub fn reply_code(command: u16) -> u16 {
    (command & 0x00FF) | 0x0200
}

pub fn decode(packet: &[u8], reply: u16, expected: Option<usize>) -> Result<Vec<u8>, Error> {
    if packet.len() < HEADER || &packet[..4] != b"BUXX" {
        return Err(Error::Header);
    }
    if get32(packet, 10) != crc32(&packet[..10]) {
        return Err(Error::HeaderCrc);
    }
    let command = u16::from_le_bytes([packet[4], packet[5]]);
    if command != reply || packet[6] != 0 {
        return Err(Error::ReplyCommand { expected: reply, got: command });
    }
    let length = usize::from(u16::from_le_bytes([packet[8], packet[9]]));
    if let Some(want) = expected
        && want != length
    {
        return Err(Error::Length { expected: want, got: length });
    }
    let end = HEADER + length;
    if packet.len() < end + if length > 0 { 4 } else { 0 } {
        return Err(Error::Truncated);
    }
    let payload = packet[HEADER..end].to_vec();
    if length > 0 && get32(packet, end) != crc32(&payload) {
        return Err(Error::PayloadCrc);
    }
    Ok(payload)
}

fn area(address: u32, length: u32, chunk: u32) -> [u8; 16] {
    let mut out = [0u8; 16];
    out[0..4].copy_from_slice(&address.to_le_bytes());
    out[4..8].copy_from_slice(&length.to_le_bytes());
    out[8..12].copy_from_slice(&chunk.to_le_bytes());
    out
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Identity {
    pub chip_id: u32,
    pub jedec: [u8; 3],
    pub status: u32,
}

impl Identity {
    pub fn block_protect_bits(&self) -> u8 {
        ((self.status >> 2) & 0x1F) as u8
    }

    pub fn capacity(&self) -> Option<u32> {
        (16..=26).contains(&self.jedec[2]).then(|| 1u32 << self.jedec[2])
    }
}

pub struct Brom<L> {
    link: L,
    writes_unlocked: bool,
    greeted: bool,
}

impl<L: Link> Brom<L> {
    pub fn new(link: L) -> Self {
        Self { link, writes_unlocked: false, greeted: false }
    }

    pub fn into_link(self) -> L {
        self.link
    }

    pub fn unlock_flash_writes(&mut self) {
        self.writes_unlocked = true;
    }

    fn call(&mut self, command: u16, flags: u8, payload: &[u8], expected: Option<usize>) -> Result<Vec<u8>, Error> {
        if !self.greeted && command != cmd::HANDSHAKE {
            self.handshake()?;
        }
        self.exchange(command, flags, payload, expected)
    }

    fn exchange(&mut self, command: u16, flags: u8, payload: &[u8], expected: Option<usize>) -> Result<Vec<u8>, Error> {
        let reply = self.link.transfer(&encode(command, flags, payload))?;
        decode(&reply, reply_code(command), expected)
    }

    pub fn handshake(&mut self) -> Result<u32, Error> {
        let info = self.exchange(cmd::HANDSHAKE, 1, &[], Some(HANDSHAKE_LEN))?;
        self.greeted = true;
        Ok(get32(&info, 40))
    }

    pub fn identify(&mut self) -> Result<Identity, Error> {
        let chip_id = self.handshake()?;
        let jedec = self.call(cmd::JEDEC, 0, &[], Some(4))?;
        let status = self.call(cmd::STATUS, 0, &[], Some(4))?;
        let id = get32(&jedec, 0);
        Ok(Identity { chip_id, jedec: [(id >> 16) as u8, (id >> 8) as u8, id as u8], status: get32(&status, 0) })
    }

    pub fn require_chip(&mut self) -> Result<Identity, Error> {
        let identity = self.identify()?;
        if identity.chip_id != EXPECTED_CHIP {
            return Err(Error::Chip(identity.chip_id));
        }
        if identity.jedec[0] != FLASH_MANUFACTURER || identity.capacity() != Some(FLASH_SIZE) {
            return Err(Error::Flash(identity.jedec));
        }
        Ok(identity)
    }

    fn flash_range(offset: u32, len: u32) -> Result<(), Error> {
        if len == 0 || u64::from(offset) + u64::from(len) > u64::from(FLASH_SIZE) {
            return Err(Error::Range);
        }
        Ok(())
    }

    pub fn read_flash(&mut self, offset: u32, out: &mut [u8]) -> Result<(), Error> {
        Self::flash_range(offset, out.len() as u32)?;
        self.call(cmd::FLASH_AREA, 0, &area(FLASH_ALIAS + offset, out.len() as u32, CHUNK), Some(0))?;
        for chunk in out.chunks_mut(CHUNK as usize) {
            let data = self.call(cmd::FLASH_READ, 0, &[], Some(chunk.len()))?;
            chunk.copy_from_slice(&data);
        }
        Ok(())
    }

    pub fn erase_flash(&mut self, offset: u32, len: u32) -> Result<(), Error> {
        if !self.writes_unlocked {
            return Err(Error::WritesLocked);
        }
        Self::flash_range(offset, len)?;
        if !offset.is_multiple_of(SECTOR) || !len.is_multiple_of(SECTOR) {
            return Err(Error::Range);
        }
        self.call(cmd::FLASH_ERASE, 0, &area(FLASH_ALIAS + offset, len, CHUNK), Some(0))?;
        Ok(())
    }

    pub fn program_flash(&mut self, offset: u32, data: &[u8]) -> Result<(), Error> {
        if !self.writes_unlocked {
            return Err(Error::WritesLocked);
        }
        Self::flash_range(offset, data.len() as u32)?;
        self.call(cmd::FLASH_AREA, 0, &area(FLASH_ALIAS + offset, data.len() as u32, CHUNK), Some(0))?;
        for chunk in data.chunks(CHUNK as usize) {
            let status = self.call(cmd::FLASH_PROGRAM, 0, chunk, Some(1))?;
            if status[0] != 0 {
                return Err(Error::ProgramStatus(status[0]));
            }
        }
        Ok(())
    }

    pub fn write_ram(&mut self, address: u32, data: &[u8]) -> Result<(), Error> {
        self.call(cmd::RAM_WRITE_AREA, 0, &area(address, data.len() as u32, CHUNK), Some(0))?;
        for chunk in data.chunks(CHUNK as usize) {
            let status = self.call(cmd::RAM_WRITE, 0, chunk, Some(1))?;
            if status[0] != 0 {
                return Err(Error::ProgramStatus(status[0]));
            }
        }
        Ok(())
    }

    pub fn read_ram(&mut self, address: u32, out: &mut [u8]) -> Result<(), Error> {
        self.call(cmd::RAM_READ_AREA, 0, &area(address, out.len() as u32, CHUNK), Some(0))?;
        for chunk in out.chunks_mut(CHUNK as usize) {
            let data = self.call(cmd::RAM_READ, 0, &[], Some(chunk.len()))?;
            chunk.copy_from_slice(&data);
        }
        Ok(())
    }

    pub fn execute(&mut self, entry: u32) -> Result<(), Error> {
        self.call(cmd::EXECUTE, 0, &entry.to_le_bytes(), Some(0))?;
        Ok(())
    }

    pub fn load_and_run(&mut self, address: u32, image: &[u8]) -> Result<(), Error> {
        self.write_ram(address, image)?;
        let mut check = vec![0u8; image.len()];
        self.read_ram(address, &mut check)?;
        if check != image {
            return Err(Error::RamReadback);
        }
        self.execute(address)
    }

    pub fn reboot(&mut self) -> Result<(), Error> {
        self.call(cmd::REBOOT, 0, &[], Some(0))?;
        self.greeted = false;
        Ok(())
    }

    pub fn backup(&mut self, len: u32, mut progress: impl FnMut(u32)) -> Result<Vec<u8>, Error> {
        let mut first = vec![0u8; len as usize];
        let mut second = vec![0u8; len as usize];
        for (pass, buffer) in [&mut first, &mut second].into_iter().enumerate() {
            for (i, part) in buffer.chunks_mut(0x1_0000).enumerate() {
                let offset = i as u32 * 0x1_0000;
                self.read_flash(offset, part)?;
                progress(pass as u32 * len + offset + part.len() as u32);
            }
        }
        if first != second {
            return Err(Error::ReadsDiffer);
        }
        Ok(first)
    }
}

impl<L: Link> FlashTarget for Brom<L> {
    fn read(&mut self, offset: u32, buf: &mut [u8]) -> Result<(), TargetError> {
        self.read_flash(offset, buf).map_err(to_target)
    }

    fn erase(&mut self, offset: u32, len: u32) -> Result<(), TargetError> {
        self.erase_flash(offset, len).map_err(to_target)
    }

    fn program(&mut self, offset: u32, data: &[u8]) -> Result<(), TargetError> {
        self.program_flash(offset, data).map_err(to_target)
    }
}

fn to_target(error: Error) -> TargetError {
    match error {
        Error::ProgramStatus(_) | Error::RamReadback => TargetError::ProgramVerify,
        _ => TargetError::Transport,
    }
}

pub mod sim {
    use super::*;

    pub struct SimBrom {
        pub flash: Vec<u8>,
        pub ram: Vec<u8>,
        pub chip_id: u32,
        pub jedec_reply: [u8; 4],
        address: u32,
        remaining: u32,
        chunk: u32,
        pub executed: Vec<u32>,
        pub reboots: u32,
        pub erases: u32,
        pub handshaken: bool,
    }

    impl SimBrom {
        pub fn new(flash: &[u8]) -> Self {
            let mut data = vec![0xFF; FLASH_SIZE as usize];
            data[..flash.len()].copy_from_slice(flash);
            Self {
                flash: data,
                ram: vec![0; 0x20_0000],
                chip_id: EXPECTED_CHIP,
                jedec_reply: [0x16, 0x65, 0xC8, 0x00],
                address: 0,
                remaining: 0,
                chunk: 0,
                executed: Vec::new(),
                reboots: 0,
                erases: 0,
                handshaken: false,
            }
        }

        fn reply(command: u16, payload: &[u8]) -> Vec<u8> {
            encode(reply_code(command), 0, payload)
        }

        fn set_area(&mut self, payload: &[u8]) {
            self.address = get32(payload, 0);
            self.remaining = get32(payload, 4);
            self.chunk = get32(payload, 8);
        }

        fn spi(address: u32) -> usize {
            (address & 0x00FF_FFFF) as usize
        }
    }

    impl Link for SimBrom {
        fn transfer(&mut self, packet: &[u8]) -> Result<Vec<u8>, Error> {
            if packet.len() != PACKET || &packet[..4] != b"BUXX" || get32(packet, 10) != crc32(&packet[..10]) {
                return Err(Error::Link("malformed request".into()));
            }
            let command = u16::from_le_bytes([packet[4], packet[5]]);
            let length = usize::from(u16::from_le_bytes([packet[8], packet[9]]));
            let payload = &packet[HEADER..HEADER + length];
            if length > 0 && get32(packet, HEADER + length) != crc32(payload) {
                return Err(Error::Link("payload crc".into()));
            }
            if command != cmd::HANDSHAKE && !self.handshaken {
                return Err(Error::Link("no reply before the handshake".into()));
            }
            Ok(match command {
                cmd::HANDSHAKE => {
                    self.handshaken = true;
                    let mut info = [0u8; HANDSHAKE_LEN];
                    info[40..44].copy_from_slice(&self.chip_id.to_le_bytes());
                    Self::reply(command, &info)
                }
                cmd::JEDEC => Self::reply(command, &self.jedec_reply),
                cmd::STATUS => Self::reply(command, &[0, 0, 0, 0]),
                cmd::FLASH_AREA | cmd::RAM_WRITE_AREA | cmd::RAM_READ_AREA => {
                    self.set_area(payload);
                    Self::reply(command, &[])
                }
                cmd::FLASH_ERASE => {
                    self.set_area(payload);
                    let start = Self::spi(self.address) & !(SECTOR as usize - 1);
                    let end = (Self::spi(self.address) + self.remaining as usize).div_ceil(SECTOR as usize) * SECTOR as usize;
                    self.flash[start..end.min(FLASH_SIZE as usize)].fill(0xFF);
                    self.erases += 1;
                    Self::reply(command, &[])
                }
                cmd::FLASH_PROGRAM => {
                    if self.remaining == 0 {
                        return Ok(Self::reply(command, &[2]));
                    }
                    let n = payload.len().min(self.remaining as usize);
                    let at = Self::spi(self.address);
                    for (i, byte) in payload[..n].iter().enumerate() {
                        self.flash[at + i] &= *byte;
                    }
                    let ok = self.flash[at..at + n] == payload[..n];
                    self.address += n as u32;
                    self.remaining -= n as u32;
                    Self::reply(command, &[u8::from(!ok)])
                }
                cmd::FLASH_READ => {
                    let n = (self.chunk.min(self.remaining)) as usize;
                    let at = Self::spi(self.address);
                    let data = self.flash[at..at + n].to_vec();
                    self.address += n as u32;
                    self.remaining -= n as u32;
                    Self::reply(command, &data)
                }
                cmd::RAM_WRITE => {
                    if self.remaining == 0 {
                        return Ok(Self::reply(command, &[2]));
                    }
                    let n = payload.len().min(self.remaining as usize);
                    let at = self.address as usize;
                    self.ram[at..at + n].copy_from_slice(&payload[..n]);
                    self.address += n as u32;
                    self.remaining -= n as u32;
                    Self::reply(command, &[0])
                }
                cmd::RAM_READ => {
                    let n = (self.chunk.min(self.remaining)) as usize;
                    let at = self.address as usize;
                    let data = self.ram[at..at + n].to_vec();
                    self.address += n as u32;
                    self.remaining -= n as u32;
                    Self::reply(command, &data)
                }
                cmd::EXECUTE => {
                    self.executed.push(get32(payload, 0));
                    Self::reply(command, &[])
                }
                cmd::REBOOT => {
                    self.reboots += 1;
                    self.handshaken = false;
                    Self::reply(command, &[])
                }
                _ => return Err(Error::Link(format!("unsupported command {command:04x}"))),
            })
        }
    }
}
