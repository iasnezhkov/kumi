// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
use kumi_keymap::hid::{
    BOOT_KEYBOARD_REPORT_LEN, CONSUMER_REPORT_LEN, CONSUMER_USAGE_MAX, KEYBOARD_LED_REPORT_LEN, KEYBOARD_REPORT_LEN,
    MOUSE_REPORT_LEN, REPORT_DESCRIPTOR, REPORT_DESCRIPTOR_NO_MOUSE, REPORT_ID_CONSUMER, REPORT_ID_KEYBOARD, REPORT_ID_MOUSE,
    boot_keyboard_report, consumer_report, keyboard_report, mouse_report,
};
use kumi_keymap::{Action, KeyCombo, KeyboardReport, Keymap, MouseAction, MouseReport, STOCK_CONSUMER_USAGES};
use std::collections::BTreeMap;

#[derive(Default)]
struct Totals {
    input_bits: u32,
    output_bits: u32,
}

struct Parsed {
    per_report: BTreeMap<u8, Totals>,
    depth_min: i32,
    depth_end: i32,
}

fn walk(descriptor: &[u8]) -> Parsed {
    let mut per_report: BTreeMap<u8, Totals> = BTreeMap::new();
    let mut report_id = 0u8;
    let mut report_size = 0u32;
    let mut report_count = 0u32;
    let mut depth = 0i32;
    let mut depth_min = 0i32;
    let mut i = 0usize;
    while i < descriptor.len() {
        let head = descriptor[i];
        assert_ne!(head, 0xFE, "long items are not used in this descriptor");
        let size = match head & 0x03 {
            3 => 4,
            n => n as usize,
        };
        assert!(i + 1 + size <= descriptor.len(), "item at {i} runs past the end");
        let mut data = 0u32;
        for (shift, byte) in descriptor[i + 1..i + 1 + size].iter().enumerate() {
            data |= (*byte as u32) << (8 * shift);
        }
        match head & 0xFC {
            0x84 => report_id = data as u8,
            0x74 => report_size = data,
            0x94 => report_count = data,
            0x80 => per_report.entry(report_id).or_default().input_bits += report_size * report_count,
            0x90 => per_report.entry(report_id).or_default().output_bits += report_size * report_count,
            0xA0 => depth += 1,
            0xC0 => {
                depth -= 1;
                depth_min = depth_min.min(depth);
            }
            _ => {}
        }
        i += 1 + size;
    }
    Parsed { per_report, depth_min, depth_end: depth }
}

#[test]
fn the_descriptor_opens_with_a_boot_compatible_keyboard() {
    assert_eq!(
        &REPORT_DESCRIPTOR[..8],
        &[0x05, 0x01, 0x09, 0x06, 0xA1, 0x01, 0x85, REPORT_ID_KEYBOARD],
        "a boot keyboard application collection must come first",
    );
}

#[test]
fn every_collection_is_closed() {
    let parsed = walk(REPORT_DESCRIPTOR);
    assert_eq!(parsed.depth_end, 0, "collections are unbalanced");
    assert_eq!(parsed.depth_min, 0, "an End Collection appeared without an open collection");
}

#[test]
fn the_declared_report_sizes_match_the_serializers() {
    let parsed = walk(REPORT_DESCRIPTOR);
    let keyboard = parsed.per_report.get(&REPORT_ID_KEYBOARD).expect("keyboard report");
    let consumer = parsed.per_report.get(&REPORT_ID_CONSUMER).expect("consumer report");
    let mouse = parsed.per_report.get(&REPORT_ID_MOUSE).expect("mouse report");

    assert_eq!(keyboard.input_bits, (BOOT_KEYBOARD_REPORT_LEN as u32) * 8);
    assert_eq!(1 + keyboard.input_bits as usize / 8, KEYBOARD_REPORT_LEN);
    assert_eq!(1 + keyboard.output_bits as usize / 8, KEYBOARD_LED_REPORT_LEN);
    assert_eq!(1 + consumer.input_bits as usize / 8, CONSUMER_REPORT_LEN);
    assert_eq!(1 + mouse.input_bits as usize / 8, MOUSE_REPORT_LEN);
}

#[test]
fn only_the_expected_report_ids_exist() {
    let parsed = walk(REPORT_DESCRIPTOR);
    let ids: Vec<u8> = parsed.per_report.keys().copied().collect();
    assert_eq!(ids, [REPORT_ID_KEYBOARD, REPORT_ID_CONSUMER, REPORT_ID_MOUSE]);
}

#[test]
fn the_no_mouse_descriptor_omits_only_the_mouse_collection() {
    assert_eq!(REPORT_DESCRIPTOR_NO_MOUSE, &REPORT_DESCRIPTOR[..91]);
    let parsed = walk(REPORT_DESCRIPTOR_NO_MOUSE);
    assert_eq!(parsed.depth_end, 0);
    assert_eq!(parsed.depth_min, 0);
    let ids: Vec<u8> = parsed.per_report.keys().copied().collect();
    assert_eq!(ids, [REPORT_ID_KEYBOARD, REPORT_ID_CONSUMER]);
    let keyboard = parsed.per_report.get(&REPORT_ID_KEYBOARD).expect("keyboard report");
    let consumer = parsed.per_report.get(&REPORT_ID_CONSUMER).expect("consumer report");
    assert_eq!(1 + keyboard.input_bits as usize / 8, KEYBOARD_REPORT_LEN);
    assert_eq!(1 + keyboard.output_bits as usize / 8, KEYBOARD_LED_REPORT_LEN);
    assert_eq!(1 + consumer.input_bits as usize / 8, CONSUMER_REPORT_LEN);
}

#[test]
fn keymap_detects_mouse_actions_in_layers_and_combos() {
    let mut map = Keymap::new(STOCK_CONSUMER_USAGES);
    assert!(!map.has_mouse_action());
    map.set(0, 0x04, Action::Mouse(MouseAction::LeftClick)).unwrap();
    assert!(map.has_mouse_action());
    map.clear();
    assert!(!map.has_mouse_action());
    map.set_combo(0, Some(KeyCombo { key_a: 0x04, key_b: 0x05, action: Action::Mouse(MouseAction::MoveUp) })).unwrap();
    assert!(map.has_mouse_action());
}

#[test]
fn mouse_report_packs_buttons_and_clamped_signed_deltas() {
    let report = MouseReport { buttons: 0x05, dx: -12, dy: 24, wheel: -1 };
    assert_eq!(mouse_report(&report), [REPORT_ID_MOUSE, 0x05, (-12i8) as u8, 24, 0xFF]);
    assert_eq!(mouse_report(&MouseReport { buttons: 0xFF, dx: i8::MIN, dy: 127, wheel: 0 }), [3, 0x07, 0x81, 0x7F, 0]);
}

#[test]
fn a_keyboard_report_carries_modifiers_and_six_keys() {
    let report = KeyboardReport { modifiers: 0x22, keys: [0x04, 0x05, 0x06, 0, 0, 0] };
    let wire = keyboard_report(&report);
    assert_eq!(wire, [REPORT_ID_KEYBOARD, 0x22, 0x00, 0x04, 0x05, 0x06, 0x00, 0x00, 0x00]);
    let boot = boot_keyboard_report(&report);
    assert_eq!(boot, [0x22, 0x00, 0x04, 0x05, 0x06, 0x00, 0x00, 0x00]);
}

#[test]
fn a_consumer_report_is_a_little_endian_usage_clamped_to_the_declared_range() {
    assert_eq!(consumer_report(0x00CD), [REPORT_ID_CONSUMER, 0xCD, 0x00]);
    assert_eq!(consumer_report(0x00E9), [REPORT_ID_CONSUMER, 0xE9, 0x00]);
    assert_eq!(consumer_report(0x03FF), [REPORT_ID_CONSUMER, 0xFF, 0x03]);
    assert_eq!(consumer_report(CONSUMER_USAGE_MAX + 1), [REPORT_ID_CONSUMER, 0x00, 0x00]);
    assert_eq!(consumer_report(0), [REPORT_ID_CONSUMER, 0x00, 0x00]);
}
