// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
use ed25519_dalek::SigningKey;
use kumi_install::sim::SimFlash;
use kumi_install::spv::{self, Verdict};
use kumi_install::*;
use kumi_layout::*;

fn fake_stock() -> Vec<u8> {
    sample::stock()
}

fn key() -> SigningKey {
    SigningKey::from_bytes(&[3; 32])
}

fn gates() -> GateEvidence {
    GateEvidence::for_simulation()
}

fn stage0(public: &[u8; 32]) -> Vec<u8> {
    sample::stage0(public)
}

fn kumi_image(signer: &SigningKey) -> Vec<u8> {
    sample::image(signer, [0, 1, 0], 1)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Outcome {
    Stock,
    Kumi,
    Service,
    Unknown,
}

fn outcome(flash: &[u8], stock: &[u8], stage0: &[u8]) -> Outcome {
    match spv::brom_verdict(flash) {
        Verdict::Service => Outcome::Service,
        Verdict::Boots { length, .. } => {
            let image = &flash[..length as usize];
            if image == &stock[..(length as usize).min(stock.len())] {
                Outcome::Stock
            } else if image == stage0 {
                Outcome::Kumi
            } else {
                Outcome::Unknown
            }
        }
    }
}

struct Fixture {
    stock: Vec<u8>,
    full: Vec<u8>,
    stage0: Vec<u8>,
    image: Vec<u8>,
    public: [u8; 32],
}

fn fixture() -> Fixture {
    let signer = key();
    let public = signer.verifying_key().to_bytes();
    let stock = fake_stock();
    let mut full = vec![0xFF; FLASH_SIZE as usize];
    full[..stock.len()].copy_from_slice(&stock);
    Fixture { stage0: stage0(&public), image: kumi_image(&signer), stock, full, public }
}

fn cost(plan: &Plan) -> usize {
    plan.steps
        .iter()
        .map(|s| match &s.op {
            Op::Erase { len, .. } => *len as usize,
            Op::Program { data, .. } => data.len(),
            Op::Verify { .. } => 0,
        })
        .sum()
}

fn budgets(total: usize) -> Vec<usize> {
    let mut points: Vec<usize> = (0..total).step_by(total / 97 + 1).collect();
    points.extend([0, 1, 255, 256, 4095, 4096, 4097, total - 1]);
    points.sort_unstable();
    points.dedup();
    points
}

fn run_with_cuts(start: &[u8], plan: &Plan, f: &Fixture, allowed: &[Outcome], end: Outcome) -> usize {
    let total = cost(plan);
    let mut cases = 0;
    for budget in budgets(total) {
        let mut flash = SimFlash::new(start);
        flash.budget = Some(budget);
        match execute(plan, &mut flash, 0, |_, _| {}) {
            Ok(()) => assert_eq!(outcome(&flash.data, &f.stock, &f.stage0), end),
            Err(error) => {
                let now = outcome(&flash.data, &f.stock, &f.stage0);
                assert!(allowed.contains(&now), "budget {budget}: {now:?} after {error:?}");
                flash.budget = None;
                let resume = resume_point(plan, error.step());
                execute(plan, &mut flash, resume, |_, _| {}).unwrap();
                assert_eq!(outcome(&flash.data, &f.stock, &f.stage0), end, "budget {budget}");
            }
        }
        cases += 1;
    }
    cases
}

#[test]
fn install_writes_everything_and_boots_kumi() {
    let f = fixture();
    let plan = plan_install(&f.stock, &f.stock[..0x1000], &f.stage0, &f.image, &f.public, &gates()).unwrap();
    let mut flash = SimFlash::new(&f.full);
    assert_eq!(outcome(&flash.data, &f.stock, &f.stage0), Outcome::Stock);
    execute(&plan, &mut flash, 0, |_, _| {}).unwrap();
    assert_eq!(outcome(&flash.data, &f.stock, &f.stage0), Outcome::Kumi);
    assert_eq!(&flash.data[SLOT_A.offset as usize..][..f.image.len()], f.image.as_slice());
    assert_eq!(&flash.data[STOCK_COPY.offset as usize..][..f.stock.len()], f.stock.as_slice());
    assert!(flash.data[BOOT_JOURNAL.offset as usize..BOOT_JOURNAL.end() as usize].iter().all(|b| *b == 0xFF));
    let mut verify = kumi_boot_core::verify_slot(&mut Nor(&mut flash.data), SlotId::A, &key().verifying_key());
    assert!(verify.as_mut().is_ok());
}

#[test]
fn install_survives_power_loss_at_any_point() {
    let f = fixture();
    let plan = plan_install(&f.stock, &f.stock[..0x1000], &f.stage0, &f.image, &f.public, &gates()).unwrap();
    let cases = run_with_cuts(&f.full, &plan, &f, &[Outcome::Stock, Outcome::Service, Outcome::Kumi], Outcome::Kumi);
    assert!(cases > 90);
}

#[test]
fn boot_state_annotations_never_claim_a_bootable_half_image() {
    let f = fixture();
    let plan = plan_install(&f.stock, &f.stock[..0x1000], &f.stage0, &f.image, &f.public, &gates()).unwrap();
    let mut flash = SimFlash::new(&f.full);
    let mut seen = Vec::new();
    for (i, step) in plan.steps.iter().enumerate() {
        execute(&Plan { kind: plan.kind, steps: vec![step.clone()] }, &mut flash, 0, |_, _| {}).unwrap();
        let now = outcome(&flash.data, &f.stock, &f.stage0);
        let expected = match step.boot_after {
            BootAfter::Stock => Outcome::Stock,
            BootAfter::Service => Outcome::Service,
            BootAfter::Kumi => Outcome::Kumi,
            BootAfter::Unchanged => now,
        };
        assert_eq!(now, expected, "step {i} {}", step.label);
        if seen.last() != Some(&step.boot_after) {
            seen.push(step.boot_after);
        }
    }
    assert_eq!(seen, [BootAfter::Stock, BootAfter::Service, BootAfter::Kumi]);
}

fn installed(f: &Fixture) -> SimFlash {
    let plan = plan_install(&f.stock, &f.stock[..0x1000], &f.stage0, &f.image, &f.public, &gates()).unwrap();
    let mut flash = SimFlash::new(&f.full);
    execute(&plan, &mut flash, 0, |_, _| {}).unwrap();
    flash
}

#[test]
fn restore_stock_from_kumi_survives_power_loss() {
    let f = fixture();
    let start = installed(&f).data;
    let plan = plan_restore_stock(&f.stock, RestoreMode::StockOnly, None).unwrap();
    let cases = run_with_cuts(&start, &plan, &f, &[Outcome::Kumi, Outcome::Service, Outcome::Stock], Outcome::Stock);
    assert!(cases > 90);
}

#[test]
fn exact_restore_returns_the_flash_bit_for_bit() {
    let f = fixture();
    let mut flash = installed(&f);
    let plan = plan_restore_stock(&f.stock, RestoreMode::Exact, Some(&f.full)).unwrap();
    execute(&plan, &mut flash, 0, |_, _| {}).unwrap();
    assert!(flash.data == f.full);
    let cases = run_with_cuts(&installed(&f).data, &plan, &f, &[Outcome::Kumi, Outcome::Service, Outcome::Stock], Outcome::Stock);
    assert!(cases > 90);
}

#[test]
fn restore_can_use_the_stock_copy_kept_in_flash() {
    let f = fixture();
    let mut flash = installed(&f);
    let copy = flash.data[STOCK_COPY.offset as usize..STOCK_COPY.end() as usize].to_vec();
    let plan = plan_restore_stock(&copy, RestoreMode::StockOnly, None).unwrap();
    execute(&plan, &mut flash, 0, |_, _| {}).unwrap();
    assert_eq!(outcome(&flash.data, &f.stock, &f.stage0), Outcome::Stock);
}

#[test]
fn plans_refuse_unsafe_inputs() {
    {
        let f = fixture();
        let mut kumi_head = f.stage0.clone();
        kumi_head.resize(0x4_0000, 0xFF);
        assert_eq!(
            plan_install(&kumi_head, &f.stock[..0x1000], &f.stage0, &f.image, &f.public, &gates()),
            Err(PlanError::BackupIsKumi)
        );
        assert_eq!(plan_restore_stock(&kumi_head, RestoreMode::StockOnly, None), Err(PlanError::BackupIsKumi));
        assert!(spv::is_kumi_stage0(&f.stage0) && !spv::is_kumi_stage0(&f.stock));
    }
    let f = fixture();
    let other = SigningKey::from_bytes(&[8; 32]);
    assert_eq!(
        plan_install(&f.stock, &f.stock[..0x1000], &f.stage0, &kumi_image(&other), &f.public, &gates()),
        Err(PlanError::ImageSignature)
    );
    assert_eq!(
        plan_install(&f.stock, &f.stock[..0x1000], &stage0(&[0x42; 32]), &f.image, &f.public, &gates()),
        Err(PlanError::Stage0LacksKey)
    );
    let mut unchecked = f.stage0.clone();
    unchecked[0x0C] &= !1;
    assert_eq!(
        plan_install(&f.stock, &f.stock[..0x1000], &unchecked, &f.image, &f.public, &gates()),
        Err(PlanError::Stage0Unchecked)
    );
    let mut wrong_alias = f.stage0.clone();
    wrong_alias[0x0C] |= 2;
    let sum = spv::image_sum(&wrong_alias);
    wrong_alias[0x2C..0x30].copy_from_slice(&sum.to_le_bytes());
    assert_eq!(
        plan_install(&f.stock, &f.stock[..0x1000], &wrong_alias, &f.image, &f.public, &gates()),
        Err(PlanError::Stage0Alias)
    );
    let mut broken_stock = f.stock.clone();
    broken_stock[0x1000] ^= 1;
    assert_eq!(
        plan_install(&broken_stock, &f.stock[..0x1000], &f.stage0, &f.image, &f.public, &gates()),
        Err(PlanError::BackupNotBootable)
    );
    assert_eq!(
        plan_install(&f.stock[..0x1000], &f.stock[..0x1000], &f.stage0, &f.image, &f.public, &gates()),
        Err(PlanError::BackupSize)
    );
    let mut truncated = f.image.clone();
    truncated.pop();
    assert_eq!(
        plan_install(&f.stock, &f.stock[..0x1000], &f.stage0, &truncated, &f.public, &gates()),
        Err(PlanError::ImageHeader)
    );
    assert_eq!(plan_restore_stock(&f.stock, RestoreMode::Exact, None), Err(PlanError::BackupSize));
}

#[test]
fn plan_description_reads_in_order() {
    let f = fixture();
    let lines = plan_install(&f.stock, &f.stock[..0x1000], &f.stage0, &f.image, &f.public, &gates()).unwrap().describe();
    assert!(lines.first().unwrap().starts_with("stock copy: erase"));
    assert!(lines.iter().any(|l| l.starts_with("disarm stock header") && l.ends_with("Service")));
    assert!(lines.last().unwrap().ends_with("Kumi"));
}

#[test]
fn the_real_stock_dump_passes_the_bootrom_model_when_present() {
    let Some(path) = std::env::var_os("KUMI_STOCK_DUMP") else {
        return;
    };
    let dump = std::fs::read(path).expect("KUMI_STOCK_DUMP must name a readable 4 MiB flash dump");
    assert_eq!(dump.len(), FLASH_SIZE as usize);
    assert_eq!(spv::brom_verdict(&dump), Verdict::Boots { length: 0x2_02AE, checksummed: true, alias_48: false });
    assert_eq!(kumi_layout::get32(&dump, 0x0C), spv::KUMI_USER_FLAGS);
    let stock = &dump[..0x4_0000];
    let f = fixture();
    let plan = plan_install(stock, &dump[..0x1000], &f.stage0, &f.image, &f.public, &gates()).unwrap();
    let mut flash = SimFlash::new(&dump);
    execute(&plan, &mut flash, 0, |_, _| {}).unwrap();
    let restore = plan_restore_stock(stock, RestoreMode::Exact, Some(&dump)).unwrap();
    execute(&restore, &mut flash, 0, |_, _| {}).unwrap();
    assert!(flash.data == dump);
    assert_eq!(hex(&sha256(&flash.data)), "70691702f030f7c04f4d10ad23412d6cf4c0103fb6984f6ae2d1272c5beee8f9");
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

struct Nor<'a>(&'a mut Vec<u8>);

impl embedded_storage::nor_flash::ErrorType for Nor<'_> {
    type Error = core::convert::Infallible;
}

impl embedded_storage::nor_flash::ReadNorFlash for Nor<'_> {
    const READ_SIZE: usize = 1;

    fn read(&mut self, offset: u32, bytes: &mut [u8]) -> Result<(), Self::Error> {
        bytes.copy_from_slice(&self.0[offset as usize..offset as usize + bytes.len()]);
        Ok(())
    }

    fn capacity(&self) -> usize {
        self.0.len()
    }
}

#[test]
fn a_partly_programmed_header_page_never_passes_the_boot_rom_checks() {
    let f = fixture();
    for (plan, start) in [
        (plan_install(&f.stock, &f.stock[..0x1000], &f.stage0, &f.image, &f.public, &gates()).unwrap(), f.full.clone()),
        (plan_restore_stock(&f.stock, RestoreMode::StockOnly, None).unwrap(), installed(&f).data),
    ] {
        let checksum_step = plan
            .steps
            .iter()
            .position(|s| matches!(&s.op, Op::Program { offset: 0x2C, data } if data.len() == 4))
            .expect("the checksum is written by its own step");
        assert!(plan.steps[checksum_step].unit > plan.steps[checksum_step - 1].unit);
        for step in &plan.steps[..checksum_step] {
            if let Op::Program { offset: 0, data } = &step.op {
                assert_eq!(&data[0x2C..0x30], &[0xFF; 4]);
            }
        }
        let mut flash = SimFlash::new(&start);
        execute(&Plan { kind: plan.kind, steps: plan.steps[..checksum_step].to_vec() }, &mut flash, 0, |_, _| {}).unwrap();
        let before_page: Vec<u8> = vec![0xFF; 256];
        let final_page = flash.data[..256].to_vec();
        let mut seed = 0x1234_5678u32;
        for _ in 0..4000 {
            let mut trial = flash.data.clone();
            for i in 0..256 {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let keep_unprogrammed = (seed >> 8) as u8 & !final_page[i];
                trial[i] = (before_page[i] & final_page[i]) | keep_unprogrammed;
            }
            assert_eq!(spv::brom_verdict(&trial), Verdict::Service);
        }
        assert_eq!(spv::brom_verdict(&flash.data), Verdict::Service);
    }
}

#[test]
fn install_invalidates_a_leftover_slot_b_and_erases_one_sector_at_a_time() {
    let f = fixture();
    let newer = sample::image(&key(), [0, 9, 0], 9);
    let mut start = f.full.clone();
    start[SLOT_B.offset as usize..][..newer.len()].copy_from_slice(&newer);
    let plan = plan_install(&f.stock, &f.stock[..0x1000], &f.stage0, &f.image, &f.public, &gates()).unwrap();
    assert!(plan.steps.iter().all(|s| !matches!(s.op, Op::Erase { len, .. } if len != SECTOR)));
    assert!(matches!(&plan.steps.iter().find(|s| s.boot_after == BootAfter::Service).unwrap().op,
        Op::Program { offset: 4, data } if data == &vec![0u8; 4]));
    let mut flash = SimFlash::new(&start);
    execute(&plan, &mut flash, 0, |_, _| {}).unwrap();
    assert!(flash.data[SLOT_B.offset as usize..][..SECTOR as usize].iter().all(|b| *b == 0xFF));
    let mut nor = Nor(&mut flash.data);
    assert!(kumi_boot_core::verify_slot(&mut nor, SlotId::B, &key().verifying_key()).is_err());
}

#[test]
fn installing_needs_recorded_evidence_for_gates_g3_g4_and_g2b() {
    let f = fixture();
    let plan = |gates: &GateEvidence| plan_install(&f.stock, &f.stock[..0x1000], &f.stage0, &f.image, &f.public, gates);
    assert_eq!(plan(&GateEvidence::new()), Err(PlanError::GateMissing(Gate::G3Emulator)));
    assert!(plan(&GateEvidence::recorded()).is_ok());
    let without_g2b = GateEvidence::new().with(Gate::G3Emulator, "emulated").with(Gate::G4ResetRetention, "measured");
    assert_eq!(plan(&without_g2b), Err(PlanError::GateMissing(Gate::G2bWritePath)));
    assert_eq!(plan(&GateEvidence::new().with(Gate::G4ResetRetention, "  ")), Err(PlanError::GateMissing(Gate::G3Emulator)));
    let without_g4 = GateEvidence::new().with(Gate::G3Emulator, "emulated").with(Gate::G2bWritePath, "measured");
    assert_eq!(plan(&without_g4), Err(PlanError::GateMissing(Gate::G4ResetRetention)));
    let complete = GateEvidence::recorded().with(Gate::G2bWritePath, "measured");
    assert!(plan(&complete).is_ok());
    assert!(GateEvidence::recorded().record(Gate::G3Emulator).unwrap().evidence.contains("0666:0888"));
    assert!(GateEvidence::recorded().record(Gate::G4ResetRetention).unwrap().evidence.contains("came back as 0666:0888"));
    assert_eq!(plan_install(&f.stock, &f.stock[..0x800], &f.stage0, &f.image, &f.public, &gates()), Err(PlanError::LiveHead));
}

#[test]
fn reinstalling_over_kumi_disarms_first_and_never_starts_the_old_stage0_on_a_partial_slot() {
    let f = fixture();
    let start = installed(&f).data;
    let stock = start[STOCK_COPY.offset as usize..STOCK_COPY.end() as usize].to_vec();
    let newer = sample::image(&key(), [0, 2, 0], 2);
    let plan = plan_install(&stock, &start[..0x1000], &f.stage0, &newer, &f.public, &gates()).unwrap();
    assert!(matches!(&plan.steps[0].op, Op::Program { offset: 4, data } if data == &vec![0u8; 4]));
    assert_eq!(plan.steps[0].boot_after, BootAfter::Service);
    let mut seen = Vec::new();
    for step in &plan.steps {
        if seen.last() != Some(&step.boot_after) {
            seen.push(step.boot_after);
        }
    }
    assert_eq!(seen, [BootAfter::Service, BootAfter::Kumi]);
    let total = cost(&plan);
    let mut cases = 0;
    for budget in budgets(total) {
        let mut flash = SimFlash::new(&start);
        flash.budget = Some(budget);
        if let Err(error) = execute(&plan, &mut flash, 0, |_, _| {}) {
            match outcome(&flash.data, &f.stock, &f.stage0) {
                Outcome::Service => {}
                Outcome::Kumi => {
                    assert!(flash.data == start, "budget {budget}: the old stage0 would start after {error:?}");
                }
                other => panic!("budget {budget}: {other:?} after {error:?}"),
            }
            flash.budget = None;
            execute(&plan, &mut flash, resume_point(&plan, error.step()), |_, _| {}).unwrap();
        }
        assert_eq!(outcome(&flash.data, &f.stock, &f.stage0), Outcome::Kumi, "budget {budget}");
        let header = kumi_boot_core::verify_slot(&mut Nor(&mut flash.data), SlotId::A, &key().verifying_key()).unwrap();
        assert_eq!(header.generation, 2);
        assert!(flash.data[BOOT_JOURNAL.offset as usize..BOOT_JOURNAL.end() as usize].iter().all(|b| *b == 0xFF));
        cases += 1;
    }
    assert!(cases > 90);
}
