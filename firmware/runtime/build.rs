// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
use std::{env, fs, path::Path};

use kumi_layout::{RT40_WINDOW, RUNTIME_DLM, RUNTIME_ENTRY, RUNTIME_ILM, RUNTIME_MIN_STACK, RUNTIME_TRAP_STACK};

fn main() {
    let out = env::var("OUT_DIR").unwrap();
    println!("cargo:rustc-link-search={}", env::var("CARGO_MANIFEST_DIR").unwrap());
    println!("cargo:rustc-link-search={out}");
    println!("cargo:rustc-link-arg=-Truntime.x");
    println!("cargo:rerun-if-changed=runtime.x");
    println!("cargo:rerun-if-env-changed=KUMI_BUILD_NAME");
    let memory = format!(
        "MEMORY {{\n  ILM (rx) : ORIGIN = {:#x}, LENGTH = {:#x}\n  DLM (rwx) : ORIGIN = {:#x}, LENGTH = {:#x}\n  RT40 (rwx) : ORIGIN = {:#x}, LENGTH = {:#x}\n}}\n\
         KUMI_RUNTIME_ENTRY = {:#x};\nKUMI_RUNTIME_TRAP_STACK = {:#x};\nKUMI_RUNTIME_MIN_STACK = {:#x};\n",
        RUNTIME_ILM.start,
        RUNTIME_ILM.end - RUNTIME_ILM.start,
        RUNTIME_DLM.start,
        RUNTIME_DLM.end - RUNTIME_DLM.start,
        RT40_WINDOW.start,
        RT40_WINDOW.end - RT40_WINDOW.start,
        RUNTIME_ENTRY,
        RUNTIME_TRAP_STACK,
        RUNTIME_MIN_STACK,
    );
    fs::write(Path::new(&out).join("runtime-memory.x"), memory).unwrap();
}
