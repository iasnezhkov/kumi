// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
mod support;

const CHIP_ID: u32 = 0x2000_0000;
const DAC_BUS: u8 = 0;
const PROBED: [u8; 2] = [0x31, 0x33];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut case = support::connect()?.case;
    if let Some(virtual_case) = support::virtual_case(&mut case) {
        virtual_case.add_i2c_device(DAC_BUS, 0x33);
    }
    case.dev_unlock(60)?;
    let block = case.dev_peek(CHIP_ID, 1)?;
    println!("chip id register {CHIP_ID:#010x}: {:#010x}", block.words[0]);
    for address in PROBED {
        match case.dev_i2c(DAC_BUS, address, &[], 0) {
            Ok(reply) => println!("I2C{DAC_BUS} {address:#04x}: {}", if reply.acked { "ACK" } else { "NACK" }),
            Err(error) if error.case_error() == Some(kumi_client::kumi_icd::CaseError::WrongState) => {
                println!("I2C needs a runtime loaded into RAM by a host");
                break;
            }
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}
