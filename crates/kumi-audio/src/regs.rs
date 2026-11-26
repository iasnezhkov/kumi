// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

pub trait Registers {
    fn read(&mut self, address: u32) -> u32;
    fn write(&mut self, address: u32, value: u32);
    fn delay_us(&mut self, microseconds: u32);

    fn update(&mut self, address: u32, clear: u32, set: u32) {
        let value = self.read(address);
        self.write(address, (value & !clear) | set);
    }

    fn write_pair(&mut self, first: (u32, u32), second: (u32, u32)) {
        self.write(first.0, first.1);
        self.write(second.0, second.1);
    }

    fn read_pair(&mut self, first: u32, second: u32) -> (u32, u32) {
        let a = self.read(first);
        (a, self.read(second))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MmioOp {
    Write { address: u32, value: u32 },
    Update { address: u32, clear: u32, set: u32 },
    Delay { microseconds: u32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Expect {
    pub address: u32,
    pub mask: u32,
    pub value: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mismatch {
    pub address: u32,
    pub expected: u32,
    pub actual: u32,
}

pub fn apply<R: Registers>(registers: &mut R, ops: &[MmioOp]) {
    for op in ops {
        apply_one(registers, *op);
    }
}

pub fn apply_one<R: Registers>(registers: &mut R, op: MmioOp) {
    match op {
        MmioOp::Write { address, value } => registers.write(address, value),
        MmioOp::Update { address, clear, set } => registers.update(address, clear, set),
        MmioOp::Delay { microseconds } => registers.delay_us(microseconds),
    }
}

pub fn verify<R: Registers>(registers: &mut R, expectations: &[Expect]) -> Result<(), Mismatch> {
    for expect in expectations {
        let actual = registers.read(expect.address) & expect.mask;
        if actual != expect.value {
            return Err(Mismatch { address: expect.address, expected: expect.value, actual });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{Access, Recorder};

    #[test]
    fn update_is_a_read_modify_write_and_ops_replay_in_order() {
        let mut r = Recorder::default();
        r.set(0x10, 0xf0f0);
        apply(
            &mut r,
            &[
                MmioOp::Update { address: 0x10, clear: 0xf000, set: 0x5 },
                MmioOp::Delay { microseconds: 7 },
                MmioOp::Write { address: 0x14, value: 9 },
            ],
        );
        assert_eq!(r.get(0x10), 0x00f5);
        assert_eq!(r.log, [Access::Read(0x10, 0xf0f0), Access::Write(0x10, 0x00f5), Access::Delay(7), Access::Write(0x14, 9)]);
        assert_eq!(verify(&mut r, &[Expect { address: 0x10, mask: 0xff, value: 0xf5 }]), Ok(()));
        assert_eq!(
            verify(&mut r, &[Expect { address: 0x14, mask: 0xf, value: 8 }]),
            Err(Mismatch { address: 0x14, expected: 8, actual: 9 })
        );
    }
}
