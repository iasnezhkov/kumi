// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
mod support;

use kumi_client::kumi_icd::CaseError;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let support::Options { mut case, args } = support::connect()?;
    let calibrate = args.first().map(String::as_str) == Some("calibrate");
    case.dev_unlock(60)?;
    let result = if calibrate { case.audio_bench_calibrate() } else { case.audio_bench_status() };
    match result {
        Ok(bench) => {
            println!("bench {:?}, CPU {:?} Hz", bench.state, bench.cpu_hz);
            if let Some(calibration) = bench.calibration {
                println!(
                    "frame clock {:?} Hz, trims {:?}, windows {:?}, {} sweep counts",
                    calibration.frame_clock_hz,
                    calibration.trims,
                    calibration.windows,
                    calibration.counts.len()
                );
            }
            if let Some(stream) = bench.stream {
                println!("stream {} Hz on {} DAC(s), ring at {:#010x}", stream.start.rate_hz, stream.dacs, stream.ring_address);
            }
            if let Some(failure) = bench.failure {
                println!("last failure in {:?}: {:?}", failure.phase, failure.fault);
            }
        }
        Err(error) if error.is_unsupported() => println!("this firmware has no audio bench"),
        Err(error) if error.case_error() == Some(CaseError::WrongState) => {
            println!("the bench runs only in a runtime loaded into RAM by a host, after `calibrate`")
        }
        Err(error) => return Err(error.into()),
    }
    Ok(())
}
