// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
use std::cell::Cell;
use std::sync::Mutex;

use kumi_client::wire::WireState;
use serde_json::Value;

static SESSION: Mutex<Option<WireState>> = Mutex::new(None);

thread_local! {
    static INSIDE: Cell<bool> = const { Cell::new(false) };
}

struct Entered;

impl Drop for Entered {
    fn drop(&mut self) {
        INSIDE.with(|inside| inside.set(false));
    }
}

pub fn exclusive(run: impl FnOnce(&mut WireState) -> Value) -> Value {
    if INSIDE.with(|inside| inside.replace(true)) {
        return crate::api::fail("Busy");
    }
    let _entered = Entered;
    let mut session = SESSION.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    run(session.get_or_insert_with(WireState::default))
}

pub fn reset() {
    if INSIDE.with(|inside| inside.get()) {
        return;
    }
    let mut session = SESSION.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    *session = None;
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
    use std::time::Duration;

    use serde_json::json;

    use super::*;

    #[test]
    fn exchanges_never_overlap_and_a_nested_call_is_refused() {
        static BUSY: AtomicBool = AtomicBool::new(false);
        static OVERLAPS: AtomicU32 = AtomicU32::new(0);
        let workers: Vec<_> = (0..4)
            .map(|_| {
                std::thread::spawn(|| {
                    for _ in 0..20 {
                        exclusive(|wire| {
                            if BUSY.swap(true, Ordering::SeqCst) {
                                OVERLAPS.fetch_add(1, Ordering::SeqCst);
                            }
                            wire.sequence = wire.sequence.wrapping_add(1);
                            std::thread::sleep(Duration::from_micros(200));
                            BUSY.store(false, Ordering::SeqCst);
                            Value::Null
                        });
                    }
                })
            })
            .collect();
        workers.into_iter().for_each(|worker| worker.join().unwrap());
        assert_eq!(OVERLAPS.load(Ordering::SeqCst), 0);
        assert!(exclusive(|wire| json!(wire.sequence)).as_u64().unwrap() >= 80);
        let nested = exclusive(|_| exclusive(|_| Value::Null));
        assert_eq!(nested["err"]["kind"], "Busy");
        let caught = std::panic::catch_unwind(|| exclusive(|_| panic!("handler failed")));
        assert!(caught.is_err());
        assert_eq!(exclusive(|_| json!(1)), json!(1));
    }
}
