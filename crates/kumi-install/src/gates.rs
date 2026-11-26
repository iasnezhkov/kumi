// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gate {
    G3Emulator,
    G4ResetRetention,
    G2bWritePath,
}

pub const REQUIRED_FOR_INSTALL: [Gate; 3] = [Gate::G3Emulator, Gate::G4ResetRetention, Gate::G2bWritePath];

pub const G3_EMULATOR_EVIDENCE: &str = "tools/brom_emu.py recovery matrix on the boot ROM dump with sha256 \
     521f2161774f1ec555440a5ba41c208a3ac94f11f80bdf8d5db0b6e0e856a916: an erased header sector and a zeroed SPV@ \
     magic both reach the service-mode USB handler, device descriptor 0666:0888, with no flash write";

pub const G4_HARDWARE_EVIDENCE: &str = "tools/g4.py on the case 2026-11-05: the kumi_chip::reset register sequence \
     (0x2100801C bit 31 set, then 0x20003024 bit 1) from boot ROM service mode came back as 0666:0888 after 5.51 s; \
     RAM 0x137D00..0x138000 kept, ILM marker kept; stock-style reset: service_mode; power cut: stock; boot ROM \
     descriptor sha256 8be4b8ef57904b71332cb5a5cc8dee850cd2aaf13695fd5459332e3e69712e19, probe image sha256 \
     2de14be59afec0d90245492c94f622162c7378a8288aa11e31243e974d89c134";

pub const G2B_HARDWARE_EVIDENCE: &str = "tools/stage0_dry.py g2b on the case 2026-11-05: stage0's write session from RAM \
     through the boot ROM routines passed (outcome 0, steps 0x7F), erased, programmed and erased 0x3F0000 in 174.5 ms, \
     flash head CRC 0xa8ff5a7f before and after through the ROM read routine; stage0.bin sha256 \
     fc9ebc0d6f8a6a07e84f2e20c44f0fb0f7e209ce0145fe7798778e438eff8f96. The probe bisect (tools/g2b_bisect.py) ran \
     all eight helper steps; reading the 0x48000000 window hangs in service mode, so the test head check uses the ROM";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GateRecord {
    pub gate: Gate,
    pub evidence: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GateEvidence {
    records: Vec<GateRecord>,
}

impl GateEvidence {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with(mut self, gate: Gate, evidence: &str) -> Self {
        if !evidence.trim().is_empty() {
            self.records.retain(|record| record.gate != gate);
            self.records.push(GateRecord { gate, evidence: evidence.to_string() });
        }
        self
    }

    pub fn record(&self, gate: Gate) -> Option<&GateRecord> {
        self.records.iter().find(|record| record.gate == gate)
    }

    pub fn missing(&self, required: &[Gate]) -> Option<Gate> {
        required.iter().copied().find(|gate| self.record(*gate).is_none())
    }

    pub fn recorded() -> Self {
        Self::new()
            .with(Gate::G3Emulator, G3_EMULATOR_EVIDENCE)
            .with(Gate::G4ResetRetention, G4_HARDWARE_EVIDENCE)
            .with(Gate::G2bWritePath, G2B_HARDWARE_EVIDENCE)
    }

    pub fn for_simulation() -> Self {
        REQUIRED_FOR_INSTALL.iter().fold(Self::new(), |evidence, gate| evidence.with(*gate, "simulated case"))
    }
}
