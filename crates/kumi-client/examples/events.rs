// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
mod support;

use std::time::{Duration, Instant};

use kumi_client::Event;
use kumi_client::kumi_icd::EventMask;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let support::Options { mut case, args } = support::connect()?;
    let seconds = args.first().map(|text| text.parse::<u64>()).transpose()?.unwrap_or(10);
    let granted = case.subscribe(EventMask::KEYS | EventMask::LOG)?;
    println!("subscribed to {granted:?}; type on the case for {seconds} s");
    if let Some(virtual_case) = support::virtual_case(&mut case) {
        for key in [0x0B, 0x08, 0x0F, 0x0F, 0x12] {
            virtual_case.tap(key);
        }
    }
    let deadline = Instant::now() + Duration::from_secs(seconds);
    let simulated = support::virtual_case(&mut case).is_some();
    loop {
        for event in case.events(16)? {
            match event {
                Event::Key(key) => println!(
                    "{} key {:#04x} on layer {} -> {:?}",
                    if key.pressed { "press" } else { "release" },
                    key.source,
                    key.layer + 1,
                    key.action
                ),
                Event::Log(line) => println!("log {} ms {:?} {}: {}", line.uptime_ms, line.level, line.module, line.text),
                other => println!("{} {other:?}", other.path()),
            }
        }
        if simulated || Instant::now() >= deadline {
            break;
        }
    }
    case.subscribe(EventMask::NONE)?;
    Ok(())
}
