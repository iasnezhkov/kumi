// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
use kumi_buxx::{Brom, EXPECTED_CHIP, Link};
use kumi_client::update::UpdateError;
use kumi_client::wire::{FrameError, FrameLink, FrameTransport, WireState};
use kumi_client::{Client, ClientError, Transport};
use kumi_icd::*;
use kumi_install::{BootAfter, GateEvidence, REQUIRED_FOR_INSTALL, RestoreMode, bundle, sha256};
use postcard_rpc::Endpoint;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::installer::{self, Backup, InstallError, Outcome};

pub const BRIDGE_VERSION: u32 = 1;

pub use crate::installer::{Progress, StockSource, stock_source};

pub fn ok(value: Value) -> Value {
    json!({ "ok": value })
}

pub fn fail(kind: impl Into<String>) -> Value {
    json!({ "err": { "kind": kind.into() } })
}

fn fail_with(kind: impl Into<String>, mut extra: Value) -> Value {
    extra["kind"] = Value::String(kind.into());
    json!({ "err": extra })
}

pub fn hex(bytes: &[u8]) -> String {
    installer::hex(bytes)
}

fn key(hex: &str) -> Option<[u8; 32]> {
    installer::parse_key(hex)
}

fn version_text(v: [u8; 3]) -> String {
    format!("{}.{}.{}", v[0], v[1], v[2])
}

pub fn install_gates(gates: &GateEvidence) -> Value {
    ok(json!({ "missing": gates.missing(&REQUIRED_FOR_INSTALL).map(|gate| format!("{gate:?}")) }))
}

pub fn info() -> Value {
    json!({
        "bridge": BRIDGE_VERSION,
        "protocol": PROTOCOL_VERSION,
        "crate": env!("CARGO_PKG_VERSION"),
        "vendorId": USB_VENDOR_ID,
        "productId": USB_PRODUCT_ID,
        "interface": [VENDOR_INTERFACE_CLASS, VENDOR_INTERFACE_SUBCLASS, VENDOR_INTERFACE_PROTOCOL],
        "serviceVendorId": kumi_buxx::VENDOR_ID,
        "serviceProductId": kumi_buxx::PRODUCT_ID,
        "packageExtension": bundle::EXTENSION,
    })
}

pub fn open_package(data: &[u8], key_hex: &str) -> Value {
    match installer::open_package(data, key_hex) {
        Ok(b) => ok(json!({
            "version": version_text(b.version),
            "generation": b.generation,
            "build": b.build,
            "stage0": b.stage0.len(),
            "image": b.image.len(),
            "sha256": hex(&sha256(data)),
        })),
        Err(e) => failure(e),
    }
}

pub fn identify<L: Link>(link: L) -> Value {
    let mut brom = Brom::new(link);
    match brom.identify() {
        Ok(id) => ok(json!({
            "chip": id.chip_id,
            "chipMatches": id.chip_id == EXPECTED_CHIP,
            "jedec": hex(&id.jedec),
            "protect": id.block_protect_bits(),
            "capacity": id.capacity(),
        })),
        Err(e) => fail(format!("{e:?}")),
    }
}

fn boot_name(boot: BootAfter) -> &'static str {
    match boot {
        BootAfter::Stock => "Stock",
        BootAfter::Service => "Service",
        BootAfter::Kumi => "Kumi",
        BootAfter::Unchanged => "Unchanged",
    }
}

pub fn failure(error: InstallError) -> Value {
    match &error {
        InstallError::Storage(detail) | InstallError::BackupMissing(detail) => {
            fail_with(error.kind(), json!({ "detail": detail }))
        }
        InstallError::Interrupted(stop) => fail_with(
            error.kind(),
            json!({
                "step": stop.step,
                "resume": stop.resume,
                "label": stop.label,
                "boots": [boot_name(stop.boots[0]), boot_name(stop.boots[1])],
            }),
        ),
        _ => fail(error.kind()),
    }
}

fn outcome(result: Result<Outcome, InstallError>) -> Value {
    match result {
        Ok(done) => ok(json!({ "steps": done.steps as u64, "boots": boot_name(done.boots) })),
        Err(error) => failure(error),
    }
}

pub fn backup<L: Link>(link: L, path: &str, progress: &mut impl Progress) -> Value {
    match installer::backup(link, path, progress) {
        Ok(report) => ok(json!({
            "size": report.size,
            "sha256": report.backup.sha256,
            "stock": format!("{:?}", report.stock),
        })),
        Err(error) => failure(error),
    }
}

fn stored(path: &str, sha: &str) -> Backup {
    Backup { path: path.to_string(), sha256: sha.to_string() }
}

#[allow(clippy::too_many_arguments)]
pub fn install<L: Link>(
    link: L,
    backup_path: &str,
    backup_sha: &str,
    package: &[u8],
    key_hex: &str,
    gates: &GateEvidence,
    from: usize,
    progress: &mut impl Progress,
) -> Value {
    outcome(installer::install(link, &stored(backup_path, backup_sha), package, key_hex, gates, from, progress))
}

pub fn restore_stock<L: Link>(
    link: L,
    backup_path: &str,
    backup_sha: &str,
    exact: bool,
    from: usize,
    progress: &mut impl Progress,
) -> Value {
    let mode = if exact { RestoreMode::Exact } else { RestoreMode::StockOnly };
    outcome(installer::restore_stock(link, &stored(backup_path, backup_sha), mode, from, progress))
}

pub fn reboot_from_service<L: Link>(link: L) -> Value {
    match installer::reboot(link) {
        Ok(()) => ok(Value::Null),
        Err(error) => failure(error),
    }
}

fn events_json(events: Vec<kumi_client::events::Event>) -> Value {
    serde_json::to_value(events).unwrap_or(Value::Array(Vec::new()))
}

fn frame_error<E: std::fmt::Debug>(error: FrameError<E>) -> Value {
    match error {
        FrameError::Link(e) => fail_with("Link", json!({ "detail": format!("{e:?}") })),
        FrameError::Wire(w) => fail_with("Wire", json!({ "detail": format!("{w:?}") })),
        FrameError::NoReply => fail("NoReply"),
        other => fail_with("Transport", json!({ "detail": format!("{other:?}") })),
    }
}

fn client_error<E: std::fmt::Debug>(error: ClientError<FrameError<E>>) -> Value {
    match error {
        ClientError::Transport(e) => frame_error(e),
        ClientError::Encode => fail("Encode"),
        ClientError::Decode => fail("Decode"),
        ClientError::Case(e) => fail_with("Case", json!({ "detail": e.to_string() })),
        other => fail_with("Client", json!({ "detail": format!("{other:?}") })),
    }
}

pub fn poll_topics<L: FrameLink>(link: L, wire: &mut WireState, max_frames: usize) -> Value
where
    L::Error: std::fmt::Debug,
{
    let mut transport = FrameTransport::with_state(link, std::mem::take(wire));
    let polled = transport.poll_events(max_frames);
    *wire = transport.into_state();
    match polled {
        Ok(events) => ok(events_json(events)),
        Err(error) => frame_error(error),
    }
}

fn call_one<E, T>(client: &mut Client<T>, request: &str) -> Result<Value, Value>
where
    E: Endpoint,
    E::Request: DeserializeOwned + Serialize,
    E::Response: DeserializeOwned + Serialize,
    T: Transport<Error = FrameError<String>>,
{
    let request: E::Request =
        serde_json::from_str(request).map_err(|e| fail_with("Request", json!({ "detail": e.to_string() })))?;
    let response = client.call::<E>(&request).map_err(client_error)?;
    serde_json::to_value(response).map_err(|_| fail("Encode"))
}

macro_rules! routes {
    ($($endpoint:ty),* $(,)?) => {
        fn routed<T: Transport<Error = FrameError<String>>>(client: &mut Client<T>, path: &str, request: &str) -> Result<Value, Value> {
            match path {
                $(p if p == <$endpoint as Endpoint>::PATH => call_one::<$endpoint, _>(client, request),)*
                _ => Err(fail("UnknownPath")),
            }
        }

        pub fn endpoint_paths() -> Vec<&'static str> {
            vec![$(<$endpoint as Endpoint>::PATH),*]
        }
    };
}

routes!(
    PingEndpoint,
    GetDeviceInfo,
    GetDiagnostics,
    GetTelemetry,
    SubscribeEvents,
    GetAudioStatus,
    GetOutputSettings,
    SetOutputSettings,
    GetDspSettings,
    SetDspSettings,
    GetDacExtras,
    SetDacExtras,
    MeasureImpedance,
    GetAudioMeters,
    PlayTestTone,
    GetEq,
    SetEq,
    SelectEq,
    GetMic,
    SetMic,
    GetKeymap,
    SetKey,
    ClearKeymap,
    GetMacro,
    SetMacro,
    GetCombo,
    SetCombo,
    GetKeyConfig,
    SetKeyConfig,
    GetKeyboardStatus,
    SaveSettings,
    ResetSettings,
    GetSlots,
    Reboot,
    ConfirmBoot,
    DevUnlock,
    DevPeek,
    DevPoke,
    DevI2c,
    DevUartSend,
    DevFault,
    DevFlashTest,
    DevAudio,
);

pub fn call<L: FrameLink<Error = String>>(link: L, wire: &mut WireState, path: &str, request: &str) -> Value {
    let mut client = Client::new(FrameTransport::with_state(link, std::mem::take(wire)));
    let result = routed(&mut client, path, request);
    let events = client.transport_mut().take_events();
    *wire = client.into_transport().into_state();
    let mut value = match result {
        Ok(response) => ok(response),
        Err(error) => error,
    };
    if !events.is_empty() {
        value["topics"] = events_json(events);
    }
    value
}

pub fn update<L: FrameLink<Error = String>>(
    link: L,
    wire: &mut WireState,
    package: &[u8],
    key_hex: &str,
    progress: &mut impl Progress,
) -> Value {
    let Some(public) = key(key_hex) else {
        return fail("Key");
    };
    let package = match bundle::open(package, &public) {
        Ok(p) => p,
        Err(e) => return fail(format!("{e:?}")),
    };
    let mut client = Client::new(FrameTransport::with_state(link, std::mem::take(wire)));
    let result =
        client.install_update(&package.image, |done, total| progress.report(u64::from(done), u64::from(total), "update"));
    *wire = client.into_transport().into_state();
    match result {
        Ok(slot) => ok(serde_json::to_value(slot).unwrap_or(Value::Null)),
        Err(UpdateError::Client(e)) => client_error(e),
        Err(UpdateError::Image) => fail("Image"),
        Err(UpdateError::NoFreeSlot) => fail("NoFreeSlot"),
        Err(UpdateError::Acknowledged { expected, got }) => {
            fail_with("Acknowledged", json!({ "expected": expected, "got": got }))
        }
        Err(other) => fail_with("Update", json!({ "detail": format!("{other:?}") })),
    }
}

pub const MAX_RESPONSE_POINTS: usize = 2_048;

fn design_error(error: kumi_dsp::DesignError) -> Value {
    fail_with("Design", json!({ "detail": format!("{error:?}") }))
}

fn profile_of(json: &str) -> Result<EqProfile, Value> {
    let profile: EqProfile = serde_json::from_str(json).map_err(|e| fail_with("Request", json!({ "detail": e.to_string() })))?;
    if profile.is_valid() { Ok(profile) } else { Err(fail("InvalidProfile")) }
}

pub fn eq_response(profile: &str, sample_rate: u32, points: usize, low_hz: f64, high_hz: f64) -> Value {
    let profile = match profile_of(profile) {
        Ok(profile) => profile,
        Err(error) => return error,
    };
    let cascade = match kumi_dsp::profile_cascade(&profile, sample_rate) {
        Ok(cascade) => cascade,
        Err(error) => return design_error(error),
    };
    let high = high_hz.min(f64::from(sample_rate) / 2.0);
    if !(low_hz > 0.0 && low_hz < high && (2..=MAX_RESPONSE_POINTS).contains(&points)) {
        return fail("Range");
    }
    let preamp_db = f64::from(profile.preamp_centi_db) / 100.0;
    let frequencies: Vec<f64> = (0..points).map(|i| low_hz * (high / low_hz).powf(i as f64 / (points - 1) as f64)).collect();
    let gain_db: Vec<f64> = frequencies
        .iter()
        .map(|f| {
            let v = preamp_db + f64::from(cascade.magnitude_db(*f as f32, sample_rate));
            if v.is_finite() { v } else { -120.0 }
        })
        .collect();
    ok(json!({ "frequencies_hz": frequencies, "gain_db": gain_db, "preamp_db": preamp_db }))
}

pub fn eq_auto_preamp(profile: &str, sample_rate: u32) -> Value {
    let profile = match profile_of(profile) {
        Ok(profile) => profile,
        Err(error) => return error,
    };
    let cascade = match kumi_dsp::profile_cascade(&profile, sample_rate) {
        Ok(cascade) => cascade,
        Err(error) => return design_error(error),
    };
    let peak = cascade.peak_db(sample_rate);
    let preamp = if peak <= 0.0 {
        0
    } else {
        let cut = (peak * 100.0).ceil() as i32;
        (-cut).max(EqProfile::MIN_PREAMP_CENTI_DB as i32) as i16
    };
    ok(json!({ "preamp_centi_db": preamp, "peak_db": f64::from(peak) }))
}
