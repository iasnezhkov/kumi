// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
use std::cell::RefCell;
use std::rc::Rc;

use kumi_install::{GateEvidence, RestoreMode};
use kumi_jni::installer::{self, Backup, InstallError};
use kumi_jni::sim::{self, Mode, Sim};

#[derive(Clone)]
struct ServicePort(Rc<RefCell<Sim>>);

impl kumi_buxx::Link for ServicePort {
    fn transfer(&mut self, packet: &[u8]) -> Result<Vec<u8>, kumi_buxx::Error> {
        let mut case = self.0.borrow_mut();
        if !case.send(packet) {
            return Err(kumi_buxx::Error::Link("the case is not in service mode".into()));
        }
        case.receive().ok_or_else(|| kumi_buxx::Error::Link("no reply".into()))
    }
}

fn steps() -> impl FnMut(u64, u64, &str) {
    let mut last = String::new();
    move |done, total, label| {
        let part = label.split(':').next().unwrap_or(label).to_string();
        if part != last {
            println!("  step {done}/{total}: {part}");
            last = part;
        }
    }
}

fn main() -> Result<(), InstallError> {
    let case = Rc::new(RefCell::new(Sim::new()));
    case.borrow_mut().set_mode(Mode::Service);
    let port = ServicePort(case.clone());
    let directory = std::env::temp_dir().join(format!("kumi-install-example-{}", std::process::id()));
    std::fs::create_dir_all(&directory).map_err(|e| InstallError::Storage(e.to_string()))?;
    let path = directory.join("case-flash.bin").to_string_lossy().into_owned();

    let identity = installer::identify(port.clone())?;
    println!("boot ROM: chip {:#010x}, flash {:02x?}", identity.chip_id, identity.jedec);
    let report_backup = installer::backup(port.clone(), &path, &mut |done: u64, total: u64, _: &str| {
        if done == total {
            println!("  backup: {total} bytes read in two passes and compared");
        }
    })?;
    println!(
        "backup {} sha256 {} holds stock in {:?}",
        report_backup.backup.path, report_backup.backup.sha256, report_backup.stock
    );
    let original = std::fs::read(&path).map_err(|e| InstallError::Storage(e.to_string()))?;

    let package = sim::demo_package([0, 1, 0], 1);
    let key = sim::demo_public_hex();
    let bundle = installer::open_package(&package, &key)?;
    println!("package {:?} generation {} ({})", bundle.version, bundle.generation, bundle.build);
    let backup = Backup { path: path.clone(), sha256: report_backup.backup.sha256.clone() };
    let done = installer::install(port.clone(), &backup, &package, &key, &GateEvidence::for_simulation(), 0, &mut steps())?;
    println!("installed in {} steps; the case boots {:?}", done.steps, done.boots);
    installer::reboot(port.clone())?;
    println!("after the reboot the case runs {:?}", case.borrow().mode());

    case.borrow_mut().set_mode(Mode::Service);
    let restored = installer::restore_stock(port.clone(), &backup, RestoreMode::Exact, 0, &mut steps())?;
    installer::reboot(port)?;
    let same = case.borrow().flash() == Some(original.as_slice());
    println!("restored in {} steps; the case boots {:?}; flash equals the backup: {same}", restored.steps, restored.boots);
    let _ = std::fs::remove_dir_all(directory);
    Ok(())
}
