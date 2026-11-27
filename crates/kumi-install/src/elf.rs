// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
const MAGIC: [u8; 4] = *b"\x7fELF";
const CLASS_32: u8 = 1;
const LITTLE_ENDIAN: u8 = 1;
const MACHINE_RISCV: u16 = 243;
const PT_LOAD: u32 = 1;
const HEADER_LEN: usize = 52;
const PROGRAM_HEADER_LEN: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ElfError {
    NotElf,
    NotRiscv32LittleEndian,
    Truncated,
    SegmentOutsideFile,
    MemoryShorterThanFile,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadSegment {
    pub vaddr: u32,
    pub paddr: u32,
    pub data: Vec<u8>,
    pub mem_size: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Elf {
    pub entry: u32,
    pub segments: Vec<LoadSegment>,
}

fn u16_at(bytes: &[u8], at: usize) -> Result<u16, ElfError> {
    let b = bytes.get(at..at + 2).ok_or(ElfError::Truncated)?;
    Ok(u16::from_le_bytes([b[0], b[1]]))
}

fn u32_at(bytes: &[u8], at: usize) -> Result<u32, ElfError> {
    let b = bytes.get(at..at + 4).ok_or(ElfError::Truncated)?;
    Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

pub fn parse(bytes: &[u8]) -> Result<Elf, ElfError> {
    if bytes.len() < HEADER_LEN || bytes[..4] != MAGIC {
        return Err(ElfError::NotElf);
    }
    if bytes[4] != CLASS_32 || bytes[5] != LITTLE_ENDIAN || u16_at(bytes, 18)? != MACHINE_RISCV {
        return Err(ElfError::NotRiscv32LittleEndian);
    }
    let entry = u32_at(bytes, 24)?;
    let table = u32_at(bytes, 28)? as usize;
    let entry_size = usize::from(u16_at(bytes, 42)?);
    let count = usize::from(u16_at(bytes, 44)?);
    if entry_size < PROGRAM_HEADER_LEN && count > 0 {
        return Err(ElfError::Truncated);
    }
    let mut segments = Vec::new();
    for i in 0..count {
        let at = table.checked_add(i * entry_size).ok_or(ElfError::Truncated)?;
        if u32_at(bytes, at)? != PT_LOAD {
            continue;
        }
        let offset = u32_at(bytes, at + 4)? as usize;
        let vaddr = u32_at(bytes, at + 8)?;
        let paddr = u32_at(bytes, at + 12)?;
        let file_size = u32_at(bytes, at + 16)? as usize;
        let mem_size = u32_at(bytes, at + 20)?;
        if (mem_size as usize) < file_size {
            return Err(ElfError::MemoryShorterThanFile);
        }
        if mem_size == 0 {
            continue;
        }
        let data = bytes.get(offset..offset.checked_add(file_size).ok_or(ElfError::SegmentOutsideFile)?);
        let data = data.ok_or(ElfError::SegmentOutsideFile)?.to_vec();
        segments.push(LoadSegment { vaddr, paddr, data, mem_size });
    }
    Ok(Elf { entry, segments })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlattenError {
    OutsideRange { paddr: u32 },
    Overlap { paddr: u32 },
}

pub fn flatten_by_load_address(elf: &Elf, base: u32, limit: u32) -> Result<Vec<u8>, FlattenError> {
    let mut placed: Vec<(u32, &[u8])> =
        elf.segments.iter().filter(|s| !s.data.is_empty()).map(|s| (s.paddr, s.data.as_slice())).collect();
    placed.sort_by_key(|(paddr, _)| *paddr);
    let mut image = Vec::new();
    for (paddr, data) in placed {
        let start = paddr.checked_sub(base).ok_or(FlattenError::OutsideRange { paddr })? as usize;
        let end = start + data.len();
        if end > limit as usize {
            return Err(FlattenError::OutsideRange { paddr });
        }
        if start < image.len() {
            return Err(FlattenError::Overlap { paddr });
        }
        image.resize(start, 0);
        image.extend_from_slice(data);
    }
    Ok(image)
}

pub fn has_symbol(bytes: &[u8], name: &str) -> bool {
    const SHT_SYMTAB: u32 = 2;
    const SHDR_LEN: usize = 40;
    const SYM_LEN: usize = 16;
    if bytes.len() < HEADER_LEN || bytes[..4] != MAGIC || bytes[4] != CLASS_32 || bytes[5] != LITTLE_ENDIAN {
        return false;
    }
    let Ok(shoff) = u32_at(bytes, 32).map(|v| v as usize) else { return false };
    let Ok(shentsize) = u16_at(bytes, 46).map(usize::from) else { return false };
    let Ok(shnum) = u16_at(bytes, 48).map(usize::from) else { return false };
    if shoff == 0 || shentsize < SHDR_LEN || shnum == 0 {
        return false;
    }
    let target = name.as_bytes();
    for i in 0..shnum {
        let Some(at) = shoff.checked_add(i * shentsize) else { return false };
        let Ok(sh_type) = u32_at(bytes, at + 4) else { continue };
        if sh_type != SHT_SYMTAB {
            continue;
        }
        let Ok(sym_off) = u32_at(bytes, at + 16).map(|v| v as usize) else { continue };
        let Ok(sym_size) = u32_at(bytes, at + 20).map(|v| v as usize) else { continue };
        let Ok(str_idx) = u32_at(bytes, at + 24).map(|v| v as usize) else { continue };
        let Ok(entsize) = u32_at(bytes, at + 36).map(|v| v as usize) else { continue };
        if entsize < SYM_LEN || str_idx >= shnum {
            continue;
        }
        let Some(str_hdr) = shoff.checked_add(str_idx * shentsize) else { continue };
        let Ok(str_off) = u32_at(bytes, str_hdr + 16).map(|v| v as usize) else { continue };
        let Ok(str_size) = u32_at(bytes, str_hdr + 20).map(|v| v as usize) else { continue };
        let Some(strtab) = bytes.get(str_off..str_off.saturating_add(str_size)) else { continue };
        let mut cursor = 0;
        while cursor + SYM_LEN <= sym_size {
            let Ok(st_name) = u32_at(bytes, sym_off + cursor).map(|v| v as usize) else { break };
            if st_name < strtab.len() {
                let tail = &strtab[st_name..];
                let end = tail.iter().position(|&b| b == 0).unwrap_or(tail.len());
                if &tail[..end] == target {
                    return true;
                }
            }
            cursor += entsize;
        }
    }
    false
}
