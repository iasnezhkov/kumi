// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
mod support;

use kumi_client::kumi_icd::{KeyAction, KeymapEntry};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let support::Options { mut case, args } = support::connect()?;
    if args.first().map(String::as_str) == Some("clear") {
        case.clear_keymap()?;
        println!("every remapping removed");
        return Ok(());
    }
    let source = args.first().map(|text| support::number(text)).transpose()?.unwrap_or(0x04);
    let target = args.get(1).map(|text| support::number(text)).transpose()?.unwrap_or(0x05);
    let entry = KeymapEntry { layer: 0, source: u8::try_from(source)?, action: KeyAction::Key(u8::try_from(target)?) };
    if !entry.is_valid() {
        return Err(format!("{entry:?} is outside the protocol limits").into());
    }
    case.set_key(entry)?;
    for mapped in case.keymap_layers()? {
        println!("layer {} key {:#04x} -> {:?}", mapped.layer + 1, mapped.source, mapped.action);
    }
    let info = case.device_info()?;
    if !info.capabilities.contains(kumi_client::kumi_icd::Capabilities::PERSISTENT_SETTINGS) {
        println!("this firmware keeps settings in RAM only: the remapping lasts until the case restarts");
    }
    Ok(())
}
