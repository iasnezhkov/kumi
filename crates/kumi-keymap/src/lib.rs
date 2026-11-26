// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
#![cfg_attr(not(test), no_std)]

pub mod hid;

use heapless::{Deque, Vec};
pub use kumi_icd::{
    COMBO_SLOTS, CaseHotkey, KeyAction as Action, KeyCombo, KeyConfig, KeyEvent as Event, MACRO_SLOTS, MacroProgram, MouseAction,
};

pub const LAYERS: usize = kumi_icd::LAYERS as usize;
pub const SOURCES: usize = 256;
pub const ROLLOVER: usize = 6;
pub const CONSUMER_SOURCE_BASE: u8 = 0xF0;
pub const CONSUMER_BITS: usize = 8;
pub const STOCK_CONSUMER_USAGES: [u16; CONSUMER_BITS] = [0xE9, 0xEA, 0xCD, 0xCF, 0xB6, 0xB5, 0x223, 0];
pub const MODIFIER_FIRST: u8 = 0xE0;
pub const MODIFIER_LAST: u8 = 0xE7;
pub const ERROR_ROLLOVER: u8 = 0x01;
pub const MAX_EVENTS: usize = 24;
pub const MAX_HOTKEYS: usize = 8;
pub const MACRO_STEP_MS: u32 = 8;
pub const MOUSE_INTERVAL_MS: u32 = 16;
const MOUSE_BASE_SPEED: i8 = 6;
const MOUSE_MAX_SPEED: i8 = 18;
const MOUSE_ACCEL_MS: u32 = 400;
const MOUSE_WHEEL_DIVIDER: u8 = 5;
const MAX_PENDING_TAP_HOLD: usize = 4;
const MAX_PENDING_COMBOS: usize = 4;
const MAX_ACTIVE_COMBOS: usize = 4;
const MACRO_QUEUE_CAPACITY: usize = 24;
const LSHIFT_BIT: u8 = 0x02;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KeyboardReport {
    pub modifiers: u8,
    pub keys: [u8; ROLLOVER],
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MouseReport {
    pub buttons: u8,
    pub dx: i8,
    pub dy: i8,
    pub wheel: i8,
}

impl MouseReport {
    pub const fn has_motion(self) -> bool {
        self.dx != 0 || self.dy != 0 || self.wheel != 0
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Input {
    pub keyboard: KeyboardReport,
    pub consumer_bits: u8,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Output {
    pub keyboard: KeyboardReport,
    pub consumer: u16,
    pub mouse: MouseReport,
    pub events: Vec<Event, MAX_EVENTS>,
    pub case_hotkeys: Vec<CaseHotkey, MAX_HOTKEYS>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeymapError {
    Layer,
    Target,
}

pub struct Keymap {
    layers: [[Action; SOURCES]; LAYERS],
    consumer_defaults: [u16; CONSUMER_BITS],
    config: KeyConfig,
    macros: [Option<MacroProgram>; MACRO_SLOTS as usize],
    combos: [Option<KeyCombo>; COMBO_SLOTS as usize],
}

impl Keymap {
    pub const fn new(consumer_defaults: [u16; CONSUMER_BITS]) -> Self {
        Self {
            layers: [[Action::Transparent; SOURCES]; LAYERS],
            consumer_defaults,
            config: KeyConfig { tapping_term_ms: 200, one_shot_timeout_ms: 3_000, combo_term_ms: 50 },
            macros: [const { None }; MACRO_SLOTS as usize],
            combos: [None; COMBO_SLOTS as usize],
        }
    }

    pub fn set_config(&mut self, config: KeyConfig) {
        if config.is_valid() {
            self.config = config;
        }
    }

    pub fn config(&self) -> KeyConfig {
        self.config
    }

    pub fn set_macro(&mut self, slot: u8, program: Option<MacroProgram>) -> Result<(), KeymapError> {
        if slot >= MACRO_SLOTS {
            return Err(KeymapError::Target);
        }
        self.macros[usize::from(slot)] = program;
        Ok(())
    }

    pub fn macro_program(&self, slot: u8) -> Option<&MacroProgram> {
        self.macros.get(usize::from(slot)).and_then(Option::as_ref)
    }

    pub fn clear_macros(&mut self) {
        for slot in &mut self.macros {
            *slot = None;
        }
    }

    pub fn set_combo(&mut self, slot: u8, combo: Option<KeyCombo>) -> Result<(), KeymapError> {
        if slot >= COMBO_SLOTS {
            return Err(KeymapError::Target);
        }
        if let Some(combo) = combo
            && !combo.is_valid()
        {
            return Err(KeymapError::Target);
        }
        self.combos[usize::from(slot)] = combo;
        Ok(())
    }

    pub fn combo(&self, slot: u8) -> Option<KeyCombo> {
        self.combos.get(usize::from(slot)).copied().flatten()
    }

    pub fn clear_combos(&mut self) {
        self.combos = [None; COMBO_SLOTS as usize];
    }

    fn has_combo_for(&self, source: u8) -> bool {
        self.combos.iter().flatten().any(|combo| combo.key_a == source || combo.key_b == source)
    }

    fn combo_matching(&self, first: u8, second: u8) -> Option<KeyCombo> {
        self.combos
            .iter()
            .flatten()
            .copied()
            .find(|combo| (combo.key_a == first && combo.key_b == second) || (combo.key_a == second && combo.key_b == first))
    }

    pub fn set(&mut self, layer: u8, source: u8, action: Action) -> Result<(), KeymapError> {
        if usize::from(layer) >= LAYERS {
            return Err(KeymapError::Layer);
        }
        match action {
            Action::LayerHold(target) | Action::LayerToggle(target) | Action::TapHoldLayer { hold_layer: target, .. }
                if usize::from(target) >= LAYERS || target == 0 =>
            {
                return Err(KeymapError::Target);
            }
            Action::TapHoldMod { hold_mods: 0, .. } | Action::OneShotMod(0) => return Err(KeymapError::Target),
            Action::Macro(slot) if slot >= MACRO_SLOTS => return Err(KeymapError::Target),
            _ => {}
        }
        self.layers[usize::from(layer)][usize::from(source)] = action;
        Ok(())
    }

    pub fn get(&self, layer: u8, source: u8) -> Option<Action> {
        self.layers.get(usize::from(layer)).map(|l| l[usize::from(source)])
    }

    pub fn clear(&mut self) {
        self.layers = [[Action::Transparent; SOURCES]; LAYERS];
    }

    pub fn has_mouse_action(&self) -> bool {
        self.layers.iter().any(|layer| layer.iter().any(|action| matches!(action, Action::Mouse(_))))
            || self.combos.iter().flatten().any(|combo| matches!(combo.action, Action::Mouse(_)))
    }

    pub fn entries(&self) -> impl Iterator<Item = (u8, u8, Action)> + '_ {
        self.layers.iter().enumerate().flat_map(|(layer, actions)| {
            actions
                .iter()
                .enumerate()
                .filter(|(_, action)| **action != Action::Transparent)
                .map(move |(source, action)| (layer as u8, source as u8, *action))
        })
    }

    fn base_action(&self, source: u8) -> Action {
        match source {
            MODIFIER_FIRST..=MODIFIER_LAST => Action::Modifiers(1 << (source - MODIFIER_FIRST)),
            s if s >= CONSUMER_SOURCE_BASE && usize::from(s - CONSUMER_SOURCE_BASE) < CONSUMER_BITS => {
                match self.consumer_defaults[usize::from(s - CONSUMER_SOURCE_BASE)] {
                    0 => Action::Disabled,
                    usage => Action::Consumer(usage),
                }
            }
            0 => Action::Disabled,
            s => Action::Key(s),
        }
    }

    fn resolve(&self, source: u8, active: u8) -> Action {
        for layer in (0..LAYERS).rev() {
            if active & (1 << layer) == 0 {
                continue;
            }
            match self.layers[layer][usize::from(source)] {
                Action::Transparent => continue,
                action => return action,
            }
        }
        self.base_action(source)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PendingTapHold {
    source: u8,
    pressed_at: u32,
    action: Action,
}

impl PendingTapHold {
    const fn hold_action(self) -> Action {
        match self.action {
            Action::TapHoldMod { hold_mods, .. } => Action::Modifiers(hold_mods),
            Action::TapHoldLayer { hold_layer, .. } => Action::LayerHold(hold_layer),
            other => other,
        }
    }

    const fn tap_usage(self) -> Option<u8> {
        match self.action {
            Action::TapHoldMod { tap, .. } | Action::TapHoldLayer { tap, .. } => Some(tap),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PendingCombo {
    source: u8,
    pressed_at: u32,
    layer: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ActiveCombo {
    key_a: u8,
    key_b: u8,
    owner: u8,
}

const fn is_mouse_motion(action: Action) -> bool {
    matches!(
        action,
        Action::Mouse(
            MouseAction::MoveUp
                | MouseAction::MoveDown
                | MouseAction::MoveLeft
                | MouseAction::MoveRight
                | MouseAction::WheelUp
                | MouseAction::WheelDown
        )
    )
}

pub struct Engine {
    pressed: [u32; SOURCES / 32],
    held: [Action; SOURCES],
    toggled: u8,
    caps_word: bool,
    consumer_order: Vec<u8, CONSUMER_BITS>,
    pending: Vec<PendingTapHold, MAX_PENDING_TAP_HOLD>,
    pending_combos: Vec<PendingCombo, MAX_PENDING_COMBOS>,
    active_combos: Vec<ActiveCombo, MAX_ACTIVE_COMBOS>,
    one_shot_mods: u8,
    one_shot_until_ms: u32,
    one_shot_armed: bool,
    one_shot_used_mods: u8,
    macro_steps: Deque<(u8, u8), MACRO_QUEUE_CAPACITY>,
    macro_active: Option<(u8, u8)>,
    macro_ready_at: Option<u32>,
    mouse_tick_at: Option<u32>,
    mouse_hold_start: Option<u32>,
    mouse_wheel_div: u8,
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

impl Engine {
    pub const fn new() -> Self {
        Self {
            pressed: [0; SOURCES / 32],
            held: [Action::Disabled; SOURCES],
            toggled: 0,
            caps_word: false,
            consumer_order: Vec::new(),
            pending: Vec::new(),
            pending_combos: Vec::new(),
            active_combos: Vec::new(),
            one_shot_mods: 0,
            one_shot_until_ms: 0,
            one_shot_armed: false,
            one_shot_used_mods: 0,
            macro_steps: Deque::new(),
            macro_active: None,
            macro_ready_at: None,
            mouse_tick_at: None,
            mouse_hold_start: None,
            mouse_wheel_div: 0,
        }
    }

    pub fn reset(&mut self) {
        *self = Self::new();
    }

    pub fn clear_toggled(&mut self) {
        self.toggled = 0;
    }

    pub fn caps_word(&self) -> bool {
        self.caps_word
    }

    pub fn toggle_caps_word(&mut self) {
        self.caps_word = !self.caps_word;
    }

    pub fn has_fast_timers(&self) -> bool {
        self.macro_active.is_some()
            || !self.macro_steps.is_empty()
            || !self.pending_combos.is_empty()
            || self.mouse_tick_at.is_some()
    }

    pub fn has_pending_timers(&self) -> bool {
        !self.pending.is_empty() || self.one_shot_armed || self.has_fast_timers()
    }

    pub fn active_layers(&self) -> u8 {
        let mut mask = 1 | self.toggled;
        for source in 0..SOURCES {
            if self.is_pressed(source as u8)
                && let Action::LayerHold(layer) = self.held[source]
            {
                mask |= 1 << layer;
            }
        }
        mask
    }

    pub fn top_layer(&self) -> u8 {
        7 - self.active_layers().leading_zeros() as u8
    }

    fn is_pressed(&self, source: u8) -> bool {
        self.pressed[usize::from(source) / 32] & (1 << (source % 32)) != 0
    }

    fn set_pressed(&mut self, source: u8, value: bool) {
        let bit = 1 << (source % 32);
        if value {
            self.pressed[usize::from(source) / 32] |= bit;
        } else {
            self.pressed[usize::from(source) / 32] &= !bit;
        }
    }

    fn apply_caps_word(&mut self, usage: u8) -> u8 {
        if !self.caps_word {
            return 0;
        }
        if (0x04..=0x1D).contains(&usage) || usage == 0x2D {
            LSHIFT_BIT
        } else if (0x1E..=0x27).contains(&usage) || usage == 0x2A || usage == 0x4C {
            0
        } else {
            self.caps_word = false;
            0
        }
    }

    fn has_held_mouse_motion(&self) -> bool {
        for source in 0..SOURCES {
            if self.is_pressed(source as u8) && is_mouse_motion(self.held[source]) {
                return true;
            }
        }
        false
    }

    fn mouse_speed(&self, now_ms: u32) -> i8 {
        let held_ms = self.mouse_hold_start.map_or(0, |start| now_ms.wrapping_sub(start));
        let ramp = held_ms.min(MOUSE_ACCEL_MS);
        let span = i32::from(MOUSE_MAX_SPEED - MOUSE_BASE_SPEED);
        let speed = i32::from(MOUSE_BASE_SPEED) + (span * ramp as i32) / MOUSE_ACCEL_MS as i32;
        speed.clamp(i32::from(MOUSE_BASE_SPEED), i32::from(MOUSE_MAX_SPEED)) as i8
    }

    fn promote_all_pending(&mut self) -> bool {
        if self.pending.is_empty() {
            return false;
        }
        for entry in self.pending.iter().copied() {
            if self.is_pressed(entry.source) {
                self.held[usize::from(entry.source)] = entry.hold_action();
            }
        }
        self.pending.clear();
        true
    }

    fn promote_expired_pending(&mut self, tapping_term_ms: u16, now_ms: u32) -> bool {
        let mut changed = false;
        let term = u32::from(tapping_term_ms);
        let mut kept: Vec<PendingTapHold, MAX_PENDING_TAP_HOLD> = Vec::new();
        for entry in self.pending.iter().copied() {
            if now_ms.wrapping_sub(entry.pressed_at) >= term {
                if self.is_pressed(entry.source) {
                    self.held[usize::from(entry.source)] = entry.hold_action();
                    changed = true;
                }
            } else {
                let _ = kept.push(entry);
            }
        }
        self.pending = kept;
        changed
    }

    fn mark_one_shot_used(&mut self) {
        for source in 0..SOURCES {
            if self.is_pressed(source as u8)
                && let Action::OneShotMod(mods) = self.held[source]
            {
                self.one_shot_used_mods |= mods;
            }
        }
    }

    fn consume_one_shot(&mut self) -> u8 {
        if !self.one_shot_armed {
            return 0;
        }
        self.one_shot_armed = false;
        core::mem::take(&mut self.one_shot_mods)
    }

    fn step_macro(&mut self, now_ms: u32) -> bool {
        if self.macro_active.is_some() {
            if self.macro_ready_at.is_some_and(|at| now_ms.wrapping_sub(at) as i32 >= 0) {
                self.macro_active = None;
                self.macro_ready_at = if self.macro_steps.is_empty() { None } else { Some(now_ms.wrapping_add(MACRO_STEP_MS)) };
                return true;
            }
            return false;
        }
        if !self.macro_steps.is_empty() && self.macro_ready_at.is_none_or(|at| now_ms.wrapping_sub(at) as i32 >= 0) {
            self.macro_active = self.macro_steps.pop_front();
            self.macro_ready_at = Some(now_ms.wrapping_add(MACRO_STEP_MS));
            return self.macro_active.is_some();
        }
        false
    }

    #[allow(clippy::too_many_arguments)]
    fn activate_action(
        &mut self,
        keymap: &Keymap,
        source: u8,
        layer: u8,
        action: Action,
        now_ms: u32,
        output: &mut Output,
        emit_mouse_motion: &mut bool,
    ) {
        self.held[usize::from(source)] = action;
        match action {
            Action::LayerToggle(target) => {
                self.toggled ^= 1 << target;
            }
            Action::CapsWord => {
                self.caps_word = !self.caps_word;
            }
            Action::Consumer(_) => {
                let _ = self.consumer_order.push(source);
            }
            Action::CaseControl(hotkey) => {
                if hotkey == CaseHotkey::ToggleCapsWord {
                    self.caps_word = !self.caps_word;
                }
                let _ = output.case_hotkeys.push(hotkey);
            }
            Action::OneShotMod(mods) => {
                self.one_shot_used_mods &= !mods;
            }
            Action::TapHoldMod { .. } | Action::TapHoldLayer { .. } => {
                if self.pending.is_full() {
                    self.promote_all_pending();
                }
                let _ = self.pending.push(PendingTapHold { source, pressed_at: now_ms, action });
            }
            Action::Macro(slot) => {
                self.mark_one_shot_used();
                let extra = self.consume_one_shot();
                if let Some(program) = keymap.macro_program(slot) {
                    for (index, step) in program.steps.iter().enumerate() {
                        let mods = if index == 0 { step.modifiers | extra } else { step.modifiers };
                        let _ = self.macro_steps.push_back((step.usage, mods));
                    }
                }
            }
            Action::Key(usage) => {
                self.mark_one_shot_used();
                let extra = self.consume_one_shot() | self.apply_caps_word(usage);
                if extra != 0 {
                    self.held[usize::from(source)] = Action::KeyWithModifiers { usage, modifiers: extra };
                }
            }
            Action::KeyWithModifiers { usage, modifiers } => {
                self.mark_one_shot_used();
                let extra = self.consume_one_shot() | self.apply_caps_word(usage);
                if extra != 0 {
                    self.held[usize::from(source)] = Action::KeyWithModifiers { usage, modifiers: modifiers | extra };
                }
            }
            Action::Mouse(_) if is_mouse_motion(action) => {
                *emit_mouse_motion = true;
                if self.mouse_hold_start.is_none() {
                    self.mouse_hold_start = Some(now_ms);
                    self.mouse_wheel_div = 0;
                }
                self.mouse_tick_at = Some(now_ms.wrapping_add(MOUSE_INTERVAL_MS));
            }
            _ => {}
        }
        let _ = output.events.push(Event { source, pressed: true, layer, action });
    }

    fn flush_all_pending_combos(
        &mut self,
        keymap: &Keymap,
        now_ms: u32,
        output: &mut Output,
        emit_mouse_motion: &mut bool,
    ) -> bool {
        if self.pending_combos.is_empty() {
            return false;
        }
        let queued = core::mem::take(&mut self.pending_combos);
        for entry in queued {
            let action = keymap.resolve(entry.source, self.active_layers());
            self.activate_action(keymap, entry.source, entry.layer, action, now_ms, output, emit_mouse_motion);
        }
        true
    }

    fn flush_expired_combos(&mut self, keymap: &Keymap, now_ms: u32, output: &mut Output, emit_mouse_motion: &mut bool) -> bool {
        if self.pending_combos.is_empty() {
            return false;
        }
        let term = u32::from(keymap.config().combo_term_ms);
        let mut changed = false;
        let mut kept: Vec<PendingCombo, MAX_PENDING_COMBOS> = Vec::new();
        let queued = core::mem::take(&mut self.pending_combos);
        for entry in queued {
            if now_ms.wrapping_sub(entry.pressed_at) >= term {
                let action = keymap.resolve(entry.source, self.active_layers());
                self.activate_action(keymap, entry.source, entry.layer, action, now_ms, output, emit_mouse_motion);
                changed = true;
            } else {
                let _ = kept.push(entry);
            }
        }
        self.pending_combos = kept;
        changed
    }

    fn build_output(&self, rollover: bool, emit_mouse_motion: bool, emit_wheel: bool, now_ms: u32, output: &mut Output) {
        let mut keys: Vec<u8, ROLLOVER> = Vec::new();
        let mut overflow = false;
        let speed = if emit_mouse_motion { self.mouse_speed(now_ms) } else { 0 };
        let mut dx: i16 = 0;
        let mut dy: i16 = 0;
        let mut wheel: i16 = 0;
        for source in 0..=255u8 {
            if !self.is_pressed(source) {
                continue;
            }
            match self.held[usize::from(source)] {
                Action::Key(usage) => overflow |= push_key(&mut keys, usage),
                Action::Modifiers(bits) | Action::OneShotMod(bits) => output.keyboard.modifiers |= bits,
                Action::KeyWithModifiers { usage, modifiers } => {
                    output.keyboard.modifiers |= modifiers;
                    overflow |= push_key(&mut keys, usage);
                }
                Action::Mouse(mouse) => match mouse {
                    MouseAction::LeftClick => output.mouse.buttons |= 1 << 0,
                    MouseAction::RightClick => output.mouse.buttons |= 1 << 1,
                    MouseAction::MiddleClick => output.mouse.buttons |= 1 << 2,
                    MouseAction::MoveUp if emit_mouse_motion => dy -= i16::from(speed),
                    MouseAction::MoveDown if emit_mouse_motion => dy += i16::from(speed),
                    MouseAction::MoveLeft if emit_mouse_motion => dx -= i16::from(speed),
                    MouseAction::MoveRight if emit_mouse_motion => dx += i16::from(speed),
                    MouseAction::WheelUp if emit_mouse_motion && emit_wheel => wheel += 1,
                    MouseAction::WheelDown if emit_mouse_motion && emit_wheel => wheel -= 1,
                    _ => {}
                },
                _ => {}
            }
        }
        output.mouse.dx = dx.clamp(-127, 127) as i8;
        output.mouse.dy = dy.clamp(-127, 127) as i8;
        output.mouse.wheel = wheel.clamp(-127, 127) as i8;
        if let Some((usage, modifiers)) = self.macro_active {
            output.keyboard.modifiers |= modifiers;
            overflow |= push_key(&mut keys, usage);
        }
        if overflow || rollover {
            output.keyboard.keys = [ERROR_ROLLOVER; ROLLOVER];
        } else {
            output.keyboard.keys[..keys.len()].copy_from_slice(&keys);
        }
        if let Some(source) = self.consumer_order.last()
            && let Action::Consumer(usage) = self.held[usize::from(*source)]
        {
            output.consumer = usage;
        }
    }

    pub fn tick(&mut self, keymap: &Keymap, now_ms: u32) -> Option<Output> {
        let mut output = Output::default();
        let mut emit_mouse_motion = false;
        let mut emit_wheel = false;
        let mut changed = self.flush_expired_combos(keymap, now_ms, &mut output, &mut emit_mouse_motion);
        changed |= self.promote_expired_pending(keymap.config().tapping_term_ms, now_ms);
        if self.one_shot_armed && now_ms.wrapping_sub(self.one_shot_until_ms) as i32 >= 0 {
            self.one_shot_armed = false;
            self.one_shot_mods = 0;
            changed = true;
        }
        changed |= self.step_macro(now_ms);
        if let Some(at) = self.mouse_tick_at {
            if !self.has_held_mouse_motion() {
                self.mouse_tick_at = None;
                self.mouse_hold_start = None;
                self.mouse_wheel_div = 0;
            } else if now_ms.wrapping_sub(at) as i32 >= 0 {
                emit_mouse_motion = true;
                self.mouse_wheel_div = self.mouse_wheel_div.wrapping_add(1);
                if self.mouse_wheel_div >= MOUSE_WHEEL_DIVIDER {
                    self.mouse_wheel_div = 0;
                    emit_wheel = true;
                }
                self.mouse_tick_at = Some(now_ms.wrapping_add(MOUSE_INTERVAL_MS));
                changed = true;
            }
        }
        if !changed {
            return None;
        }
        self.build_output(false, emit_mouse_motion, emit_wheel, now_ms, &mut output);
        Some(output)
    }

    pub fn process(&mut self, keymap: &Keymap, input: &Input) -> Output {
        self.process_at(keymap, input, 0)
    }

    pub fn process_at(&mut self, keymap: &Keymap, input: &Input, now_ms: u32) -> Output {
        let config = keymap.config();
        let mut output = Output::default();
        let mut emit_mouse_motion = false;
        let _ = self.flush_expired_combos(keymap, now_ms, &mut output, &mut emit_mouse_motion);
        let _ = self.promote_expired_pending(config.tapping_term_ms, now_ms);
        if self.one_shot_armed && now_ms.wrapping_sub(self.one_shot_until_ms) as i32 >= 0 {
            self.one_shot_armed = false;
            self.one_shot_mods = 0;
        }
        let mut now = [0u32; SOURCES / 32];
        let mut mark = |source: u8| now[usize::from(source) / 32] |= 1 << (source % 32);
        for bit in 0..8 {
            if input.keyboard.modifiers & (1 << bit) != 0 {
                mark(MODIFIER_FIRST + bit);
            }
            if input.consumer_bits & (1 << bit) != 0 {
                mark(CONSUMER_SOURCE_BASE + bit);
            }
        }
        let rollover = input.keyboard.keys.contains(&ERROR_ROLLOVER);
        if !rollover {
            for key in input.keyboard.keys.iter().filter(|k| **k > ERROR_ROLLOVER) {
                mark(*key);
            }
        }
        for source in 0..=255u8 {
            let was = self.is_pressed(source);
            let is = now[usize::from(source) / 32] & (1 << (source % 32)) != 0;
            if !was || is || (rollover && source < MODIFIER_FIRST) {
                continue;
            }
            if let Some(pos) = self.pending_combos.iter().position(|entry| entry.source == source) {
                let entry = self.pending_combos.remove(pos);
                self.set_pressed(source, false);
                let action = keymap.resolve(source, self.active_layers());
                match action {
                    Action::Key(usage) => {
                        self.mark_one_shot_used();
                        let mods = self.consume_one_shot() | self.apply_caps_word(usage);
                        let _ = self.macro_steps.push_back((usage, mods));
                    }
                    Action::KeyWithModifiers { usage, modifiers } => {
                        self.mark_one_shot_used();
                        let mods = modifiers | self.consume_one_shot() | self.apply_caps_word(usage);
                        let _ = self.macro_steps.push_back((usage, mods));
                    }
                    Action::TapHoldMod { tap, .. } | Action::TapHoldLayer { tap, .. } => {
                        self.mark_one_shot_used();
                        let mods = self.consume_one_shot() | self.apply_caps_word(tap);
                        let _ = self.macro_steps.push_back((tap, mods));
                    }
                    Action::LayerToggle(target) => {
                        self.toggled ^= 1 << target;
                    }
                    Action::CapsWord => {
                        self.caps_word = !self.caps_word;
                    }
                    Action::CaseControl(hotkey) => {
                        if hotkey == CaseHotkey::ToggleCapsWord {
                            self.caps_word = !self.caps_word;
                        }
                        let _ = output.case_hotkeys.push(hotkey);
                    }
                    Action::OneShotMod(mods) => {
                        self.one_shot_mods |= mods;
                        self.one_shot_armed = true;
                        self.one_shot_until_ms = now_ms.wrapping_add(u32::from(config.one_shot_timeout_ms));
                    }
                    Action::Macro(slot) => {
                        self.mark_one_shot_used();
                        let extra = self.consume_one_shot();
                        if let Some(program) = keymap.macro_program(slot) {
                            for (index, step) in program.steps.iter().enumerate() {
                                let mods = if index == 0 { step.modifiers | extra } else { step.modifiers };
                                let _ = self.macro_steps.push_back((step.usage, mods));
                            }
                        }
                    }
                    _ => {}
                }
                self.held[usize::from(source)] = Action::Disabled;
                let _ = output.events.push(Event { source, pressed: true, layer: entry.layer, action });
                let _ = output.events.push(Event { source, pressed: false, layer: self.top_layer(), action });
                continue;
            }
            if let Some(pos) = self.active_combos.iter().position(|c| c.key_a == source || c.key_b == source) {
                let combo = self.active_combos.remove(pos);
                let owner = combo.owner;
                if let Some(ppos) = self.pending.iter().position(|entry| entry.source == owner) {
                    let entry = self.pending.remove(ppos);
                    if let Some(usage) = entry.tap_usage() {
                        self.mark_one_shot_used();
                        let mods = self.consume_one_shot() | self.apply_caps_word(usage);
                        let _ = self.macro_steps.push_back((usage, mods));
                    }
                }
                let action = self.held[usize::from(owner)];
                if let Action::OneShotMod(mods) = action {
                    if self.one_shot_used_mods & mods == 0 {
                        self.one_shot_mods |= mods;
                        self.one_shot_armed = true;
                        self.one_shot_until_ms = now_ms.wrapping_add(u32::from(config.one_shot_timeout_ms));
                    }
                    self.one_shot_used_mods &= !mods;
                }
                self.held[usize::from(owner)] = Action::Disabled;
                self.consumer_order.retain(|s| *s != owner);
                self.set_pressed(source, false);
                let _ = output.events.push(Event { source, pressed: false, layer: self.top_layer(), action });
                continue;
            }
            if let Some(position) = self.pending.iter().position(|entry| entry.source == source) {
                let entry = self.pending.remove(position);
                if let Some(usage) = entry.tap_usage() {
                    self.mark_one_shot_used();
                    let mods = self.consume_one_shot() | self.apply_caps_word(usage);
                    let _ = self.macro_steps.push_back((usage, mods));
                }
            }
            let action = self.held[usize::from(source)];
            if let Action::OneShotMod(mods) = action {
                if self.one_shot_used_mods & mods == 0 {
                    self.one_shot_mods |= mods;
                    self.one_shot_armed = true;
                    self.one_shot_until_ms = now_ms.wrapping_add(u32::from(config.one_shot_timeout_ms));
                }
                self.one_shot_used_mods &= !mods;
            }
            self.set_pressed(source, false);
            self.consumer_order.retain(|s| *s != source);
            let _ = output.events.push(Event { source, pressed: false, layer: self.top_layer(), action });
        }
        let mut newly_pressed: Vec<u8, 16> = Vec::new();
        for source in 0..=255u8 {
            let is = now[usize::from(source) / 32] & (1 << (source % 32)) != 0;
            if is && !self.is_pressed(source) && !(rollover && source < MODIFIER_FIRST) {
                let _ = newly_pressed.push(source);
            }
        }
        if !newly_pressed.is_empty() {
            self.promote_all_pending();
        }
        let mut consumed_by_combo = [false; 16];
        for i in 0..newly_pressed.len() {
            if consumed_by_combo[i] {
                continue;
            }
            let src = newly_pressed[i];
            if let Some(pidx) = self.pending_combos.iter().position(|p| keymap.combo_matching(p.source, src).is_some()) {
                let partner = self.pending_combos.remove(pidx);
                if let Some(combo) = keymap.combo_matching(partner.source, src) {
                    consumed_by_combo[i] = true;
                    self.set_pressed(src, true);
                    self.held[usize::from(src)] = Action::Disabled;
                    let owner = partner.source;
                    let _ = self.active_combos.push(ActiveCombo { key_a: partner.source, key_b: src, owner });
                    self.activate_action(keymap, owner, partner.layer, combo.action, now_ms, &mut output, &mut emit_mouse_motion);
                }
                continue;
            }
            for j in (i + 1)..newly_pressed.len() {
                if consumed_by_combo[j] {
                    continue;
                }
                let other = newly_pressed[j];
                if let Some(combo) = keymap.combo_matching(src, other) {
                    consumed_by_combo[i] = true;
                    consumed_by_combo[j] = true;
                    self.set_pressed(src, true);
                    self.set_pressed(other, true);
                    self.held[usize::from(other)] = Action::Disabled;
                    let layer = self.top_layer();
                    let _ = self.active_combos.push(ActiveCombo { key_a: src, key_b: other, owner: src });
                    self.activate_action(keymap, src, layer, combo.action, now_ms, &mut output, &mut emit_mouse_motion);
                    break;
                }
            }
        }
        let unconsumed_new = newly_pressed.iter().enumerate().filter(|(idx, _)| !consumed_by_combo[*idx]).count();
        if unconsumed_new > 0 {
            let _ = self.flush_all_pending_combos(keymap, now_ms, &mut output, &mut emit_mouse_motion);
        }
        let initial_layers = self.active_layers();
        for layers_first in [true, false] {
            for (idx, &source) in newly_pressed.iter().enumerate() {
                if consumed_by_combo[idx] || self.is_pressed(source) {
                    continue;
                }
                if !layers_first && unconsumed_new == 1 && keymap.has_combo_for(source) && !self.pending_combos.is_full() {
                    let layer = self.top_layer();
                    self.set_pressed(source, true);
                    self.held[usize::from(source)] = Action::Transparent;
                    let _ = self.pending_combos.push(PendingCombo { source, pressed_at: now_ms, layer });
                    continue;
                }
                let layer = self.top_layer();
                let action = keymap.resolve(source, if layers_first { initial_layers } else { self.active_layers() });
                if layers_first && !matches!(action, Action::LayerHold(_) | Action::LayerToggle(_)) {
                    continue;
                }
                if layers_first && unconsumed_new == 1 && keymap.has_combo_for(source) {
                    continue;
                }
                self.set_pressed(source, true);
                self.activate_action(keymap, source, layer, action, now_ms, &mut output, &mut emit_mouse_motion);
            }
        }
        if !self.has_held_mouse_motion() {
            self.mouse_tick_at = None;
            self.mouse_hold_start = None;
            self.mouse_wheel_div = 0;
        }
        let _ = self.step_macro(now_ms);
        self.build_output(rollover, emit_mouse_motion, true, now_ms, &mut output);
        output
    }
}

fn push_key(keys: &mut Vec<u8, ROLLOVER>, usage: u8) -> bool {
    if usage <= ERROR_ROLLOVER || usage >= MODIFIER_FIRST || keys.contains(&usage) {
        return false;
    }
    keys.push(usage).is_err()
}

#[cfg(test)]
mod tests {
    use super::*;
    use kumi_icd::MacroStep;

    const A: u8 = 0x04;
    const B: u8 = 0x05;
    const C: u8 = 0x06;
    const J: u8 = 0x0D;
    const K: u8 = 0x0E;
    const NUM_1: u8 = 0x1E;
    const MINUS: u8 = 0x2D;
    const ESC: u8 = 0x29;
    const SPACE: u8 = 0x2C;
    const CAPS: u8 = 0x39;
    const H: u8 = 0x0B;
    const LEFT: u8 = 0x50;
    const LCTRL: u8 = 0xE0;
    const LSHIFT: u8 = 0xE1;
    const RSHIFT: u8 = 0xE5;
    const LSHIFT_BIT: u8 = 0x02;
    const RSHIFT_BIT: u8 = 0x20;
    const LCTRL_BIT: u8 = 0x01;
    const VOLUME_UP: u16 = 0xE9;
    const PLAY: u16 = 0xCD;

    fn keymap() -> Keymap {
        Keymap::new(STOCK_CONSUMER_USAGES)
    }

    fn keys(list: &[u8], modifiers: u8) -> Input {
        let mut keyboard = KeyboardReport { modifiers, keys: [0; ROLLOVER] };
        keyboard.keys[..list.len()].copy_from_slice(list);
        Input { keyboard, consumer_bits: 0 }
    }

    #[test]
    fn empty_keymap_passes_reports_through() {
        let map = keymap();
        let mut engine = Engine::new();
        let out = engine.process(&map, &keys(&[A, B], LSHIFT_BIT));
        assert_eq!(out.keyboard, KeyboardReport { modifiers: LSHIFT_BIT, keys: [A, B, 0, 0, 0, 0] });
        assert_eq!(out.events.len(), 3);
        let out = engine.process(&map, &keys(&[], 0));
        assert_eq!(out.keyboard, KeyboardReport::default());
        assert!(out.events.iter().all(|e| !e.pressed));
    }

    #[test]
    fn simultaneous_layer_and_key_press_resolves_the_key_on_the_new_layer() {
        let mut map = keymap();
        map.set(0, CAPS, Action::LayerHold(1)).unwrap();
        map.set(1, A, Action::Key(B)).unwrap();
        let mut engine = Engine::new();
        let out = engine.process(&map, &keys(&[A, CAPS], 0));
        assert_eq!(out.keyboard.keys, [B, 0, 0, 0, 0, 0]);
        assert_eq!(engine.top_layer(), 1);
        let out = engine.process(&map, &keys(&[A], 0));
        assert_eq!(out.keyboard.keys, [B, 0, 0, 0, 0, 0]);
        assert_eq!(engine.top_layer(), 0);
    }

    #[test]
    fn releasing_a_layer_before_a_new_key_uses_the_base_mapping() {
        let mut map = keymap();
        map.set(0, CAPS, Action::LayerHold(1)).unwrap();
        map.set(1, A, Action::Key(B)).unwrap();
        let mut engine = Engine::new();
        engine.process(&map, &keys(&[CAPS], 0));
        let out = engine.process(&map, &keys(&[A], 0));
        assert_eq!(out.keyboard.keys, [A, 0, 0, 0, 0, 0]);
        assert_eq!(engine.top_layer(), 0);
    }

    #[test]
    fn caps_as_control_and_disabled_keys() {
        let mut map = keymap();
        map.set(0, CAPS, Action::Modifiers(LCTRL_BIT)).unwrap();
        map.set(0, ESC, Action::Disabled).unwrap();
        let mut engine = Engine::new();
        let out = engine.process(&map, &keys(&[CAPS, A, ESC], 0));
        assert_eq!(out.keyboard, KeyboardReport { modifiers: LCTRL_BIT, keys: [A, 0, 0, 0, 0, 0] });
    }

    #[test]
    fn layer_hold_remaps_while_held_and_release_uses_the_press_action() {
        let mut map = keymap();
        map.set(0, CAPS, Action::LayerHold(1)).unwrap();
        map.set(1, H, Action::Key(LEFT)).unwrap();
        let mut engine = Engine::new();
        engine.process(&map, &keys(&[CAPS], 0));
        assert_eq!(engine.active_layers(), 0b11);
        let out = engine.process(&map, &keys(&[CAPS, H], 0));
        assert_eq!(out.keyboard.keys[0], LEFT);
        let out = engine.process(&map, &keys(&[H], 0));
        assert_eq!(out.keyboard.keys[0], LEFT);
        assert_eq!(engine.active_layers(), 0b01);
        engine.process(&map, &keys(&[], 0));
        let out = engine.process(&map, &keys(&[H], 0));
        assert_eq!(out.keyboard.keys[0], H);
    }

    #[test]
    fn layer_toggle_latches() {
        let mut map = keymap();
        map.set(0, ESC, Action::LayerToggle(2)).unwrap();
        map.set(2, A, Action::KeyWithModifiers { usage: B, modifiers: LSHIFT_BIT }).unwrap();
        let mut engine = Engine::new();
        engine.process(&map, &keys(&[ESC], 0));
        engine.process(&map, &keys(&[], 0));
        let out = engine.process(&map, &keys(&[A], 0));
        assert_eq!(out.keyboard, KeyboardReport { modifiers: LSHIFT_BIT, keys: [B, 0, 0, 0, 0, 0] });
        engine.process(&map, &keys(&[], 0));
        engine.process(&map, &keys(&[ESC], 0));
        engine.process(&map, &keys(&[], 0));
        assert_eq!(engine.process(&map, &keys(&[A], 0)).keyboard.keys[0], A);
    }

    #[test]
    fn consumer_bits_map_to_usages_and_can_be_remapped() {
        let mut map = keymap();
        let mut engine = Engine::new();
        let out = engine.process(&map, &Input { consumer_bits: 0b001, ..Input::default() });
        assert_eq!(out.consumer, VOLUME_UP);
        let out = engine.process(&map, &Input::default());
        assert_eq!(out.consumer, 0);
        map.set(0, CONSUMER_SOURCE_BASE, Action::Consumer(PLAY)).unwrap();
        map.set(0, CONSUMER_SOURCE_BASE + 1, Action::Key(A)).unwrap();
        let out = engine.process(&map, &Input { consumer_bits: 0b011, ..Input::default() });
        assert_eq!(out.consumer, PLAY);
        assert_eq!(out.keyboard.keys[0], A);
    }

    #[test]
    fn the_default_consumer_bits_follow_the_stock_report_descriptor() {
        let map = keymap();
        let mut engine = Engine::new();
        let usages: [u16; CONSUMER_BITS] = core::array::from_fn(|bit| {
            let out = engine.process(&map, &Input { consumer_bits: 1 << bit, ..Input::default() });
            let _ = engine.process(&map, &Input::default());
            out.consumer
        });
        assert_eq!(usages, [VOLUME_UP, 0xEA, PLAY, 0xCF, 0xB6, 0xB5, 0x223, 0]);
    }

    #[test]
    fn rollover_is_reported_and_held_state_survives() {
        let map = keymap();
        let mut engine = Engine::new();
        engine.process(&map, &keys(&[A], 0));
        let out = engine.process(&map, &keys(&[ERROR_ROLLOVER; 6], LCTRL_BIT));
        assert_eq!(out.keyboard.keys, [ERROR_ROLLOVER; ROLLOVER]);
        assert_eq!(out.keyboard.modifiers, LCTRL_BIT);
        let out = engine.process(&map, &keys(&[A], 0));
        assert_eq!(out.keyboard.keys[0], A);
        let mut map = keymap();
        for (i, s) in [A, B, 0x06, 0x07, 0x08, 0x09].iter().enumerate() {
            map.set(0, *s, Action::KeyWithModifiers { usage: 0x10 + i as u8, modifiers: 0 }).unwrap();
        }
        map.set(0, 0x0A, Action::Key(0x20)).unwrap();
        let mut engine = Engine::new();
        let out = engine.process(&map, &keys(&[A, B, 0x06, 0x07, 0x08, 0x09], 0));
        assert_eq!(out.keyboard.keys, [0x10, 0x11, 0x12, 0x13, 0x14, 0x15]);
        let _ = LCTRL;
    }

    #[test]
    fn keymap_validation_and_listing() {
        let mut map = keymap();
        assert_eq!(map.set(4, A, Action::Disabled), Err(KeymapError::Layer));
        assert_eq!(map.set(0, A, Action::LayerHold(0)), Err(KeymapError::Target));
        assert_eq!(map.set(0, A, Action::LayerToggle(4)), Err(KeymapError::Target));
        assert_eq!(map.set(0, A, Action::TapHoldMod { tap: ESC, hold_mods: 0 }), Err(KeymapError::Target));
        assert_eq!(map.set(0, A, Action::TapHoldLayer { tap: SPACE, hold_layer: 0 }), Err(KeymapError::Target));
        assert_eq!(map.set(0, A, Action::OneShotMod(0)), Err(KeymapError::Target));
        assert_eq!(map.set(0, A, Action::Macro(MACRO_SLOTS)), Err(KeymapError::Target));
        map.set(1, A, Action::Key(B)).unwrap();
        map.set(0, CAPS, Action::Modifiers(LCTRL_BIT)).unwrap();
        let entries: std::vec::Vec<_> = map.entries().collect();
        assert_eq!(entries, [(0, CAPS, Action::Modifiers(LCTRL_BIT)), (1, A, Action::Key(B))]);
        map.clear();
        assert_eq!(map.entries().count(), 0);
    }

    #[test]
    fn tap_hold_mod_taps_quickly_holds_on_timeout_and_permissively_holds_on_chord() {
        let mut map = keymap();
        map.set(0, CAPS, Action::TapHoldMod { tap: ESC, hold_mods: LCTRL_BIT }).unwrap();
        let mut engine = Engine::new();

        let out = engine.process_at(&map, &keys(&[CAPS], 0), 10);
        assert_eq!(out.keyboard, KeyboardReport::default());
        assert!(engine.has_pending_timers());
        let out = engine.process_at(&map, &keys(&[], 0), 80);
        assert_eq!(out.keyboard.keys[0], ESC);
        let released = engine.tick(&map, 80 + MACRO_STEP_MS).unwrap();
        assert_eq!(released.keyboard, KeyboardReport::default());

        let _ = engine.process_at(&map, &keys(&[CAPS], 0), 500);
        assert!(engine.tick(&map, 650).is_none());
        let held = engine.tick(&map, 700).unwrap();
        assert_eq!(held.keyboard.modifiers, LCTRL_BIT);
        let chord = engine.process_at(&map, &keys(&[CAPS, C], 0), 710);
        assert_eq!(chord.keyboard, KeyboardReport { modifiers: LCTRL_BIT, keys: [C, 0, 0, 0, 0, 0] });
        let _ = engine.process_at(&map, &keys(&[], 0), 750);

        let _ = engine.process_at(&map, &keys(&[CAPS], 0), 1000);
        let fast = engine.process_at(&map, &keys(&[CAPS, C], 0), 1030);
        assert_eq!(fast.keyboard, KeyboardReport { modifiers: LCTRL_BIT, keys: [C, 0, 0, 0, 0, 0] });
    }

    #[test]
    fn tap_hold_layer_activates_layer_on_chord_and_taps_space_when_released() {
        let mut map = keymap();
        map.set(0, SPACE, Action::TapHoldLayer { tap: SPACE, hold_layer: 1 }).unwrap();
        map.set(1, H, Action::Key(LEFT)).unwrap();
        let mut engine = Engine::new();
        let _ = engine.process_at(&map, &keys(&[SPACE], 0), 100);
        let chord = engine.process_at(&map, &keys(&[SPACE, H], 0), 130);
        assert_eq!(chord.keyboard.keys[0], LEFT);
        assert_eq!(engine.top_layer(), 1);
        let _ = engine.process_at(&map, &keys(&[], 0), 160);
        assert_eq!(engine.top_layer(), 0);
        let _ = engine.process_at(&map, &keys(&[SPACE], 0), 300);
        let tap = engine.process_at(&map, &keys(&[], 0), 350);
        assert_eq!(tap.keyboard.keys[0], SPACE);
    }

    #[test]
    fn one_shot_modifier_latches_for_next_key_or_expires_and_does_not_latch_when_held() {
        let mut map = keymap();
        map.set(0, LSHIFT, Action::OneShotMod(LSHIFT_BIT)).unwrap();
        let mut engine = Engine::new();
        let down = engine.process_at(&map, &keys(&[], LSHIFT_BIT), 0);
        assert_eq!(down.keyboard.modifiers, LSHIFT_BIT);
        let up = engine.process_at(&map, &keys(&[], 0), 50);
        assert_eq!(up.keyboard.modifiers, 0);
        let next = engine.process_at(&map, &keys(&[A], 0), 150);
        assert_eq!(next.keyboard, KeyboardReport { modifiers: LSHIFT_BIT, keys: [A, 0, 0, 0, 0, 0] });
        let _ = engine.process_at(&map, &keys(&[], 0), 180);
        let plain = engine.process_at(&map, &keys(&[B], 0), 200);
        assert_eq!(plain.keyboard, KeyboardReport { modifiers: 0, keys: [B, 0, 0, 0, 0, 0] });
        let _ = engine.process_at(&map, &keys(&[], 0), 220);

        let _ = engine.process_at(&map, &keys(&[], LSHIFT_BIT), 1000);
        let _ = engine.process_at(&map, &keys(&[], 0), 1040);
        assert!(engine.tick(&map, 4040).is_some());
        let after_timeout = engine.process_at(&map, &keys(&[A], 0), 4050);
        assert_eq!(after_timeout.keyboard.modifiers, 0);
        let _ = engine.process_at(&map, &keys(&[], 0), 4060);

        let _ = engine.process_at(&map, &keys(&[], LSHIFT_BIT), 5000);
        let chord = engine.process_at(&map, &keys(&[A], LSHIFT_BIT), 5020);
        assert_eq!(chord.keyboard.modifiers, LSHIFT_BIT);
        let _ = engine.process_at(&map, &keys(&[], LSHIFT_BIT), 5040);
        let _ = engine.process_at(&map, &keys(&[], 0), 5060);
        let not_latched = engine.process_at(&map, &keys(&[B], 0), 5080);
        assert_eq!(not_latched.keyboard.modifiers, 0);
    }

    #[test]
    fn macros_and_case_hotkeys_execute_cleanly() {
        let mut map = keymap();
        let mut steps = Vec::new();
        steps.push(MacroStep { usage: A, modifiers: LSHIFT_BIT }).unwrap();
        steps.push(MacroStep { usage: B, modifiers: 0 }).unwrap();
        map.set_macro(0, Some(MacroProgram { steps })).unwrap();
        map.set(0, CAPS, Action::Macro(0)).unwrap();
        map.set(0, ESC, Action::CaseControl(CaseHotkey::CycleEq)).unwrap();
        let mut engine = Engine::new();
        let out = engine.process_at(&map, &keys(&[CAPS, ESC], 0), 100);
        assert_eq!(out.case_hotkeys.as_slice(), &[CaseHotkey::CycleEq]);
        assert_eq!(out.keyboard, KeyboardReport { modifiers: LSHIFT_BIT, keys: [A, 0, 0, 0, 0, 0] });
        let gap = engine.tick(&map, 100 + MACRO_STEP_MS).unwrap();
        assert_eq!(gap.keyboard, KeyboardReport::default());
        let step2 = engine.tick(&map, 100 + 2 * MACRO_STEP_MS).unwrap();
        assert_eq!(step2.keyboard, KeyboardReport { modifiers: 0, keys: [B, 0, 0, 0, 0, 0] });
        let done = engine.tick(&map, 100 + 3 * MACRO_STEP_MS).unwrap();
        assert_eq!(done.keyboard, KeyboardReport::default());
    }

    #[test]
    fn caps_word_shifts_letters_and_hyphen_keeps_digits_and_stops_on_space() {
        let mut map = keymap();
        map.set(0, CAPS, Action::CapsWord).unwrap();
        let mut engine = Engine::new();
        let _ = engine.process_at(&map, &keys(&[CAPS], 0), 10);
        let _ = engine.process_at(&map, &keys(&[], 0), 20);
        assert!(engine.caps_word());
        let a = engine.process_at(&map, &keys(&[A], 0), 30);
        assert_eq!(a.keyboard, KeyboardReport { modifiers: LSHIFT_BIT, keys: [A, 0, 0, 0, 0, 0] });
        let _ = engine.process_at(&map, &keys(&[], 0), 40);
        let dash = engine.process_at(&map, &keys(&[MINUS], 0), 50);
        assert_eq!(dash.keyboard, KeyboardReport { modifiers: LSHIFT_BIT, keys: [MINUS, 0, 0, 0, 0, 0] });
        let _ = engine.process_at(&map, &keys(&[], 0), 60);
        let digit = engine.process_at(&map, &keys(&[NUM_1], 0), 70);
        assert_eq!(digit.keyboard, KeyboardReport { modifiers: 0, keys: [NUM_1, 0, 0, 0, 0, 0] });
        assert!(engine.caps_word());
        let _ = engine.process_at(&map, &keys(&[], 0), 80);
        let sp = engine.process_at(&map, &keys(&[SPACE], 0), 90);
        assert_eq!(sp.keyboard, KeyboardReport { modifiers: 0, keys: [SPACE, 0, 0, 0, 0, 0] });
        assert!(!engine.caps_word());
        let _ = engine.process_at(&map, &keys(&[], 0), 100);
        let lower = engine.process_at(&map, &keys(&[B], 0), 110);
        assert_eq!(lower.keyboard, KeyboardReport { modifiers: 0, keys: [B, 0, 0, 0, 0, 0] });
    }

    #[test]
    fn combos_fire_on_simultaneous_or_staggered_chord_and_preserve_single_taps() {
        let mut map = keymap();
        map.set_combo(0, Some(KeyCombo { key_a: J, key_b: K, action: Action::Key(ESC) })).unwrap();
        map.set_combo(1, Some(KeyCombo { key_a: LSHIFT, key_b: RSHIFT, action: Action::CapsWord })).unwrap();
        let mut engine = Engine::new();

        let sim = engine.process_at(&map, &keys(&[J, K], 0), 100);
        assert_eq!(sim.keyboard.keys[0], ESC);
        let rel = engine.process_at(&map, &keys(&[K], 0), 120);
        assert_eq!(rel.keyboard, KeyboardReport::default());
        let _ = engine.process_at(&map, &keys(&[], 0), 140);

        let p1 = engine.process_at(&map, &keys(&[J], 0), 200);
        assert_eq!(p1.keyboard, KeyboardReport::default());
        assert!(engine.has_fast_timers());
        let p2 = engine.process_at(&map, &keys(&[J, K], 0), 230);
        assert_eq!(p2.keyboard.keys[0], ESC);
        let _ = engine.process_at(&map, &keys(&[], 0), 260);

        let _ = engine.process_at(&map, &keys(&[J], 0), 400);
        let tap = engine.process_at(&map, &keys(&[], 0), 420);
        assert_eq!(tap.keyboard.keys[0], J);
        let done = engine.tick(&map, 420 + MACRO_STEP_MS).unwrap();
        assert_eq!(done.keyboard, KeyboardReport::default());

        let _ = engine.process_at(&map, &keys(&[J], 0), 600);
        let timeout = engine.tick(&map, 650).unwrap();
        assert_eq!(timeout.keyboard.keys[0], J);
        let _ = engine.process_at(&map, &keys(&[], 0), 680);

        let _ = engine.process_at(&map, &keys(&[], LSHIFT_BIT | RSHIFT_BIT), 800);
        assert!(engine.caps_word());
        let _ = engine.process_at(&map, &keys(&[], 0), 820);
        let _ = engine.process_at(&map, &keys(&[SPACE], 0), 840);
        let _ = engine.process_at(&map, &keys(&[], 0), 860);

        let _ = engine.process_at(&map, &keys(&[], LSHIFT_BIT), 1000);
        let _ = engine.process_at(&map, &keys(&[J], LSHIFT_BIT), 1015);
        let shift_j = engine.process_at(&map, &keys(&[], LSHIFT_BIT), 1035);
        assert_eq!(shift_j.keyboard, KeyboardReport { modifiers: LSHIFT_BIT, keys: [J, 0, 0, 0, 0, 0] });
        let _ = engine.process_at(&map, &keys(&[], 0), 1060);
        let _ = engine.tick(&map, 1060 + MACRO_STEP_MS);

        let sim_shift_j = engine.process_at(&map, &keys(&[J], LSHIFT_BIT), 1200);
        assert_eq!(sim_shift_j.keyboard, KeyboardReport { modifiers: LSHIFT_BIT, keys: [J, 0, 0, 0, 0, 0] });
        let _ = engine.process_at(&map, &keys(&[], 0), 1220);

        map.set_combo(2, Some(KeyCombo { key_a: A, key_b: B, action: Action::OneShotMod(LSHIFT_BIT) })).unwrap();
        let _ = engine.process_at(&map, &keys(&[A, B], 0), 1400);
        let _ = engine.process_at(&map, &keys(&[B], 0), 1420);
        let _ = engine.process_at(&map, &keys(&[], 0), 1440);
        let latched = engine.process_at(&map, &keys(&[C], 0), 1500);
        assert_eq!(latched.keyboard, KeyboardReport { modifiers: LSHIFT_BIT, keys: [C, 0, 0, 0, 0, 0] });
    }

    #[test]
    fn mouse_keys_emit_buttons_accelerated_cursor_motion_and_wheel() {
        let mut map = keymap();
        map.set(0, A, Action::Mouse(MouseAction::MoveRight)).unwrap();
        map.set(0, B, Action::Mouse(MouseAction::LeftClick)).unwrap();
        map.set(0, C, Action::Mouse(MouseAction::WheelUp)).unwrap();
        let mut engine = Engine::new();

        let first = engine.process_at(&map, &keys(&[A, B], 0), 0);
        assert_eq!(first.mouse, MouseReport { buttons: 1, dx: MOUSE_BASE_SPEED, dy: 0, wheel: 0 });
        assert!(engine.has_fast_timers());
        assert!(engine.tick(&map, 10).is_none());
        let step1 = engine.tick(&map, MOUSE_INTERVAL_MS).unwrap();
        assert_eq!(step1.mouse.buttons, 1);
        assert!(step1.mouse.dx >= MOUSE_BASE_SPEED);
        let fast = engine.tick(&map, 450).unwrap();
        assert_eq!(fast.mouse.dx, MOUSE_MAX_SPEED);
        let released = engine.process_at(&map, &keys(&[], 0), 470);
        assert_eq!(released.mouse, MouseReport::default());
        assert!(!engine.has_fast_timers());

        let wheel = engine.process_at(&map, &keys(&[C], 0), 500);
        assert_eq!(wheel.mouse.wheel, 1);
        let _ = engine.process_at(&map, &keys(&[], 0), 520);
    }
}
