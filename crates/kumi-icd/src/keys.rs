// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
use heapless::Vec;
use postcard_schema::Schema;
use serde::{Deserialize, Serialize};

pub const LAYERS: u8 = 4;
pub const KEYMAP_CHUNK: usize = 32;
pub const MACRO_SLOTS: u8 = 8;
pub const MACRO_MAX_STEPS: usize = 16;
pub const COMBO_SLOTS: u8 = 8;

#[derive(Serialize, Deserialize, Schema, Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaseHotkey {
    CycleEq,
    CycleFilter,
    CycleCrossfeed,
    ToggleLimiter,
    ToggleMono,
    ToggleMute,
    ToggleLineOut,
    ToggleLoudness,
    ToggleCapsWord,
}

#[derive(Serialize, Deserialize, Schema, Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseAction {
    MoveUp,
    MoveDown,
    MoveLeft,
    MoveRight,
    LeftClick,
    RightClick,
    MiddleClick,
    WheelUp,
    WheelDown,
}

#[derive(Serialize, Deserialize, Schema, Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyAction {
    Transparent,
    Disabled,
    Key(u8),
    Modifiers(u8),
    KeyWithModifiers { usage: u8, modifiers: u8 },
    Consumer(u16),
    LayerHold(u8),
    LayerToggle(u8),
    TapHoldMod { tap: u8, hold_mods: u8 },
    TapHoldLayer { tap: u8, hold_layer: u8 },
    OneShotMod(u8),
    Macro(u8),
    CaseControl(CaseHotkey),
    CapsWord,
    Mouse(MouseAction),
}

#[derive(Serialize, Deserialize, Schema, Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyCombo {
    pub key_a: u8,
    pub key_b: u8,
    pub action: KeyAction,
}

impl KeyCombo {
    pub fn is_valid(&self) -> bool {
        let valid_source = |source: u8| source > 0x01;
        valid_source(self.key_a)
            && valid_source(self.key_b)
            && self.key_a != self.key_b
            && self.action != KeyAction::Transparent
            && self.action.is_valid()
    }
}

#[derive(Serialize, Deserialize, Schema, Debug, Clone, Copy, PartialEq, Eq)]
pub struct ComboSlotUpdate {
    pub slot: u8,
    pub combo: Option<KeyCombo>,
}

impl ComboSlotUpdate {
    pub fn is_valid(&self) -> bool {
        self.slot < COMBO_SLOTS && self.combo.is_none_or(|combo| combo.is_valid())
    }
}

#[derive(Serialize, Deserialize, Schema, Debug, Clone, Copy, PartialEq, Eq)]
pub struct MacroStep {
    pub usage: u8,
    pub modifiers: u8,
}

impl MacroStep {
    pub fn is_valid(&self) -> bool {
        (KeyAction::MIN_KEY_USAGE..=KeyAction::MAX_KEY_USAGE).contains(&self.usage)
    }
}

#[derive(Serialize, Deserialize, Schema, Debug, Clone, PartialEq, Eq, Default)]
pub struct MacroProgram {
    pub steps: Vec<MacroStep, MACRO_MAX_STEPS>,
}

impl MacroProgram {
    pub fn is_valid(&self) -> bool {
        self.steps.iter().all(MacroStep::is_valid)
    }
}

#[derive(Serialize, Deserialize, Schema, Debug, Clone, PartialEq, Eq)]
pub struct MacroSlotUpdate {
    pub slot: u8,
    pub program: MacroProgram,
}

impl MacroSlotUpdate {
    pub fn is_valid(&self) -> bool {
        self.slot < MACRO_SLOTS && self.program.is_valid()
    }
}

#[derive(Serialize, Deserialize, Schema, Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyConfig {
    pub tapping_term_ms: u16,
    pub one_shot_timeout_ms: u16,
    pub combo_term_ms: u16,
}

impl Default for KeyConfig {
    fn default() -> Self {
        Self { tapping_term_ms: 200, one_shot_timeout_ms: 3_000, combo_term_ms: 50 }
    }
}

impl KeyConfig {
    pub const MIN_TAPPING_TERM_MS: u16 = 80;
    pub const MAX_TAPPING_TERM_MS: u16 = 1_000;
    pub const MIN_ONE_SHOT_MS: u16 = 250;
    pub const MAX_ONE_SHOT_MS: u16 = 10_000;
    pub const MIN_COMBO_TERM_MS: u16 = 15;
    pub const MAX_COMBO_TERM_MS: u16 = 200;

    pub fn is_valid(&self) -> bool {
        (Self::MIN_TAPPING_TERM_MS..=Self::MAX_TAPPING_TERM_MS).contains(&self.tapping_term_ms)
            && (Self::MIN_ONE_SHOT_MS..=Self::MAX_ONE_SHOT_MS).contains(&self.one_shot_timeout_ms)
            && (Self::MIN_COMBO_TERM_MS..=Self::MAX_COMBO_TERM_MS).contains(&self.combo_term_ms)
    }
}

#[derive(Serialize, Deserialize, Schema, Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediaGestures {
    pub enabled: bool,
    pub window_ms: u16,
    pub idle_guard_ms: u16,
    pub double_shift_usage: u16,
    pub double_backspace_usage: u16,
    pub double_space_usage: u16,
}

impl Default for MediaGestures {
    fn default() -> Self {
        Self {
            enabled: false,
            window_ms: 320,
            idle_guard_ms: 1_200,
            double_shift_usage: 0xB6,
            double_backspace_usage: 0xB5,
            double_space_usage: 0xCD,
        }
    }
}

impl MediaGestures {
    pub const MIN_WINDOW_MS: u16 = 150;
    pub const MAX_WINDOW_MS: u16 = 600;
    pub const MAX_IDLE_GUARD_MS: u16 = 5_000;

    pub fn is_valid(&self) -> bool {
        (Self::MIN_WINDOW_MS..=Self::MAX_WINDOW_MS).contains(&self.window_ms)
            && self.idle_guard_ms <= Self::MAX_IDLE_GUARD_MS
            && self.double_shift_usage <= KeyAction::MAX_CONSUMER_USAGE
            && self.double_backspace_usage <= KeyAction::MAX_CONSUMER_USAGE
            && self.double_space_usage <= KeyAction::MAX_CONSUMER_USAGE
    }
}

#[derive(Serialize, Deserialize, Schema, Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeymapEntry {
    pub layer: u8,
    pub source: u8,
    pub action: KeyAction,
}

#[derive(Serialize, Deserialize, Schema, Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeymapPage {
    pub layer: u8,
    pub start: u8,
}

#[derive(Serialize, Deserialize, Schema, Debug, Clone, PartialEq, Eq)]
pub struct KeymapChunk {
    pub entries: Vec<KeymapEntry, KEYMAP_CHUNK>,
    pub more: bool,
}

#[derive(Serialize, Deserialize, Schema, Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyEvent {
    pub source: u8,
    pub pressed: bool,
    pub layer: u8,
    pub action: KeyAction,
}

#[derive(Serialize, Deserialize, Schema, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct KeyboardStatus {
    pub link_up: bool,
    pub frames: u32,
    pub bad_frames: u32,
    pub controller_commands: u32,
    pub active_layer: u8,
    pub caps_word: bool,
    pub host_leds: u8,
}

impl KeyAction {
    pub const MIN_KEY_USAGE: u8 = 0x04;
    pub const MAX_KEY_USAGE: u8 = 0xDF;
    pub const MAX_CONSUMER_USAGE: u16 = 0x03FF;

    pub fn is_valid(&self) -> bool {
        let key = |usage: &u8| (Self::MIN_KEY_USAGE..=Self::MAX_KEY_USAGE).contains(usage);
        match self {
            KeyAction::Transparent
            | KeyAction::Disabled
            | KeyAction::Modifiers(_)
            | KeyAction::CaseControl(_)
            | KeyAction::CapsWord
            | KeyAction::Mouse(_) => true,
            KeyAction::Key(usage) | KeyAction::KeyWithModifiers { usage, .. } => key(usage),
            KeyAction::Consumer(usage) => (1..=Self::MAX_CONSUMER_USAGE).contains(usage),
            KeyAction::LayerHold(layer) | KeyAction::LayerToggle(layer) => (1..LAYERS).contains(layer),
            KeyAction::TapHoldMod { tap, hold_mods } => key(tap) && *hold_mods != 0,
            KeyAction::TapHoldLayer { tap, hold_layer } => key(tap) && (1..LAYERS).contains(hold_layer),
            KeyAction::OneShotMod(mods) => *mods != 0,
            KeyAction::Macro(slot) => *slot < MACRO_SLOTS,
        }
    }
}

impl KeymapEntry {
    pub fn is_valid(&self) -> bool {
        self.layer < LAYERS && self.action.is_valid()
    }
}

impl KeymapPage {
    pub fn is_valid(&self) -> bool {
        self.layer < LAYERS
    }
}
