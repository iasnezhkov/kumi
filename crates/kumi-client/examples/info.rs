// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
mod support;

use kumi_client::kumi_icd::{Capabilities, stage};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut case = support::connect()?.case;
    let info = case.device_info()?;
    println!(
        "Kumi {}.{}.{} ({}), protocol {}",
        info.firmware.major, info.firmware.minor, info.firmware.patch, info.build, info.protocol
    );
    println!("chip {:#010x}, flash {:02x?}, running from RAM: {}", info.chip_id, info.flash_jedec, info.running_from_ram);
    let names: Vec<&str> =
        Capabilities::NAMES.iter().filter(|(_, bit)| info.capabilities.bits() & bit != 0).map(|(name, _)| *name).collect();
    println!("capabilities: {}", names.join(", "));
    match case.diagnostics() {
        Ok(diagnostics) => {
            let at = stage::name(diagnostics.stage).unwrap_or("unknown stage");
            println!(
                "boot {} at {at}, up {} ms, safe mode {}",
                diagnostics.boot_count, diagnostics.uptime_ms, diagnostics.safe_mode
            );
            match diagnostics.last_crash {
                Some(crash) => println!(
                    "last crash: {:?} at {}, mcause {:#x} mepc {:#010x}, {}",
                    crash.reason,
                    stage::name(crash.stage).unwrap_or("unknown stage"),
                    crash.mcause,
                    crash.mepc,
                    crash.message
                ),
                None => println!("no crash recorded"),
            }
        }
        Err(error) if error.is_unsupported() || error.is_unknown_endpoint() => println!("this firmware has no diagnostics"),
        Err(error) => return Err(error.into()),
    }
    let keyboard = case.keyboard_status()?;
    println!("keyboard link up: {}, frames {}, layer {}", keyboard.link_up, keyboard.frames, keyboard.active_layer + 1);
    let slots = case.slots()?;
    match slots.booted {
        Some(slot) => println!("booted from slot {slot:?}"),
        None => println!("not booted from a flash slot (loaded into RAM by a host)"),
    }
    Ok(())
}
