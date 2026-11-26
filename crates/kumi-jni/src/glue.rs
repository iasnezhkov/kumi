// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use jni::JNIEnv;
use jni::objects::{JByteArray, JClass, JObject, JString, JValue};
use jni::sys::{jboolean, jbyteArray, jdouble, jint, jlong, jstring};
use kumi_client::wire::FrameLink;
use serde_json::Value;

use crate::api::{self, Progress};
use crate::session::{self, exclusive};
use crate::sim::{self, Mode, Sim};

fn clear_pending_exception(env: &mut JNIEnv) {
    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_clear();
    }
}

struct JavaLink<'a, 'l> {
    env: JNIEnv<'l>,
    link: &'a JObject<'l>,
}

impl JavaLink<'_, '_> {
    fn clear_exception(&mut self) {
        clear_pending_exception(&mut self.env);
    }

    fn send_bytes(&mut self, data: &[u8]) -> Result<(), String> {
        let array = match self.env.byte_array_from_slice(data) {
            Ok(array) => array,
            Err(e) => {
                self.clear_exception();
                return Err(e.to_string());
            }
        };
        let sent = self.env.call_method(self.link, "send", "([B)Z", &[JValue::Object(&array)]).and_then(|v| v.z());
        let _ = self.env.delete_local_ref(array);
        match sent {
            Ok(true) => Ok(()),
            Ok(false) => Err("send failed".to_string()),
            Err(e) => {
                self.clear_exception();
                Err(e.to_string())
            }
        }
    }

    fn read_method(&mut self, method: &str) -> Result<Option<Vec<u8>>, String> {
        let value = match self.env.call_method(self.link, method, "()[B", &[]).and_then(|v| v.l()) {
            Ok(v) => v,
            Err(e) => {
                self.clear_exception();
                return Err(e.to_string());
            }
        };
        if value.is_null() {
            return Ok(None);
        }
        let array = JByteArray::from(value);
        let bytes = self.env.convert_byte_array(&array).map_err(|e| e.to_string());
        let _ = self.env.delete_local_ref(array);
        if bytes.is_err() {
            self.clear_exception();
        }
        bytes.map(Some)
    }

    fn receive_bytes(&mut self) -> Result<Vec<u8>, String> {
        self.read_method("receive")?.ok_or_else(|| "timeout".to_string())
    }
}

impl FrameLink for JavaLink<'_, '_> {
    type Error = String;

    fn send(&mut self, frame: &[u8]) -> Result<(), String> {
        self.send_bytes(frame)
    }

    fn receive(&mut self) -> Result<Vec<u8>, String> {
        self.receive_bytes()
    }

    fn try_receive(&mut self) -> Result<Option<Vec<u8>>, String> {
        self.read_method("tryReceive")
    }
}

impl kumi_buxx::Link for JavaLink<'_, '_> {
    fn transfer(&mut self, packet: &[u8]) -> Result<Vec<u8>, kumi_buxx::Error> {
        self.send_bytes(packet).map_err(kumi_buxx::Error::Link)?;
        self.receive_bytes().map_err(kumi_buxx::Error::Link)
    }
}

struct SimLink(Arc<Mutex<Sim>>);

impl kumi_buxx::Link for SimLink {
    fn transfer(&mut self, packet: &[u8]) -> Result<Vec<u8>, kumi_buxx::Error> {
        let mut sim = self.0.lock().map_err(|_| kumi_buxx::Error::Link("simulator unavailable".into()))?;
        if !sim.send(packet) {
            return Err(kumi_buxx::Error::Link("simulator refused the packet".into()));
        }
        sim.receive().ok_or_else(|| kumi_buxx::Error::Link("timeout".into()))
    }
}

struct JavaProgress<'a, 'l> {
    env: JNIEnv<'l>,
    sink: &'a JObject<'l>,
    last: Option<(u64, u64)>,
}

impl Progress for JavaProgress<'_, '_> {
    fn report(&mut self, done: u64, total: u64, label: &str) {
        if self.sink.is_null() || self.last == Some((done, total)) {
            return;
        }
        self.last = Some((done, total));
        let Ok(text) = self.env.new_string(label) else {
            clear_pending_exception(&mut self.env);
            return;
        };
        let args = [JValue::Long(done as i64), JValue::Long(total as i64), JValue::Object(&text)];
        if self.env.call_method(self.sink, "progress", "(JJLjava/lang/String;)V", &args).is_err() {
            clear_pending_exception(&mut self.env);
        }
        let _ = self.env.delete_local_ref(text);
    }
}

fn link<'a, 'l>(env: &JNIEnv<'l>, link: &'a JObject<'l>) -> JavaLink<'a, 'l> {
    JavaLink { env: unsafe { env.unsafe_clone() }, link }
}

fn progress<'a, 'l>(env: &JNIEnv<'l>, sink: &'a JObject<'l>) -> JavaProgress<'a, 'l> {
    JavaProgress { env: unsafe { env.unsafe_clone() }, sink, last: None }
}

fn text(env: &mut JNIEnv, value: &JString) -> String {
    match env.get_string(value) {
        Ok(s) => s.into(),
        Err(_) => {
            clear_pending_exception(env);
            String::new()
        }
    }
}

fn bytes(env: &mut JNIEnv, value: &JByteArray) -> Vec<u8> {
    match env.convert_byte_array(value) {
        Ok(b) => b,
        Err(_) => {
            clear_pending_exception(env);
            Vec::new()
        }
    }
}

fn answer(env: &mut JNIEnv, run: impl FnOnce(&mut JNIEnv) -> Value) -> jstring {
    let value = match catch_unwind(AssertUnwindSafe(|| run(env))) {
        Ok(value) => value,
        Err(_) => api::fail("Panic"),
    };
    clear_pending_exception(env);
    env.new_string(value.to_string()).map(|s| s.into_raw()).unwrap_or_else(|_| {
        clear_pending_exception(env);
        std::ptr::null_mut()
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_mindone_snapcase_kumi_KumiNative_info<'l>(mut env: JNIEnv<'l>, _class: JClass<'l>) -> jstring {
    answer(&mut env, |_| api::info())
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_mindone_snapcase_kumi_KumiNative_installGates<'l>(
    mut env: JNIEnv<'l>,
    _class: JClass<'l>,
) -> jstring {
    answer(&mut env, |_| api::install_gates(&kumi_install::GateEvidence::recorded()))
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_mindone_snapcase_kumi_KumiNative_resetSession<'l>(_env: JNIEnv<'l>, _class: JClass<'l>) {
    let _ = catch_unwind(session::reset);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_mindone_snapcase_kumi_KumiNative_openPackage<'l>(
    mut env: JNIEnv<'l>,
    _class: JClass<'l>,
    data: JByteArray<'l>,
    key: JString<'l>,
) -> jstring {
    answer(&mut env, |env| {
        let data = bytes(env, &data);
        let key = text(env, &key);
        api::open_package(&data, &key)
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_mindone_snapcase_kumi_KumiNative_identify<'l>(
    mut env: JNIEnv<'l>,
    _class: JClass<'l>,
    port: JObject<'l>,
) -> jstring {
    answer(&mut env, |env| exclusive(|_| api::identify(link(env, &port))))
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_mindone_snapcase_kumi_KumiNative_backup<'l>(
    mut env: JNIEnv<'l>,
    _class: JClass<'l>,
    port: JObject<'l>,
    path: JString<'l>,
    sink: JObject<'l>,
) -> jstring {
    answer(&mut env, |env| {
        let path = text(env, &path);
        exclusive(|_| api::backup(link(env, &port), &path, &mut progress(env, &sink)))
    })
}

#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub extern "system" fn Java_org_mindone_snapcase_kumi_KumiNative_install<'l>(
    mut env: JNIEnv<'l>,
    _class: JClass<'l>,
    port: JObject<'l>,
    backup: JString<'l>,
    backup_sha: JString<'l>,
    package: JByteArray<'l>,
    key: JString<'l>,
    from: jint,
    sink: JObject<'l>,
) -> jstring {
    answer(&mut env, |env| {
        let backup = text(env, &backup);
        let backup_sha = text(env, &backup_sha);
        let package = bytes(env, &package);
        let key = text(env, &key);
        let from = usize::try_from(from).unwrap_or(0);
        let gates = kumi_install::GateEvidence::recorded();
        exclusive(|_| {
            api::install(link(env, &port), &backup, &backup_sha, &package, &key, &gates, from, &mut progress(env, &sink))
        })
    })
}

#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub extern "system" fn Java_org_mindone_snapcase_kumi_KumiNative_simInstall<'l>(
    mut env: JNIEnv<'l>,
    _class: JClass<'l>,
    handle: jlong,
    backup: JString<'l>,
    backup_sha: JString<'l>,
    package: JByteArray<'l>,
    key: JString<'l>,
    from: jint,
    sink: JObject<'l>,
) -> jstring {
    answer(&mut env, |env| {
        let backup = text(env, &backup);
        let backup_sha = text(env, &backup_sha);
        let package = bytes(env, &package);
        let key = text(env, &key);
        let from = usize::try_from(from).unwrap_or(0);
        let Some(sim) = sim_get(handle) else { return api::fail("Simulator") };
        let gates = kumi_install::GateEvidence::for_simulation();
        exclusive(|_| api::install(SimLink(sim), &backup, &backup_sha, &package, &key, &gates, from, &mut progress(env, &sink)))
    })
}

#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub extern "system" fn Java_org_mindone_snapcase_kumi_KumiNative_restoreStock<'l>(
    mut env: JNIEnv<'l>,
    _class: JClass<'l>,
    port: JObject<'l>,
    backup: JString<'l>,
    backup_sha: JString<'l>,
    exact: jboolean,
    from: jint,
    sink: JObject<'l>,
) -> jstring {
    answer(&mut env, |env| {
        let backup = text(env, &backup);
        let backup_sha = text(env, &backup_sha);
        let from = usize::try_from(from).unwrap_or(0);
        exclusive(|_| api::restore_stock(link(env, &port), &backup, &backup_sha, exact != 0, from, &mut progress(env, &sink)))
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_mindone_snapcase_kumi_KumiNative_rebootFromService<'l>(
    mut env: JNIEnv<'l>,
    _class: JClass<'l>,
    port: JObject<'l>,
) -> jstring {
    answer(&mut env, |env| exclusive(|_| api::reboot_from_service(link(env, &port))))
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_mindone_snapcase_kumi_KumiNative_call<'l>(
    mut env: JNIEnv<'l>,
    _class: JClass<'l>,
    port: JObject<'l>,
    path: JString<'l>,
    request: JString<'l>,
) -> jstring {
    answer(&mut env, |env| {
        let path = text(env, &path);
        let request = text(env, &request);
        exclusive(|wire| api::call(link(env, &port), wire, &path, &request))
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_mindone_snapcase_kumi_KumiNative_pollTopics<'l>(
    mut env: JNIEnv<'l>,
    _class: JClass<'l>,
    port: JObject<'l>,
    max_frames: jint,
) -> jstring {
    answer(&mut env, |env| {
        let max_frames = usize::try_from(max_frames).unwrap_or(0);
        exclusive(|wire| api::poll_topics(link(env, &port), wire, max_frames))
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_mindone_snapcase_kumi_KumiNative_update<'l>(
    mut env: JNIEnv<'l>,
    _class: JClass<'l>,
    port: JObject<'l>,
    package: JByteArray<'l>,
    key: JString<'l>,
    sink: JObject<'l>,
) -> jstring {
    answer(&mut env, |env| {
        let package = bytes(env, &package);
        let key = text(env, &key);
        exclusive(|wire| api::update(link(env, &port), wire, &package, &key, &mut progress(env, &sink)))
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_mindone_snapcase_kumi_KumiNative_eqResponse<'l>(
    mut env: JNIEnv<'l>,
    _class: JClass<'l>,
    profile: JString<'l>,
    sample_rate: jint,
    points: jint,
    low_hz: jdouble,
    high_hz: jdouble,
) -> jstring {
    answer(&mut env, |env| {
        let profile = text(env, &profile);
        let points = usize::try_from(points).unwrap_or(0);
        api::eq_response(&profile, u32::try_from(sample_rate).unwrap_or(0), points, low_hz, high_hz)
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_mindone_snapcase_kumi_KumiNative_eqAutoPreamp<'l>(
    mut env: JNIEnv<'l>,
    _class: JClass<'l>,
    profile: JString<'l>,
    sample_rate: jint,
) -> jstring {
    answer(&mut env, |env| {
        let profile = text(env, &profile);
        api::eq_auto_preamp(&profile, u32::try_from(sample_rate).unwrap_or(0))
    })
}

fn guarded<T>(fallback: T, run: impl FnOnce() -> T) -> T {
    catch_unwind(AssertUnwindSafe(run)).unwrap_or(fallback)
}

fn sims() -> &'static Mutex<HashMap<i64, Arc<Mutex<Sim>>>> {
    static TABLE: OnceLock<Mutex<HashMap<i64, Arc<Mutex<Sim>>>>> = OnceLock::new();
    TABLE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn sim_get(handle: jlong) -> Option<Arc<Mutex<Sim>>> {
    if handle <= 0 {
        return None;
    }
    sims().lock().unwrap_or_else(|p| p.into_inner()).get(&handle).cloned()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_mindone_snapcase_kumi_KumiNative_simCreate<'l>(_env: JNIEnv<'l>, _class: JClass<'l>) -> jlong {
    static NEXT_ID: AtomicI64 = AtomicI64::new(1);
    guarded(0, || {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        sims().lock().unwrap_or_else(|p| p.into_inner()).insert(id, Arc::new(Mutex::new(Sim::new())));
        id
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_mindone_snapcase_kumi_KumiNative_simDestroy<'l>(
    _env: JNIEnv<'l>,
    _class: JClass<'l>,
    handle: jlong,
) {
    if handle > 0 {
        guarded((), || {
            let _ = sims().lock().unwrap_or_else(|p| p.into_inner()).remove(&handle);
        });
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_mindone_snapcase_kumi_KumiNative_simMode<'l>(
    _env: JNIEnv<'l>,
    _class: JClass<'l>,
    handle: jlong,
) -> jint {
    guarded(-1, || sim_get(handle).and_then(|s| s.lock().ok().map(|s| s.mode() as jint)).unwrap_or(-1))
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_mindone_snapcase_kumi_KumiNative_simSetMode<'l>(
    _env: JNIEnv<'l>,
    _class: JClass<'l>,
    handle: jlong,
    mode: jint,
) {
    guarded((), || {
        if let (Some(sim), Some(mode)) = (sim_get(handle), Mode::from_index(mode))
            && let Ok(mut sim) = sim.lock()
        {
            sim.set_mode(mode);
        }
    });
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_mindone_snapcase_kumi_KumiNative_simSend<'l>(
    mut env: JNIEnv<'l>,
    _class: JClass<'l>,
    handle: jlong,
    data: JByteArray<'l>,
) -> jboolean {
    let data = bytes(&mut env, &data);
    u8::from(guarded(false, || sim_get(handle).and_then(|s| s.lock().ok().map(|mut s| s.send(&data))).unwrap_or(false)))
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_mindone_snapcase_kumi_KumiNative_simReceive<'l>(
    mut env: JNIEnv<'l>,
    _class: JClass<'l>,
    handle: jlong,
) -> jbyteArray {
    let frame = guarded(None, || sim_get(handle).and_then(|s| s.lock().ok().and_then(|mut s| s.receive())));
    clear_pending_exception(&mut env);
    match frame {
        Some(frame) => env.byte_array_from_slice(&frame).map(|a| a.into_raw()).unwrap_or_else(|_| {
            clear_pending_exception(&mut env);
            std::ptr::null_mut()
        }),
        None => std::ptr::null_mut(),
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_mindone_snapcase_kumi_KumiNative_simPublicKey<'l>(
    mut env: JNIEnv<'l>,
    _class: JClass<'l>,
) -> jstring {
    clear_pending_exception(&mut env);
    env.new_string(sim::demo_public_hex()).map(|s| s.into_raw()).unwrap_or_else(|_| {
        clear_pending_exception(&mut env);
        std::ptr::null_mut()
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_mindone_snapcase_kumi_KumiNative_simPackage<'l>(
    mut env: JNIEnv<'l>,
    _class: JClass<'l>,
    major: jint,
    minor: jint,
    patch: jint,
    generation: jint,
) -> jbyteArray {
    let version = [major as u8, minor as u8, patch as u8];
    let Some(package) = guarded(None, || Some(sim::demo_package(version, generation.max(0) as u32))) else {
        return std::ptr::null_mut();
    };
    clear_pending_exception(&mut env);
    env.byte_array_from_slice(&package).map(|a| a.into_raw()).unwrap_or_else(|_| {
        clear_pending_exception(&mut env);
        std::ptr::null_mut()
    })
}
