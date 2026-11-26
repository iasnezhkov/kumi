// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
use std::{env, fs, path::Path};

use kumi_layout::{
    FLASH_XIP_BASE, SPV_HEADER_RESERVED, STAGE0, STAGE0_BOOT_ENTRY, STAGE0_DRY_ENTRY, STAGE0_G2B_ENTRY, STAGE0_MIN_STACK,
    STAGE0_RAM, STAGE0_RAM_IMAGE_OFFSET, STAGE0_TRAP_STACK, STAGE0_VERIFY_ENTRY, STAGE0_VERIFY_MAGIC_ADDRESS, STAGE0_XIP_ENTRY,
};

fn main() {
    let dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let out = env::var("OUT_DIR").unwrap();
    println!("cargo:rustc-link-search={dir}");
    println!("cargo:rustc-link-search={out}");
    println!("cargo:rustc-link-arg=-Tstage0.x");
    println!("cargo:rerun-if-changed=stage0.x");
    println!("cargo:rerun-if-env-changed=KUMI_PUBLIC_KEY_HEX");
    let memory = format!(
        "MEMORY {{\n  FLASH (rx) : ORIGIN = {:#x}, LENGTH = {:#x}\n  RAM (rwx) : ORIGIN = {:#x}, LENGTH = {:#x}\n}}\n\
         KUMI_SPV_HEADER_RESERVED = {:#x};\nKUMI_STAGE0_XIP_ENTRY = {:#x};\nKUMI_STAGE0_RAM_LMA = {:#x};\n\
         KUMI_STAGE0_BOOT_ENTRY = {:#x};\nKUMI_STAGE0_DRY_ENTRY = {:#x};\nKUMI_STAGE0_VERIFY_ENTRY = {:#x};\n\
         KUMI_STAGE0_VERIFY_MAGIC_ADDRESS = {:#x};\nKUMI_STAGE0_G2B_ENTRY = {:#x};\nKUMI_STAGE0_MIN_STACK = {:#x};\n\
         KUMI_STAGE0_TRAP_STACK = {:#x};\n",
        FLASH_XIP_BASE,
        STAGE0.size,
        STAGE0_RAM.start,
        STAGE0_RAM.end - STAGE0_RAM.start,
        SPV_HEADER_RESERVED,
        STAGE0_XIP_ENTRY,
        FLASH_XIP_BASE + STAGE0_RAM_IMAGE_OFFSET,
        STAGE0_BOOT_ENTRY,
        STAGE0_DRY_ENTRY,
        STAGE0_VERIFY_ENTRY,
        STAGE0_VERIFY_MAGIC_ADDRESS,
        STAGE0_G2B_ENTRY,
        STAGE0_MIN_STACK,
        STAGE0_TRAP_STACK,
    );
    fs::write(Path::new(&out).join("stage0-memory.x"), memory).unwrap();
    let key = match env::var("KUMI_PUBLIC_KEY_HEX") {
        Ok(hex) => parse(hex.trim()),
        Err(_) => {
            println!("cargo:warning=KUMI_PUBLIC_KEY_HEX is not set: this stage0 accepts no image and always enters the boot ROM");
            [0u8; 32]
        }
    };
    let bytes: Vec<String> = key.iter().map(|b| format!("0x{b:02x}")).collect();
    fs::write(Path::new(&out).join("public_key.rs"), format!("[{}]\n", bytes.join(", "))).unwrap();
}

fn parse(hex: &str) -> [u8; 32] {
    assert!(hex.len() == 64 && hex.bytes().all(|c| c.is_ascii_hexdigit()), "KUMI_PUBLIC_KEY_HEX must be 64 hex digits");
    let mut key = [0u8; 32];
    for (i, byte) in key.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).unwrap();
    }
    key
}
