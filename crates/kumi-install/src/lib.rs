// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
pub mod bundle;
pub mod elf;
pub mod gates;
pub mod pack;
pub mod plan;
pub mod sample;
pub mod spv;

pub use gates::{Gate, GateEvidence, GateRecord, REQUIRED_FOR_INSTALL};
pub use kumi_layout::{FlashTarget, TargetError};
pub use plan::{BootAfter, Op, Plan, PlanError, PlanKind, RestoreMode, Step, plan_install, plan_restore_stock, sha256};

use kumi_layout::{FLASH_SIZE, SECTOR};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecError {
    Target { step: usize, error: TargetError },
    Readback { step: usize },
    NotErased { step: usize },
    VerifyFailed { step: usize },
    Range { step: usize },
}

impl ExecError {
    pub fn step(&self) -> usize {
        match *self {
            ExecError::Target { step, .. }
            | ExecError::Readback { step }
            | ExecError::NotErased { step }
            | ExecError::VerifyFailed { step }
            | ExecError::Range { step } => step,
        }
    }
}

const READ_CHUNK: usize = 4096;

fn in_flash(offset: u32, len: u32) -> bool {
    u64::from(offset) + u64::from(len) <= u64::from(FLASH_SIZE)
}

pub fn execute<T: FlashTarget>(
    plan: &Plan,
    target: &mut T,
    from: usize,
    mut progress: impl FnMut(usize, &Step),
) -> Result<(), ExecError> {
    for (index, step) in plan.steps.iter().enumerate().skip(from) {
        progress(index, step);
        let err = |error| ExecError::Target { step: index, error };
        match &step.op {
            Op::Erase { offset, len } => {
                if !in_flash(*offset, *len) || offset % SECTOR != 0 || len % SECTOR != 0 {
                    return Err(ExecError::Range { step: index });
                }
                target.erase(*offset, *len).map_err(err)?;
                if *len <= 0x1_0000 {
                    let mut buf = vec![0u8; *len as usize];
                    target.read(*offset, &mut buf).map_err(err)?;
                    if buf.iter().any(|b| *b != 0xFF) {
                        return Err(ExecError::NotErased { step: index });
                    }
                }
            }
            Op::Program { offset, data } => {
                if !in_flash(*offset, data.len() as u32) {
                    return Err(ExecError::Range { step: index });
                }
                target.program(*offset, data).map_err(err)?;
                let mut buf = vec![0u8; data.len()];
                target.read(*offset, &mut buf).map_err(err)?;
                if buf != *data {
                    return Err(ExecError::Readback { step: index });
                }
            }
            Op::Verify { offset, len, sha256 } => {
                if !in_flash(*offset, *len) {
                    return Err(ExecError::Range { step: index });
                }
                let mut hasher = <sha2::Sha256 as sha2::Digest>::new();
                let mut done = 0u32;
                let mut buf = vec![0u8; READ_CHUNK];
                while done < *len {
                    let n = READ_CHUNK.min((*len - done) as usize);
                    target.read(offset + done, &mut buf[..n]).map_err(err)?;
                    sha2::Digest::update(&mut hasher, &buf[..n]);
                    done += n as u32;
                }
                let got: [u8; 32] = sha2::Digest::finalize(hasher).into();
                if got != *sha256 {
                    return Err(ExecError::VerifyFailed { step: index });
                }
            }
        }
    }
    Ok(())
}

pub fn resume_point(plan: &Plan, failed_step: usize) -> usize {
    let unit = plan.steps.get(failed_step).map_or(usize::MAX, |s| s.unit);
    plan.first_step_of_unit(unit)
}

pub mod sim {
    use super::{FlashTarget, TargetError};
    use kumi_layout::{FLASH_SIZE, SECTOR};

    pub struct SimFlash {
        pub data: Vec<u8>,
        pub budget: Option<usize>,
        pub operations: usize,
    }

    impl SimFlash {
        pub fn new(contents: &[u8]) -> Self {
            let mut data = vec![0xFF; FLASH_SIZE as usize];
            data[..contents.len()].copy_from_slice(contents);
            Self { data, budget: None, operations: 0 }
        }

        fn spend(&mut self, bytes: usize) -> Result<usize, usize> {
            match self.budget {
                None => Ok(bytes),
                Some(left) if left >= bytes => {
                    self.budget = Some(left - bytes);
                    Ok(bytes)
                }
                Some(left) => {
                    self.budget = Some(0);
                    Err(left)
                }
            }
        }
    }

    impl FlashTarget for SimFlash {
        fn read(&mut self, offset: u32, buf: &mut [u8]) -> Result<(), TargetError> {
            let start = offset as usize;
            buf.copy_from_slice(&self.data[start..start + buf.len()]);
            Ok(())
        }

        fn erase(&mut self, offset: u32, len: u32) -> Result<(), TargetError> {
            self.operations += 1;
            for sector in (offset..offset + len).step_by(SECTOR as usize) {
                let range = sector as usize..(sector + SECTOR) as usize;
                if let Err(partial) = self.spend(SECTOR as usize) {
                    for (i, byte) in self.data[range].iter_mut().enumerate() {
                        *byte = if i < partial { 0xFF } else { byte.rotate_left(3) ^ 0x5A };
                    }
                    return Err(TargetError::PowerLost);
                }
                self.data[range].fill(0xFF);
            }
            Ok(())
        }

        fn program(&mut self, offset: u32, data: &[u8]) -> Result<(), TargetError> {
            self.operations += 1;
            let allowed = match self.spend(data.len()) {
                Ok(n) => n,
                Err(n) => n,
            };
            for (i, byte) in data.iter().enumerate().take(allowed) {
                self.data[offset as usize + i] &= *byte;
            }
            if allowed < data.len() {
                return Err(TargetError::PowerLost);
            }
            Ok(())
        }
    }
}
