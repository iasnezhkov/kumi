// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
use heapless::Vec;
use kumi_control::Controls;
use kumi_icd::{
    CaseError, DacFilter, EqBand, EqProfile, EqSlotUpdate, FilterKind, KeyAction, KeymapEntry, KeymapPage, MicSettings,
    OutputSettings, SettingsScope,
};
use kumi_settings::StoredSettings;

fn controls() -> Controls {
    Controls::new(StoredSettings::default())
}

fn band(freq: u32, gain: i16) -> EqBand {
    EqBand { kind: FilterKind::Peak, frequency_hz: freq, q_milli: 1_000, gain_centi_db: gain }
}

fn profile(freq: u32, gain: i16) -> EqProfile {
    let mut bands = Vec::new();
    bands.push(band(freq, gain)).unwrap();
    EqProfile { preamp_centi_db: -100, bands }
}

fn key(layer: u8, source: u8, action: KeyAction) -> KeymapEntry {
    KeymapEntry { layer, source, action }
}

#[test]
fn output_and_mic_round_trip_and_mark_dirty() {
    let mut c = controls();
    assert!(!c.dirty());
    let wanted = OutputSettings {
        filter: DacFilter::NonOversampling,
        volume_limit_centi_db: -300,
        balance_centi_db: 20,
        dsd_direct: true,
    };
    c.set_output(wanted).unwrap();
    assert_eq!(c.output(), wanted);
    c.set_mic(MicSettings { gain_centi_db: 900 }).unwrap();
    assert_eq!(c.mic().gain_centi_db, 900);
    assert!(c.dirty());
    c.clear_dirty();
    assert!(!c.dirty());
}

#[test]
fn out_of_range_audio_settings_are_refused_without_a_change() {
    let mut c = controls();
    let original = c.output();
    for bad in [
        OutputSettings { volume_limit_centi_db: 100, ..original },
        OutputSettings { volume_limit_centi_db: i16::MIN, ..original },
        OutputSettings { balance_centi_db: 601, ..original },
    ] {
        assert_eq!(c.set_output(bad), Err(CaseError::InvalidArgument));
    }
    assert_eq!(c.set_mic(MicSettings { gain_centi_db: -100 }), Err(CaseError::InvalidArgument));
    assert_eq!(c.set_mic(MicSettings { gain_centi_db: i16::MAX }), Err(CaseError::InvalidArgument));
    assert_eq!(c.output(), original);
    assert!(!c.dirty());
}

#[test]
fn eq_slots_validate_and_selection_requires_a_filled_slot() {
    let mut c = controls();
    assert_eq!(c.eq(0), Ok(None));
    assert_eq!(c.eq(4), Err(CaseError::InvalidArgument));
    assert_eq!(c.set_eq(EqSlotUpdate { slot: 99, profile: profile(100, 100) }), Err(CaseError::InvalidArgument));
    let mut bad = profile(100, 100);
    bad.bands[0].frequency_hz = 1;
    assert_eq!(c.set_eq(EqSlotUpdate { slot: 0, profile: bad }), Err(CaseError::InvalidArgument));
    assert_eq!(c.select_eq(Some(0)), Err(CaseError::WrongState));
    assert_eq!(c.select_eq(Some(4)), Err(CaseError::InvalidArgument));

    c.set_eq(EqSlotUpdate { slot: 1, profile: profile(60, 300) }).unwrap();
    assert_eq!(c.eq(1).unwrap(), Some(profile(60, 300)));
    c.select_eq(Some(1)).unwrap();
    assert_eq!(c.active_eq(), Some(1));
    c.select_eq(None).unwrap();
    assert_eq!(c.active_eq(), None);
    assert_eq!(c.select_eq(Some(2)), Err(CaseError::WrongState));
}

#[test]
fn keys_upsert_page_and_clear() {
    let mut c = controls();
    assert_eq!(c.set_key(key(9, 4, KeyAction::Key(0x04))), Err(CaseError::InvalidArgument));
    assert_eq!(c.set_key(key(0, 4, KeyAction::LayerHold(9))), Err(CaseError::InvalidArgument));
    assert_eq!(c.set_key(key(0, 4, KeyAction::LayerHold(0))), Err(CaseError::InvalidArgument));
    assert_eq!(c.set_key(key(0, 4, KeyAction::LayerToggle(0))), Err(CaseError::InvalidArgument));
    assert_eq!(c.set_key(key(0, 4, KeyAction::Consumer(0x400))), Err(CaseError::InvalidArgument));
    assert_eq!(c.set_key(key(0, 4, KeyAction::Key(0x01))), Err(CaseError::InvalidArgument));
    assert_eq!(c.set_key(key(0, 4, KeyAction::KeyWithModifiers { usage: 0xE1, modifiers: 1 })), Err(CaseError::InvalidArgument));
    assert!(!c.dirty());

    c.set_key(key(0, 10, KeyAction::Key(0x10))).unwrap();
    c.set_key(key(0, 4, KeyAction::Consumer(0xE9))).unwrap();
    c.set_key(key(1, 4, KeyAction::LayerHold(2))).unwrap();
    c.set_key(key(0, 4, KeyAction::Key(0x55))).unwrap();

    let page = c.keymap(KeymapPage { layer: 0, start: 0 }).unwrap();
    let sources: std::vec::Vec<u8> = page.entries.iter().map(|e| e.source).collect();
    assert_eq!(sources, [4, 10]);
    assert_eq!(page.entries[0].action, KeyAction::Key(0x55));
    assert!(!page.more);

    c.set_key(key(0, 4, KeyAction::Transparent)).unwrap();
    let page = c.keymap(KeymapPage { layer: 0, start: 0 }).unwrap();
    assert_eq!(page.entries.len(), 1);
    assert_eq!(page.entries[0].source, 10);

    assert_eq!(c.keymap(KeymapPage { layer: 9, start: 0 }), Err(CaseError::InvalidArgument));
    c.clear_keymap().unwrap();
    assert_eq!(c.keymap(KeymapPage { layer: 0, start: 0 }).unwrap().entries.len(), 0);
}

#[test]
fn a_page_reports_more_and_honors_the_start() {
    let mut c = controls();
    for source in 0..40u8 {
        c.set_key(key(0, source, KeyAction::Key(0x04 + source))).unwrap();
    }
    let first = c.keymap(KeymapPage { layer: 0, start: 0 }).unwrap();
    assert_eq!(first.entries.len(), 32);
    assert_eq!(first.entries[0].source, 0);
    assert!(first.more);
    let rest = c.keymap(KeymapPage { layer: 0, start: 32 }).unwrap();
    assert_eq!(rest.entries.len(), 8);
    assert_eq!(rest.entries[0].source, 32);
    assert!(!rest.more);
}

#[test]
fn a_full_reverse_order_keymap_pages_in_source_order() {
    let mut controls = controls();
    for source in (0..96).rev() {
        controls.set_key(key(0, source, KeyAction::Key(0x04 + source))).unwrap();
    }
    for start in [0, 32, 64] {
        let page = controls.keymap(KeymapPage { layer: 0, start }).unwrap();
        assert_eq!(page.entries.len(), 32);
        assert_eq!(page.more, start < 64);
        let sources: std::vec::Vec<_> = page.entries.iter().map(|entry| entry.source).collect();
        assert_eq!(sources, (start..start + 32).collect::<std::vec::Vec<_>>());
    }
}

#[test]
fn reset_scopes_touch_only_their_area() {
    let mut c = controls();
    c.set_output(OutputSettings {
        filter: DacFilter::SlowLowLatency,
        volume_limit_centi_db: -500,
        balance_centi_db: 0,
        dsd_direct: false,
    })
    .unwrap();
    c.set_eq(EqSlotUpdate { slot: 0, profile: profile(100, 200) }).unwrap();
    c.select_eq(Some(0)).unwrap();
    c.set_key(key(0, 4, KeyAction::Key(0x04))).unwrap();

    c.reset(SettingsScope::Audio).unwrap();
    assert_eq!(c.output(), StoredSettings::default().output);
    assert_eq!(c.active_eq(), None);
    assert_eq!(c.eq(0), Ok(None));
    assert_eq!(c.keymap(KeymapPage { layer: 0, start: 0 }).unwrap().entries.len(), 1);

    c.set_eq(EqSlotUpdate { slot: 0, profile: profile(100, 200) }).unwrap();
    c.reset(SettingsScope::Keymap).unwrap();
    assert_eq!(c.keymap(KeymapPage { layer: 0, start: 0 }).unwrap().entries.len(), 0);
    assert!(c.eq(0).unwrap().is_some());

    c.reset(SettingsScope::All).unwrap();
    assert_eq!(c.settings(), &StoredSettings::default());
}

#[test]
fn storage_full_is_reported() {
    let mut c = controls();
    let mut layer = 0u8;
    let mut source = 0u16;
    let mut full = false;
    for _ in 0..1_000 {
        let result = c.set_key(key(layer, source as u8, KeyAction::Key(0x04)));
        if result == Err(CaseError::StorageFull) {
            full = true;
            break;
        }
        result.unwrap();
        source += 1;
        if source == 256 {
            source = 0;
            layer += 1;
            if layer == 4 {
                break;
            }
        }
    }
    assert!(full, "the keymap must report StorageFull once it is full");
}

#[test]
fn writing_the_value_already_held_does_not_mark_dirty() {
    let mut c = controls();
    let output = c.output();
    c.set_output(output).unwrap();
    c.set_mic(c.mic()).unwrap();
    c.select_eq(None).unwrap();
    c.clear_keymap().unwrap();
    c.set_key(key(0, 4, KeyAction::Transparent)).unwrap();
    assert!(!c.dirty());

    c.set_eq(EqSlotUpdate { slot: 1, profile: profile(100, 300) }).unwrap();
    c.set_key(key(0, 4, KeyAction::Key(5))).unwrap();
    assert!(c.dirty());
    c.clear_dirty();
    c.set_eq(EqSlotUpdate { slot: 1, profile: profile(100, 300) }).unwrap();
    c.set_key(key(0, 4, KeyAction::Key(5))).unwrap();
    c.select_eq(None).unwrap();
    assert!(!c.dirty());
    c.set_key(key(0, 4, KeyAction::Transparent)).unwrap();
    assert!(c.dirty());
}
