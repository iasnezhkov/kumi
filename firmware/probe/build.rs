// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
fn main() {
    let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    println!("cargo:rustc-link-search={dir}");
    println!("cargo:rustc-link-arg=-Tprobe.x");
    println!("cargo:rerun-if-changed=probe.x");
}
