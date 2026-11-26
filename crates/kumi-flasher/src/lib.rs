// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
use std::fmt;
use std::fs;
#[cfg(feature = "host-transports")]
use std::io;
use std::io::Write;
#[cfg(feature = "host-transports")]
use std::process::Command;
#[cfg(feature = "host-transports")]
use std::time::Duration;

use kumi_buxx::Brom;
pub use kumi_buxx::{EXPECTED_CHIP, Identity, Link, PACKET, PRODUCT_ID, VENDOR_ID};
#[cfg(feature = "host-transports")]
use kumi_client::bridge::{DEVICE_DESCRIPTOR, HelperMode, HelperProcess, helper_command_for, phone_command};
#[cfg(feature = "host-transports")]
use kumi_client::usb::BromUsbTransport;
use kumi_install::bundle;
pub use kumi_install::bundle::{Bundle, BundleError};
use kumi_install::plan::STOCK_SPAN;
use kumi_install::spv::{self, Verdict};
pub use kumi_install::{BootAfter, ExecError, Gate, GateEvidence, PlanError, REQUIRED_FOR_INSTALL, RestoreMode, TargetError};
use kumi_install::{Plan, execute, plan_install, plan_restore_stock, resume_point, sha256};
use kumi_layout::{FLASH_SIZE, SECTOR, STOCK_COPY};

pub trait Progress {
    fn report(&mut self, done: u64, total: u64, label: &str);
}

impl<F: FnMut(u64, u64, &str)> Progress for F {
    fn report(&mut self, done: u64, total: u64, label: &str) {
        self(done, total, label)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StockSource {
    Head,
    Copy,
    None,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Backup {
    pub path: String,
    pub sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackupReport {
    pub backup: Backup,
    pub size: usize,
    pub stock: StockSource,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub steps: usize,
    pub boots: BootAfter,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Interruption {
    pub error: ExecError,
    pub step: usize,
    pub resume: usize,
    pub label: Option<String>,
    pub boots: [BootAfter; 2],
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum InstallError {
    Brom(kumi_buxx::Error),
    Key,
    Package(BundleError),
    Plan(PlanError),
    Storage(String),
    StorageReadback,
    BackupMissing(String),
    BackupChanged,
    BackupSize,
    NotStock,
    BackupStale,
    NoStockInBackup,
    ExactNeedsStockHead,
    StockMismatch,
    Interrupted(Interruption),
}

impl InstallError {
    pub fn kind(&self) -> String {
        match self {
            InstallError::Brom(error) => format!("{error:?}"),
            InstallError::Package(error) => format!("{error:?}"),
            InstallError::Plan(error) => format!("{error:?}"),
            InstallError::Interrupted(interruption) => format!("{:?}", interruption.error),
            InstallError::Key => "Key".into(),
            InstallError::Storage(_) => "Storage".into(),
            InstallError::StorageReadback => "StorageReadback".into(),
            InstallError::BackupMissing(_) => "BackupMissing".into(),
            InstallError::BackupChanged => "BackupChanged".into(),
            InstallError::BackupSize => "BackupSize".into(),
            InstallError::NotStock => "NotStock".into(),
            InstallError::BackupStale => "BackupStale".into(),
            InstallError::NoStockInBackup => "NoStockInBackup".into(),
            InstallError::ExactNeedsStockHead => "ExactNeedsStockHead".into(),
            InstallError::StockMismatch => "StockMismatch".into(),
        }
    }

    pub fn resume_from(&self) -> Option<usize> {
        match self {
            InstallError::Interrupted(interruption) => Some(interruption.resume),
            _ => None,
        }
    }

    pub fn wrote_nothing(&self) -> bool {
        !matches!(self, InstallError::Interrupted(_))
    }
}

impl fmt::Display for InstallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InstallError::Storage(detail) | InstallError::BackupMissing(detail) => write!(f, "{}: {detail}", self.kind()),
            InstallError::Interrupted(interruption) => write!(
                f,
                "{} at step {}{}; resume from step {}",
                self.kind(),
                interruption.step,
                interruption.label.as_deref().map(|label| format!(" ({label})")).unwrap_or_default(),
                interruption.resume
            ),
            _ => f.write_str(&self.kind()),
        }
    }
}

impl std::error::Error for InstallError {}

pub fn parse_key(hex: &str) -> Option<[u8; 32]> {
    kumi_install::pack::parse_hex32(hex).ok()
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn open_package(data: &[u8], public_key: &str) -> Result<Bundle, InstallError> {
    let public = parse_key(public_key).ok_or(InstallError::Key)?;
    bundle::open(data, &public).map_err(InstallError::Package)
}

pub fn identify<L: Link>(link: L) -> Result<Identity, InstallError> {
    Brom::new(link).identify().map_err(InstallError::Brom)
}

fn stock_checks(image: &[u8]) -> bool {
    matches!(spv::brom_verdict(image), Verdict::Boots { checksummed: true, .. })
}

fn copy_of(flash: &[u8]) -> &[u8] {
    &flash[STOCK_COPY.offset as usize..(STOCK_COPY.offset + STOCK_SPAN) as usize]
}

pub fn stock_source(flash: &[u8]) -> StockSource {
    if flash.len() != FLASH_SIZE as usize {
        return StockSource::None;
    }
    let head = &flash[..STOCK_SPAN as usize];
    let copy = copy_of(flash);
    if stock_checks(head) && !spv::is_kumi_stage0(head) {
        StockSource::Head
    } else if stock_checks(copy) && !spv::is_kumi_stage0(copy) {
        StockSource::Copy
    } else {
        StockSource::None
    }
}

fn stock_of(flash: &[u8]) -> Option<&[u8]> {
    match stock_source(flash) {
        StockSource::Head => Some(&flash[..STOCK_SPAN as usize]),
        StockSource::Copy => Some(copy_of(flash)),
        StockSource::None => None,
    }
}

fn write_atomically(path: &str, data: &[u8]) -> std::io::Result<()> {
    let partial = format!("{path}.partial");
    let mut file = fs::File::create(&partial)?;
    file.write_all(data)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&partial, path)?;
    if let Some(dir) = std::path::Path::new(path).parent()
        && let Ok(d) = fs::File::open(dir)
    {
        let _ = d.sync_all();
    }
    Ok(())
}

pub fn backup<L: Link>(link: L, path: &str, progress: &mut impl Progress) -> Result<BackupReport, InstallError> {
    let mut brom = Brom::new(link);
    brom.require_chip().map_err(InstallError::Brom)?;
    let total = 2 * u64::from(FLASH_SIZE);
    let data = brom.backup(FLASH_SIZE, |done| progress.report(u64::from(done), total, "backup")).map_err(InstallError::Brom)?;
    write_atomically(path, &data).map_err(|e| InstallError::Storage(e.to_string()))?;
    let written = match fs::read(path) {
        Ok(back) if back == data => back,
        _ => return Err(InstallError::StorageReadback),
    };
    Ok(BackupReport {
        backup: Backup { path: path.to_string(), sha256: hex(&sha256(&written)) },
        size: written.len(),
        stock: stock_source(&written),
    })
}

fn load_backup(backup: &Backup) -> Result<Vec<u8>, InstallError> {
    let data = fs::read(&backup.path).map_err(|e| InstallError::BackupMissing(e.to_string()))?;
    if hex(&sha256(&data)) != backup.sha256.to_ascii_lowercase() {
        return Err(InstallError::BackupChanged);
    }
    if data.len() != FLASH_SIZE as usize {
        return Err(InstallError::BackupSize);
    }
    Ok(data)
}

fn run_plan<L: Link>(mut brom: Brom<L>, plan: &Plan, from: usize, progress: &mut impl Progress) -> Result<Outcome, InstallError> {
    brom.require_chip().map_err(InstallError::Brom)?;
    brom.unlock_flash_writes();
    let total = plan.steps.len() as u64;
    match execute(plan, &mut brom, from, |i, step| progress.report(i as u64, total, &step.label)) {
        Ok(()) => {
            progress.report(total, total, "done");
            Ok(Outcome { steps: plan.steps.len(), boots: plan.steps.last().map_or(BootAfter::Unchanged, |s| s.boot_after) })
        }
        Err(error) => {
            let step = error.step();
            let before = step.checked_sub(1).and_then(|i| plan.steps.get(i)).map_or(BootAfter::Unchanged, |s| s.boot_after);
            let after = plan.steps.get(step).map_or(BootAfter::Unchanged, |s| s.boot_after);
            Err(InstallError::Interrupted(Interruption {
                error,
                step,
                resume: resume_point(plan, step),
                label: plan.steps.get(step).map(|s| s.label.clone()),
                boots: [before, after],
            }))
        }
    }
}

pub fn install<L: Link>(
    link: L,
    backup: &Backup,
    package: &[u8],
    public_key: &str,
    gates: &GateEvidence,
    from: usize,
    progress: &mut impl Progress,
) -> Result<Outcome, InstallError> {
    let flash = load_backup(backup)?;
    if stock_source(&flash) != StockSource::Head {
        return Err(InstallError::NotStock);
    }
    let public = parse_key(public_key).ok_or(InstallError::Key)?;
    let package = bundle::open(package, &public).map_err(InstallError::Package)?;
    let head = &flash[..SECTOR as usize];
    let mut brom = Brom::new(link);
    if from == 0 {
        let mut live = vec![0u8; head.len()];
        brom.read_flash(0, &mut live).map_err(InstallError::Brom)?;
        if live != head {
            return Err(InstallError::BackupStale);
        }
    }
    let plan = plan_install(&flash[..STOCK_SPAN as usize], head, &package.stage0, &package.image, &public, gates)
        .map_err(InstallError::Plan)?;
    run_plan(brom, &plan, from, progress)
}

fn case_holds<L: Link>(brom: &mut Brom<L>, stock: &[u8]) -> Result<bool, kumi_buxx::Error> {
    let mut buf = vec![0u8; STOCK_SPAN as usize];
    for offset in [STOCK_COPY.offset, 0] {
        brom.read_flash(offset, &mut buf)?;
        if buf == stock {
            return Ok(true);
        }
    }
    Ok(false)
}

pub fn restore_stock<L: Link>(
    link: L,
    backup: &Backup,
    mode: RestoreMode,
    from: usize,
    progress: &mut impl Progress,
) -> Result<Outcome, InstallError> {
    let flash = load_backup(backup)?;
    let stock = stock_of(&flash).ok_or(InstallError::NoStockInBackup)?;
    let exact = mode == RestoreMode::Exact;
    if exact && stock_source(&flash) != StockSource::Head {
        return Err(InstallError::ExactNeedsStockHead);
    }
    let mut brom = Brom::new(link);
    if from == 0 && !case_holds(&mut brom, stock).map_err(InstallError::Brom)? {
        return Err(InstallError::StockMismatch);
    }
    let plan = plan_restore_stock(stock, mode, exact.then_some(flash.as_slice())).map_err(InstallError::Plan)?;
    run_plan(brom, &plan, from, progress)
}

pub fn reboot<L: Link>(link: L) -> Result<(), InstallError> {
    Brom::new(link).reboot().map_err(InstallError::Brom)
}

#[cfg(feature = "host-transports")]
pub const BROM_REPLY_TIMEOUT: Duration = Duration::from_secs(25);

#[cfg(feature = "host-transports")]
pub fn is_service_descriptor(identity: &[u8]) -> bool {
    identity.len() >= DEVICE_DESCRIPTOR
        && identity[1] == 1
        && u16::from_le_bytes([identity[8], identity[9]]) == VENDOR_ID
        && u16::from_le_bytes([identity[10], identity[11]]) == PRODUCT_ID
}

#[cfg(feature = "host-transports")]
pub struct BromLink {
    process: HelperProcess,
    timeout: Duration,
}

#[cfg(feature = "host-transports")]
impl BromLink {
    pub fn phone(serial: &str, helper: &str, usb_path: &str, deadline: u32, writes: bool) -> io::Result<Self> {
        let mode = if writes { HelperMode::InstallBridge } else { HelperMode::Bridge };
        Self::spawn(phone_command(serial, &helper_command_for(helper, usb_path, deadline, mode)?))
    }

    pub fn spawn(command: Command) -> io::Result<Self> {
        let mut process = HelperProcess::start(command)?;
        if !is_service_descriptor(&process.first_message()?) {
            return Err(io::Error::other("the phone helper did not open a boot ROM 0666:0888 device"));
        }
        Ok(Self { process, timeout: BROM_REPLY_TIMEOUT })
    }

    pub fn set_timeout(&mut self, timeout: Duration) {
        self.timeout = timeout;
    }
}

#[cfg(feature = "host-transports")]
impl Link for BromLink {
    fn transfer(&mut self, packet: &[u8]) -> Result<Vec<u8>, kumi_buxx::Error> {
        if packet.len() > PACKET {
            return Err(kumi_buxx::Error::Link(format!("a boot ROM request of {} bytes", packet.len())));
        }
        let mut request = packet.to_vec();
        request.resize(PACKET, 0);
        let link = |e: io::Error| kumi_buxx::Error::Link(e.to_string());
        self.process.write(&request).map_err(link)?;
        self.process.next(self.timeout).map_err(link)?.ok_or_else(|| kumi_buxx::Error::Link("no reply from the boot ROM".into()))
    }
}

#[cfg(feature = "host-transports")]
pub struct UsbBromLink {
    transport: BromUsbTransport,
}

#[cfg(feature = "host-transports")]
impl UsbBromLink {
    pub fn open(selector: Option<(u8, u8)>) -> io::Result<Self> {
        Ok(Self { transport: BromUsbTransport::open_at(selector)? })
    }
}

#[cfg(feature = "host-transports")]
impl Link for UsbBromLink {
    fn transfer(&mut self, packet: &[u8]) -> Result<Vec<u8>, kumi_buxx::Error> {
        self.transport.transfer(packet).map_err(|e| kumi_buxx::Error::Link(e.to_string()))
    }
}

#[cfg(all(test, feature = "host-transports"))]
mod tests {
    use super::*;

    fn octal(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("\\{b:03o}")).collect()
    }

    fn helper(messages: &[&[u8]]) -> Command {
        let mut stream = Vec::new();
        for message in messages {
            stream.extend_from_slice(&(message.len() as u16).to_le_bytes());
            stream.extend_from_slice(message);
        }
        let mut command = Command::new("sh");
        command.args(["-c", &format!("printf '{}'; cat > /dev/null", octal(&stream))]);
        command
    }

    const SERVICE: [u8; 18] = [18, 1, 0, 2, 0, 0, 0, 64, 0x66, 0x06, 0x88, 0x08, 0, 1, 1, 2, 0, 1];

    #[test]
    fn the_boot_rom_link_needs_the_service_device_and_returns_each_reply() {
        let mut kumi = SERVICE;
        kumi[8..12].copy_from_slice(&[0x09, 0x12, 0x01, 0x00]);
        assert!(BromLink::spawn(helper(&[&kumi])).is_err());
        let mut link = BromLink::spawn(helper(&[&SERVICE, b"BUXX reply"])).unwrap();
        link.set_timeout(Duration::from_secs(2));
        assert_eq!(link.transfer(b"BUXX").unwrap(), b"BUXX reply");
        assert!(link.transfer(&[0; PACKET + 1]).is_err());
        assert!(matches!(link.transfer(b"BUXX"), Err(kumi_buxx::Error::Link(_))));
    }
}
