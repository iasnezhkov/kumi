// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
use std::cell::RefCell;
use std::rc::Rc;

use kumi_install::{BootAfter, ExecError, GateEvidence, PlanError, RestoreMode, TargetError};
use kumi_jni::installer::{self, Backup, InstallError, StockSource};
use kumi_jni::sim::{self, Mode, Sim};

#[derive(Clone)]
struct Port(Rc<RefCell<Sim>>, Rc<RefCell<Option<usize>>>);

impl Port {
    fn service() -> Self {
        let sim = Rc::new(RefCell::new(Sim::new()));
        sim.borrow_mut().set_mode(Mode::Service);
        Port(sim, Rc::new(RefCell::new(None)))
    }
}

impl kumi_buxx::Link for Port {
    fn transfer(&mut self, packet: &[u8]) -> Result<Vec<u8>, kumi_buxx::Error> {
        if let Some(left) = self.1.borrow_mut().as_mut() {
            if *left == 0 {
                return Err(kumi_buxx::Error::Link("unplugged".into()));
            }
            *left -= 1;
        }
        let mut sim = self.0.borrow_mut();
        if !sim.send(packet) {
            return Err(kumi_buxx::Error::Link("no device".into()));
        }
        sim.receive().ok_or_else(|| kumi_buxx::Error::Link("timeout".into()))
    }
}

fn quiet() -> impl FnMut(u64, u64, &str) {
    |_, _, _| {}
}

fn temp(name: &str) -> String {
    let dir = std::env::temp_dir().join(format!("kumi-installer-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("backup.bin").to_string_lossy().into_owned()
}

#[test]
fn the_typed_installer_installs_resumes_and_restores_bit_for_bit() {
    let port = Port::service();
    let key = sim::demo_public_hex();
    let package = sim::demo_package([0, 1, 0], 1);
    assert_eq!(installer::identify(port.clone()).unwrap().chip_id, kumi_buxx::EXPECTED_CHIP);
    assert_eq!(installer::open_package(&package, &key).unwrap().version, [0, 1, 0]);

    let path = temp("typed");
    let report = installer::backup(port.clone(), &path, &mut quiet()).unwrap();
    assert_eq!((report.size, report.stock), (4 * 1024 * 1024, StockSource::Head));
    let original = std::fs::read(&path).unwrap();
    let backup = report.backup;

    *port.1.borrow_mut() = Some(40);
    let stopped =
        installer::install(port.clone(), &backup, &package, &key, &GateEvidence::for_simulation(), 0, &mut quiet()).unwrap_err();
    *port.1.borrow_mut() = None;
    let InstallError::Interrupted(stop) = &stopped else { panic!("{stopped:?}") };
    assert_eq!(stop.error, ExecError::Target { step: stop.step, error: TargetError::Transport });
    assert_eq!(stopped.resume_from(), Some(stop.resume));
    assert!(!stopped.wrote_nothing());
    assert!(stopped.to_string().contains("resume from step"), "{stopped}");

    let done =
        installer::install(port.clone(), &backup, &package, &key, &GateEvidence::for_simulation(), stop.resume, &mut quiet())
            .unwrap();
    assert_eq!(done.boots, BootAfter::Kumi);
    installer::reboot(port.clone()).unwrap();
    assert_eq!(port.0.borrow().mode(), Mode::Kumi);

    port.0.borrow_mut().set_mode(Mode::Service);
    let restored = installer::restore_stock(port.clone(), &backup, RestoreMode::Exact, 0, &mut quiet()).unwrap();
    assert_eq!(restored.boots, BootAfter::Stock);
    installer::reboot(port.clone()).unwrap();
    assert_eq!(port.0.borrow().flash().unwrap(), original.as_slice());
}

#[test]
fn refusals_arrive_as_typed_errors_before_anything_is_written() {
    let port = Port::service();
    let key = sim::demo_public_hex();
    let package = sim::demo_package([0, 1, 0], 1);
    let path = temp("refusals");
    let backup = installer::backup(port.clone(), &path, &mut quiet()).unwrap().backup;
    let gates = GateEvidence::for_simulation();

    let changed = Backup { sha256: "00".repeat(32), ..backup.clone() };
    assert_eq!(
        installer::install(port.clone(), &changed, &package, &key, &gates, 0, &mut quiet()),
        Err(InstallError::BackupChanged)
    );
    let missing = Backup { path: format!("{path}.absent"), ..backup.clone() };
    assert!(matches!(
        installer::install(port.clone(), &missing, &package, &key, &gates, 0, &mut quiet()),
        Err(InstallError::BackupMissing(_))
    ));
    assert_eq!(installer::install(port.clone(), &backup, &package, "zz", &gates, 0, &mut quiet()), Err(InstallError::Key));
    let other = installer::hex(ed25519_dalek::SigningKey::from_bytes(&[1; 32]).verifying_key().as_bytes());
    let refused = installer::install(port.clone(), &backup, &package, &other, &gates, 0, &mut quiet()).unwrap_err();
    assert_eq!(refused.kind(), "Signature");
    assert!(refused.wrote_nothing() && refused.resume_from().is_none());
    let ungated = installer::install(port.clone(), &backup, &package, &key, &GateEvidence::new(), 0, &mut quiet()).unwrap_err();
    assert!(matches!(ungated, InstallError::Plan(PlanError::GateMissing(_))), "{ungated:?}");
    assert_eq!(ungated.kind(), "GateMissing(G3Emulator)");
    assert_eq!(port.0.borrow().flash().unwrap()[..0x4_0000], kumi_install::sample::stock()[..]);
    let mut other_stock = kumi_install::sample::stock();
    other_stock[0x1000] ^= 0x01;
    let stranger = Port(Rc::new(RefCell::new(Sim::with_flash(&other_stock))), Rc::new(RefCell::new(None)));
    stranger.0.borrow_mut().set_mode(Mode::Service);
    assert_eq!(
        installer::restore_stock(stranger.clone(), &backup, RestoreMode::StockOnly, 0, &mut quiet()),
        Err(InstallError::StockMismatch)
    );
    assert_eq!(stranger.0.borrow().flash().unwrap()[..0x4_0000], other_stock[..]);
    assert!(matches!(installer::backup(Port::service(), "/nonexistent/dir/x.bin", &mut quiet()), Err(InstallError::Storage(_))));
}
