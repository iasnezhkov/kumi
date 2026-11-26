// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
use kumi_install::GateEvidence;
use std::cell::RefCell;
use std::rc::Rc;

use kumi_client::wire::{FrameLink, WireState};
use kumi_icd::{ENDPOINT_LIST, UNSERVED_PATHS};
use kumi_jni::api;
use kumi_jni::sim::{self, Mode, Sim};
use serde_json::{Value, json};

#[derive(Clone)]
struct Port(Rc<RefCell<Sim>>);

impl FrameLink for Port {
    type Error = String;

    fn send(&mut self, frame: &[u8]) -> Result<(), String> {
        if self.0.borrow_mut().send(frame) { Ok(()) } else { Err("no device".into()) }
    }

    fn receive(&mut self) -> Result<Vec<u8>, String> {
        self.0.borrow_mut().receive().ok_or_else(|| "timeout".to_string())
    }

    fn try_receive(&mut self) -> Result<Option<Vec<u8>>, String> {
        Ok(self.0.borrow_mut().receive())
    }
}

impl kumi_buxx::Link for Port {
    fn transfer(&mut self, packet: &[u8]) -> Result<Vec<u8>, kumi_buxx::Error> {
        FrameLink::send(self, packet).map_err(kumi_buxx::Error::Link)?;
        FrameLink::receive(self).map_err(kumi_buxx::Error::Link)
    }
}

fn quiet() -> impl FnMut(u64, u64, &str) {
    |_, _, _| {}
}

fn ok(value: &Value) -> &Value {
    assert!(value.get("err").is_none(), "unexpected error: {value}");
    &value["ok"]
}

struct Session {
    port: Port,
    wire: WireState,
}

impl Session {
    fn new(sim: Sim) -> Self {
        Self { port: Port(Rc::new(RefCell::new(sim))), wire: WireState::default() }
    }

    fn call(&mut self, path: &str, request: Value) -> Value {
        api::call(self.port.clone(), &mut self.wire, path, &request.to_string())
    }

    fn poll(&mut self, max_frames: usize) -> Value {
        api::poll_topics(self.port.clone(), &mut self.wire, max_frames)
    }

    fn mode(&self) -> Mode {
        self.port.0.borrow().mode()
    }

    fn slots(&mut self) -> Value {
        ok(&self.call("system/slots", Value::Null)).clone()
    }

    fn installed(name: &str, version: [u8; 3]) -> Self {
        let s = Session::new(Sim::new());
        s.port.0.borrow_mut().set_mode(Mode::Service);
        let path = temp(name);
        let sha = ok(&api::backup(s.port.clone(), &path, &mut quiet()))["sha256"].as_str().unwrap().to_string();
        let package = sim::demo_package(version, 1);
        ok(&api::install(
            s.port.clone(),
            &path,
            &sha,
            &package,
            &sim::demo_public_hex(),
            &GateEvidence::for_simulation(),
            0,
            &mut quiet(),
        ));
        ok(&api::reboot_from_service(s.port.clone()));
        assert_eq!(s.mode(), Mode::Kumi);
        s
    }
}

fn temp(name: &str) -> String {
    let dir = std::env::temp_dir().join(format!("kumi-jni-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("backup.bin").to_string_lossy().into_owned()
}

#[test]
fn install_use_update_and_return_to_stock_through_the_bridge() {
    let mut s = Session::new(Sim::new());
    let key = sim::demo_public_hex();
    let package = sim::demo_package([0, 1, 0], 1);
    assert_eq!(ok(&api::open_package(&package, &key))["version"], "0.1.0");

    assert!(api::identify(s.port.clone()).get("err").is_some(), "a stock case does not speak the boot ROM protocol");
    s.port.0.borrow_mut().set_mode(Mode::Service);
    assert_eq!(ok(&api::identify(s.port.clone()))["chipMatches"], true);

    let path = temp("flow");
    let backup = api::backup(s.port.clone(), &path, &mut quiet());
    let backup = ok(&backup).clone();
    assert_eq!(backup["stock"], "Head");
    let sha = backup["sha256"].as_str().unwrap().to_string();
    let original = std::fs::read(&path).unwrap();

    let mut steps = Vec::new();
    let installed = api::install(
        s.port.clone(),
        &path,
        &sha,
        &package,
        &key,
        &GateEvidence::for_simulation(),
        0,
        &mut |done: u64, total: u64, _: &str| steps.push((done, total)),
    );
    assert_eq!(ok(&installed)["boots"], "Kumi");
    assert!(steps.len() > 10 && steps.last().unwrap().0 == steps.last().unwrap().1);
    ok(&api::reboot_from_service(s.port.clone()));
    assert_eq!(s.mode(), Mode::Kumi);

    let info = s.call("device/info", Value::Null);
    assert_eq!(ok(&info)["firmware"], json!({ "major": 0, "minor": 1, "patch": 0 }));
    assert_eq!(ok(&info)["capabilities"]["firmware_update"], true);
    assert_eq!(ok(&info)["running_from_ram"], false);
    assert_eq!(s.slots()["slots"][0]["state"], "Trial", "the first boot after an install is a trial");
    assert_eq!(ok(&s.call("postcard-rpc/ping", json!(7))), &json!(7));

    let profile = json!({ "preamp_centi_db": -300, "bands": [
        { "kind": "LowShelf", "frequency_hz": 90, "q_milli": 707, "gain_centi_db": 450 },
        { "kind": "Peak", "frequency_hz": 3000, "q_milli": 1400, "gain_centi_db": -200 },
    ]});
    assert_eq!(ok(&s.call("audio/eq/set", json!({ "slot": 1, "profile": profile }))), &json!({ "Ok": null }));
    assert_eq!(ok(&s.call("audio/eq/get", json!(1))), &json!({ "Ok": profile }));
    assert_eq!(ok(&s.call("audio/eq/get", json!(9))), &json!({ "Err": "InvalidArgument" }));
    assert_eq!(ok(&s.call("audio/eq/get", json!(2))), &json!({ "Ok": null }));
    assert_eq!(ok(&s.call("audio/eq/select", json!(3))), &json!({ "Err": "WrongState" }));
    assert_eq!(
        ok(&s.call(
            "keys/map/set",
            json!({ "layer": 0, "source": 4, "action": { "KeyWithModifiers": { "usage": 5, "modifiers": 2 } } })
        )),
        &json!({ "Ok": null })
    );
    assert_eq!(ok(&s.call("keys/map/get", json!({ "layer": 0, "start": 0 })))["Ok"]["entries"][0]["source"], 4);

    let next = sim::demo_package([0, 2, 0], 2);
    assert_eq!(api::update(s.port.clone(), &mut s.wire, &next, &key, &mut quiet())["err"]["detail"], "WrongState");
    assert_eq!(ok(&s.call("system/confirm", Value::Null)), &json!({ "Ok": null }));
    let updated = api::update(s.port.clone(), &mut s.wire, &next, &key, &mut quiet());
    assert_eq!(ok(&updated)["slot"], "B");
    assert_eq!(ok(&updated)["state"], "Trial");
    ok(&s.call("system/reboot", json!("Normal")));
    let slots = s.slots();
    assert_eq!(slots["booted"], "B");
    assert_eq!(slots["slots"][1]["state"], "Trial");
    assert_eq!(ok(&s.call("system/confirm", Value::Null)), &json!({ "Ok": null }));
    assert_eq!(ok(&s.call("device/info", Value::Null))["firmware"]["minor"], 2);
    assert_eq!(s.slots()["slots"][0]["state"], "Confirmed", "confirming B keeps A as the rollback image");
    assert_eq!(ok(&s.call("audio/eq/get", json!(1))), &json!({ "Ok": profile }), "settings survive the update reboot");
    ok(&s.call("audio/eq/select", json!(1)));
    assert_eq!(ok(&s.call("audio/status", Value::Null))["eq_active"], 1);

    ok(&s.call("system/reboot", json!("BootRom")));
    assert_eq!(s.mode(), Mode::Service);
    let other_case = Port(Rc::new(RefCell::new(Sim::new())));
    other_case.0.borrow_mut().set_mode(Mode::Service);
    let foreign = temp("foreign");
    let mut foreign_flash = std::fs::read(&path).unwrap();
    foreign_flash[0x100] ^= 0xFF;
    std::fs::write(&foreign, &foreign_flash).unwrap();
    let foreign_sha = api::hex(&kumi_install::sha256(&foreign_flash));
    assert_eq!(api::restore_stock(other_case, &foreign, &foreign_sha, false, 0, &mut quiet())["err"]["kind"], "NoStockInBackup");
    let restored = api::restore_stock(s.port.clone(), &path, &sha, true, 0, &mut quiet());
    assert_eq!(ok(&restored)["boots"], "Stock");
    ok(&api::reboot_from_service(s.port.clone()));
    assert_eq!(s.mode(), Mode::Stock);
    assert_eq!(s.port.0.borrow().flash().unwrap(), original.as_slice());
}

#[test]
fn the_bridge_refuses_what_it_cannot_prove_safe() {
    let port = Port(Rc::new(RefCell::new(Sim::new())));
    port.0.borrow_mut().set_mode(Mode::Service);
    let key = sim::demo_public_hex();
    let path = temp("refuse");
    let backup = api::backup(port.clone(), &path, &mut quiet());
    let sha = ok(&backup)["sha256"].as_str().unwrap().to_string();
    let package = sim::demo_package([0, 1, 0], 1);

    let other = api::hex(ed25519_dalek::SigningKey::from_bytes(&[1; 32]).verifying_key().as_bytes());
    assert_eq!(
        api::install(port.clone(), &path, &sha, &package, &other, &GateEvidence::for_simulation(), 0, &mut quiet())["err"]["kind"],
        "Signature"
    );
    assert_eq!(api::open_package(&package, "zz")["err"]["kind"], "Key");
    let wrong_sha = "00".repeat(32);
    assert_eq!(
        api::install(port.clone(), &path, &wrong_sha, &package, &key, &GateEvidence::for_simulation(), 0, &mut quiet())["err"]["kind"],
        "BackupChanged"
    );
    let mut tampered = package.clone();
    tampered[100] ^= 1;
    assert_eq!(
        api::install(port.clone(), &path, &sha, &tampered, &key, &GateEvidence::for_simulation(), 0, &mut quiet())["err"]["kind"],
        "Signature"
    );
    assert_eq!(port.0.borrow().flash().unwrap()[..0x4_0000], kumi_install::sample::stock()[..]);
}

#[test]
fn installing_over_a_kumi_backup_is_refused_but_restoring_uses_the_stock_copy() {
    let port = Port(Rc::new(RefCell::new(Sim::new())));
    port.0.borrow_mut().set_mode(Mode::Service);
    let key = sim::demo_public_hex();
    let first = temp("first");
    let sha = ok(&api::backup(port.clone(), &first, &mut quiet()))["sha256"].as_str().unwrap().to_string();
    let package = sim::demo_package([0, 1, 0], 1);
    ok(&api::install(port.clone(), &first, &sha, &package, &key, &GateEvidence::for_simulation(), 0, &mut quiet()));

    let second = temp("second");
    let after = api::backup(port.clone(), &second, &mut quiet());
    assert_eq!(ok(&after)["stock"], "Copy");
    let sha2 = ok(&after)["sha256"].as_str().unwrap().to_string();
    assert_eq!(
        api::install(port.clone(), &second, &sha2, &package, &key, &GateEvidence::for_simulation(), 0, &mut quiet())["err"]["kind"],
        "NotStock"
    );
    assert_eq!(api::restore_stock(port.clone(), &second, &sha2, true, 0, &mut quiet())["err"]["kind"], "ExactNeedsStockHead");
    let mut other_stock = kumi_install::sample::stock();
    other_stock[0x1000] ^= 0x01;
    let stranger = Port(Rc::new(RefCell::new(Sim::with_flash(&other_stock))));
    stranger.0.borrow_mut().set_mode(Mode::Service);
    assert_eq!(api::restore_stock(stranger.clone(), &second, &sha2, false, 0, &mut quiet())["err"]["kind"], "StockMismatch");
    assert_eq!(stranger.0.borrow().flash().unwrap()[..0x4_0000], other_stock[..]);
    assert_eq!(ok(&api::restore_stock(port.clone(), &second, &sha2, false, 0, &mut quiet()))["boots"], "Stock");
    assert_eq!(port.0.borrow().flash().unwrap()[..0x4_0000], kumi_install::sample::stock()[..]);
}

#[test]
fn calls_report_unknown_paths_bad_requests_and_a_missing_device() {
    let mut s = Session::new(Sim::new());
    assert_eq!(s.call("device/info", Value::Null)["err"]["kind"], "Link");
    s.port.0.borrow_mut().set_mode(Mode::Kumi);
    assert_eq!(s.call("no/such", Value::Null)["err"]["kind"], "UnknownPath");
    assert_eq!(api::call(s.port.clone(), &mut s.wire, "audio/mic/set", "{\"gain\": 1}")["err"]["kind"], "Request");
    let info = s.call("device/info", Value::Null);
    assert_eq!(ok(&info)["running_from_ram"], true);
    assert_eq!(ok(&info)["capabilities"]["persistent_settings"], false);
    assert_eq!(ok(&s.call("settings/save", Value::Null)), &json!({ "Err": "Unsupported" }));
    assert_eq!(ok(&s.poll(4)), &json!([]));
}

#[test]
fn the_bridge_routes_every_served_endpoint_and_nothing_else() {
    let routed = api::endpoint_paths();
    for (path, _, _) in ENDPOINT_LIST.endpoints {
        let elsewhere = path.starts_with("update/") || UNSERVED_PATHS.contains(path);
        assert_eq!(routed.contains(path), !elsewhere, "{path}");
    }
    assert!(routed.contains(&"postcard-rpc/ping"));
}

#[test]
fn diagnostics_and_the_event_stream_cross_the_bridge() {
    let mut s = Session::installed("diagnostics", [0, 1, 0]);
    let diagnostics = ok(&s.call("system/diagnostics", Value::Null))["Ok"].clone();
    assert_eq!(diagnostics["boot_count"], 1);
    assert_eq!(diagnostics["safe_mode"], false);
    assert_eq!(diagnostics["last_crash"], Value::Null);

    let reply = s.call("events/subscribe", json!({ "keys": true, "log": true, "audio": true }));
    assert_eq!(ok(&reply)["Ok"], json!({ "keys": true, "log": true, "audio": false, "uart": false }));
    assert_eq!(reply["topics"][0]["path"], "log");
    assert_eq!(reply["topics"][0]["data"]["text"], "Event stream started");
    assert_eq!(ok(&s.poll(8)), &json!([]));
    let off = s.call("events/subscribe", json!({}));
    assert_eq!(ok(&off)["Ok"], json!({ "keys": false, "log": false, "audio": false, "uart": false }));
    assert!(off.get("topics").is_none());
}

#[test]
fn an_unconfirmed_update_rolls_back_after_its_trial_boots() {
    let mut s = Session::installed("rollback", [0, 1, 0]);
    ok(&s.call("system/confirm", Value::Null));
    let next = sim::demo_package([0, 2, 0], 2);
    ok(&api::update(s.port.clone(), &mut s.wire, &next, &sim::demo_public_hex(), &mut quiet()));
    for _ in 0..3 {
        ok(&s.call("system/reboot", json!("Normal")));
        assert_eq!(s.slots()["booted"], "B");
    }
    ok(&s.call("system/reboot", json!("Normal")));
    let slots = s.slots();
    assert_eq!(slots["booted"], "A", "{slots}");
    assert_eq!(slots["slots"][0]["state"], "Confirmed");
    assert_eq!(slots["slots"][1]["state"], "Invalid");
    assert_eq!(ok(&s.call("device/info", Value::Null))["firmware"]["minor"], 1);
    assert_eq!(ok(&s.call("system/reboot", json!("OtherSlot"))), &json!({ "Err": "WrongState" }));
}
