// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
use kumi_control::{AutoSave, Controls, SaveTrigger};
use kumi_icd::{DacFilter, MicSettings, OutputSettings};
use kumi_settings::StoredSettings;

fn output(limit: i16) -> OutputSettings {
    OutputSettings { filter: DacFilter::SlowLowLatency, volume_limit_centi_db: limit, balance_centi_db: 0, dsd_direct: false }
}

#[test]
fn nothing_pending_is_never_due() {
    let auto = AutoSave::new(100);
    assert!(!auto.pending());
    assert!(!auto.due(0, SaveTrigger::Tick));
    assert!(!auto.due(1_000, SaveTrigger::Sleeping));
    assert!(!auto.due(1_000, SaveTrigger::LinkDown));
}

#[test]
fn a_change_persists_only_after_it_settles() {
    let mut auto = AutoSave::new(100);
    auto.record(50);
    assert!(auto.pending());
    assert!(!auto.due(149, SaveTrigger::Tick));
    assert!(auto.due(150, SaveTrigger::Tick));
    assert!(auto.due(200, SaveTrigger::Tick));
    auto.saved();
    assert!(!auto.pending());
    assert!(!auto.due(200, SaveTrigger::Tick));
}

#[test]
fn sleep_or_link_down_flushes_at_once() {
    let mut auto = AutoSave::new(10_000);
    auto.record(50);
    assert!(!auto.due(60, SaveTrigger::Tick));
    assert!(auto.due(60, SaveTrigger::Sleeping));
    assert!(auto.due(60, SaveTrigger::LinkDown));
}

#[test]
fn a_burst_of_changes_coalesces_into_one_window() {
    let mut auto = AutoSave::new(100);
    auto.record(50);
    auto.record(70);
    auto.record(90);
    assert!(!auto.due(149, SaveTrigger::Tick));
    assert!(!auto.due(189, SaveTrigger::Tick));
    assert!(auto.due(190, SaveTrigger::Tick));
    auto.record(180);
    auto.record(270);
    auto.record(360);
    auto.record(450);
    auto.record(540);
    assert!(!auto.due(549, SaveTrigger::Tick));
    assert!(auto.due(550, SaveTrigger::Tick));
}

#[test]
fn the_tick_counter_may_wrap() {
    let mut auto = AutoSave::new(100);
    auto.record(u32::MAX - 20);
    assert!(!auto.due(u32::MAX - 10, SaveTrigger::Tick));
    assert!(auto.due(79, SaveTrigger::Tick));
}

fn tick(controls: &mut Controls, auto: &mut AutoSave, now: u32, trigger: SaveTrigger) -> Option<StoredSettings> {
    if controls.dirty() {
        auto.record(now);
        controls.clear_dirty();
    }
    if auto.due(now, trigger) {
        let snapshot = controls.settings().clone();
        auto.saved();
        return Some(snapshot);
    }
    None
}

#[test]
fn a_firmware_loop_saves_once_without_a_save_button() {
    let mut controls = Controls::new(StoredSettings::default());
    let mut auto = AutoSave::new(100);

    controls.set_output(output(-100)).unwrap();
    assert!(tick(&mut controls, &mut auto, 10, SaveTrigger::Tick).is_none());
    controls.set_output(output(-200)).unwrap();
    assert!(tick(&mut controls, &mut auto, 30, SaveTrigger::Tick).is_none());
    controls.set_mic(MicSettings { gain_centi_db: 600 }).unwrap();
    assert!(tick(&mut controls, &mut auto, 60, SaveTrigger::Tick).is_none());

    assert!(tick(&mut controls, &mut auto, 159, SaveTrigger::Tick).is_none());
    let persisted = tick(&mut controls, &mut auto, 160, SaveTrigger::Tick).expect("the burst persists once it settles");
    assert_eq!(persisted.output.volume_limit_centi_db, -200);
    assert_eq!(persisted.mic.gain_centi_db, 600);

    assert!(tick(&mut controls, &mut auto, 250, SaveTrigger::Tick).is_none());

    controls.set_output(output(-300)).unwrap();
    let on_sleep = tick(&mut controls, &mut auto, 260, SaveTrigger::Sleeping).expect("sleep flushes before power is lost");
    assert_eq!(on_sleep.output.volume_limit_centi_db, -300);
}
