// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
#![cfg_attr(not(test), no_std)]

use heapless::Vec;
use kumi_icd::{
    COMBO_SLOTS, CaseError, ComboSlotUpdate, DacExtras, DspExtras, DspSettings, EQ_SLOTS, EqBaseSettings, EqProfile, EqSelection,
    EqSlotUpdate, KEYMAP_CHUNK, KeyAction, KeyCombo, KeyConfig, KeymapChunk, KeymapEntry, KeymapPage, MACRO_SLOTS, MacroProgram,
    MacroSlotUpdate, MediaGestures, MicSettings, OutputSettings, SettingsScope, ToneEqSettings,
};
use kumi_settings::{KEYMAP_MAX, StoredSettings};

pub struct Controls {
    settings: StoredSettings,
    dirty: bool,
    keymap_dirty: bool,
}

fn slot_index(slot: u8) -> Result<usize, CaseError> {
    if slot < EQ_SLOTS { Ok(usize::from(slot)) } else { Err(CaseError::InvalidArgument) }
}

fn macro_index(slot: u8) -> Result<usize, CaseError> {
    if slot < MACRO_SLOTS { Ok(usize::from(slot)) } else { Err(CaseError::InvalidArgument) }
}

fn combo_index(slot: u8) -> Result<usize, CaseError> {
    if slot < COMBO_SLOTS { Ok(usize::from(slot)) } else { Err(CaseError::InvalidArgument) }
}

fn check(valid: bool) -> Result<(), CaseError> {
    if valid { Ok(()) } else { Err(CaseError::InvalidArgument) }
}

impl Controls {
    pub fn new(settings: StoredSettings) -> Self {
        Self { settings, dirty: false, keymap_dirty: false }
    }

    pub fn settings(&self) -> &StoredSettings {
        &self.settings
    }

    pub fn dirty(&self) -> bool {
        self.dirty
    }

    pub fn clear_dirty(&mut self) {
        self.dirty = false;
    }

    pub fn take_keymap_dirty(&mut self) -> bool {
        core::mem::replace(&mut self.keymap_dirty, false)
    }

    pub fn output(&self) -> OutputSettings {
        self.settings.output
    }

    pub fn set_output(&mut self, output: OutputSettings) -> Result<(), CaseError> {
        check(output.is_valid())?;
        if self.settings.output == output {
            return Ok(());
        }
        self.settings.output = output;
        self.dirty = true;
        Ok(())
    }

    pub fn dsp(&self) -> DspSettings {
        self.settings.dsp
    }

    pub fn set_dsp(&mut self, dsp: DspSettings) -> Result<(), CaseError> {
        if self.settings.dsp == dsp {
            return Ok(());
        }
        self.settings.dsp = dsp;
        self.dirty = true;
        Ok(())
    }

    pub fn dsp_extras(&self) -> DspExtras {
        self.settings.dsp_extras
    }

    pub fn set_dsp_extras(&mut self, extras: DspExtras) -> Result<(), CaseError> {
        check(extras.is_valid())?;
        if self.settings.dsp_extras == extras {
            return Ok(());
        }
        self.settings.dsp_extras = extras;
        self.dirty = true;
        Ok(())
    }

    pub fn eq_base(&self) -> EqBaseSettings {
        self.settings.eq_base.clone()
    }

    pub fn set_eq_base(&mut self, base: EqBaseSettings) -> Result<(), CaseError> {
        check(base.is_valid())?;
        if self.settings.eq_base == base {
            return Ok(());
        }
        self.settings.eq_base = base;
        self.dirty = true;
        Ok(())
    }

    pub fn tone_eq(&self) -> ToneEqSettings {
        self.settings.tone_eq
    }

    pub fn set_tone_eq(&mut self, tone: ToneEqSettings) -> Result<(), CaseError> {
        check(tone.is_valid())?;
        if self.settings.tone_eq == tone {
            return Ok(());
        }
        self.settings.tone_eq = tone;
        self.dirty = true;
        Ok(())
    }

    pub fn media_gestures(&self) -> MediaGestures {
        self.settings.media_gestures
    }

    pub fn set_media_gestures(&mut self, gestures: MediaGestures) -> Result<(), CaseError> {
        check(gestures.is_valid())?;
        if self.settings.media_gestures == gestures {
            return Ok(());
        }
        self.settings.media_gestures = gestures;
        self.dirty = true;
        self.keymap_dirty = true;
        Ok(())
    }

    pub fn dac_extras(&self) -> DacExtras {
        self.settings.dac_extras
    }

    pub fn set_dac_extras(&mut self, extras: DacExtras) -> Result<(), CaseError> {
        if self.settings.dac_extras == extras {
            return Ok(());
        }
        self.settings.dac_extras = extras;
        self.dirty = true;
        Ok(())
    }

    pub fn key_config(&self) -> KeyConfig {
        self.settings.key_config
    }

    pub fn set_key_config(&mut self, config: KeyConfig) -> Result<(), CaseError> {
        check(config.is_valid())?;
        if self.settings.key_config == config {
            return Ok(());
        }
        self.settings.key_config = config;
        self.dirty = true;
        self.keymap_dirty = true;
        Ok(())
    }

    pub fn macro_slot(&self, slot: u8) -> Result<Option<MacroProgram>, CaseError> {
        Ok(self.settings.macros[macro_index(slot)?].clone())
    }

    pub fn set_macro(&mut self, update: MacroSlotUpdate) -> Result<(), CaseError> {
        check(update.is_valid())?;
        let index = usize::from(update.slot);
        let value = if update.program.steps.is_empty() { None } else { Some(update.program) };
        if self.settings.macros[index] == value {
            return Ok(());
        }
        self.settings.macros[index] = value;
        self.dirty = true;
        self.keymap_dirty = true;
        Ok(())
    }

    pub fn combo_slot(&self, slot: u8) -> Result<Option<KeyCombo>, CaseError> {
        Ok(self.settings.combos[combo_index(slot)?])
    }

    pub fn set_combo(&mut self, update: ComboSlotUpdate) -> Result<(), CaseError> {
        check(update.is_valid())?;
        let index = usize::from(update.slot);
        if self.settings.combos[index] == update.combo {
            return Ok(());
        }
        self.settings.combos[index] = update.combo;
        self.dirty = true;
        self.keymap_dirty = true;
        Ok(())
    }

    pub fn mic(&self) -> MicSettings {
        self.settings.mic
    }

    pub fn set_mic(&mut self, mic: MicSettings) -> Result<(), CaseError> {
        check(mic.is_valid())?;
        if self.settings.mic == mic {
            return Ok(());
        }
        self.settings.mic = mic;
        self.dirty = true;
        Ok(())
    }

    pub fn eq(&self, slot: u8) -> Result<Option<EqProfile>, CaseError> {
        Ok(self.settings.eq_slots[slot_index(slot)?].clone())
    }

    pub fn set_eq(&mut self, update: EqSlotUpdate) -> Result<(), CaseError> {
        check(update.is_valid())?;
        let index = usize::from(update.slot);
        if self.settings.eq_slots[index].as_ref() == Some(&update.profile) {
            return Ok(());
        }
        self.settings.eq_slots[index] = Some(update.profile);
        self.dirty = true;
        Ok(())
    }

    pub fn active_eq(&self) -> EqSelection {
        self.settings.active_eq
    }

    pub fn select_eq(&mut self, selection: EqSelection) -> Result<(), CaseError> {
        if let Some(slot) = selection
            && self.settings.eq_slots[slot_index(slot)?].is_none()
        {
            return Err(CaseError::WrongState);
        }
        if self.settings.active_eq == selection {
            return Ok(());
        }
        self.settings.active_eq = selection;
        self.dirty = true;
        Ok(())
    }

    pub fn keymap(&self, page: KeymapPage) -> Result<KeymapChunk, CaseError> {
        check(page.is_valid())?;
        let mut matching: Vec<KeymapEntry, KEYMAP_MAX> = Vec::new();
        for entry in &self.settings.keymap {
            if entry.layer == page.layer && entry.source >= page.start {
                let _ = matching.push(*entry);
            }
        }
        for index in 1..matching.len() {
            let entry = matching[index];
            let mut at = index;
            while at > 0 && matching[at - 1].source > entry.source {
                matching[at] = matching[at - 1];
                at -= 1;
            }
            matching[at] = entry;
        }
        let mut entries: Vec<KeymapEntry, KEYMAP_CHUNK> = Vec::new();
        for entry in matching.iter().take(KEYMAP_CHUNK) {
            let _ = entries.push(*entry);
        }
        let more = matching.len() > KEYMAP_CHUNK;
        Ok(KeymapChunk { entries, more })
    }

    pub fn set_key(&mut self, entry: KeymapEntry) -> Result<(), CaseError> {
        check(entry.is_valid())?;
        let current = self.settings.keymap.iter().find(|e| e.layer == entry.layer && e.source == entry.source).map(|e| e.action);
        if current.unwrap_or(KeyAction::Transparent) == entry.action {
            return Ok(());
        }
        self.settings.keymap.retain(|e| !(e.layer == entry.layer && e.source == entry.source));
        if entry.action != KeyAction::Transparent && self.settings.keymap.push(entry).is_err() {
            return Err(CaseError::StorageFull);
        }
        self.dirty = true;
        self.keymap_dirty = true;
        Ok(())
    }

    pub fn clear_keymap(&mut self) -> Result<(), CaseError> {
        if self.settings.keymap.is_empty() {
            return Ok(());
        }
        self.settings.keymap.clear();
        self.dirty = true;
        self.keymap_dirty = true;
        Ok(())
    }

    pub fn reset(&mut self, scope: SettingsScope) -> Result<(), CaseError> {
        let before = self.settings.clone();
        let defaults = StoredSettings::default();
        match scope {
            SettingsScope::All => self.settings = defaults,
            SettingsScope::Audio => {
                self.settings.output = defaults.output;
                self.settings.dsp = defaults.dsp;
                self.settings.dsp_extras = defaults.dsp_extras;
                self.settings.dac_extras = defaults.dac_extras;
                self.settings.mic = defaults.mic;
                self.settings.eq_slots = defaults.eq_slots;
                self.settings.active_eq = defaults.active_eq;
                self.settings.eq_base = defaults.eq_base;
                self.settings.tone_eq = defaults.tone_eq;
            }
            SettingsScope::Keymap => {
                self.settings.keymap.clear();
                self.settings.key_config = defaults.key_config;
                self.settings.macros = defaults.macros;
                self.settings.combos = defaults.combos;
                self.settings.media_gestures = defaults.media_gestures;
            }
        }
        if self.settings != before {
            self.dirty = true;
            if before.keymap != self.settings.keymap
                || before.key_config != self.settings.key_config
                || before.macros != self.settings.macros
                || before.combos != self.settings.combos
                || before.media_gestures != self.settings.media_gestures
            {
                self.keymap_dirty = true;
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveTrigger {
    Tick,
    Sleeping,
    LinkDown,
}

pub struct AutoSave {
    settle_ticks: u32,
    pending_since: Option<u32>,
    first_pending_since: Option<u32>,
}

impl AutoSave {
    pub const fn new(settle_ticks: u32) -> Self {
        Self { settle_ticks, pending_since: None, first_pending_since: None }
    }

    pub fn record(&mut self, now: u32) {
        self.pending_since = Some(now);
        if self.first_pending_since.is_none() {
            self.first_pending_since = Some(now);
        }
    }

    pub fn pending(&self) -> bool {
        self.pending_since.is_some()
    }

    pub fn saved(&mut self) {
        self.pending_since = None;
        self.first_pending_since = None;
    }

    pub fn due(&self, now: u32, trigger: SaveTrigger) -> bool {
        match self.pending_since {
            None => false,
            Some(since) => match trigger {
                SaveTrigger::Sleeping | SaveTrigger::LinkDown => true,
                SaveTrigger::Tick => {
                    now.wrapping_sub(since) >= self.settle_ticks
                        || self
                            .first_pending_since
                            .is_some_and(|first| now.wrapping_sub(first) >= self.settle_ticks.saturating_mul(5))
                }
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keymap_dirty_tracks_only_keymap_changes_and_survives_clear_dirty() {
        let mut controls = Controls::new(StoredSettings::default());
        assert!(!controls.take_keymap_dirty());

        let mut dsp = controls.dsp();
        dsp.mono = true;
        controls.set_dsp(dsp).unwrap();
        assert!(controls.dirty());
        assert!(!controls.take_keymap_dirty());

        controls.set_key(KeymapEntry { layer: 0, source: 0x39, action: KeyAction::Key(0x29) }).unwrap();
        controls.clear_dirty();
        assert!(controls.take_keymap_dirty());
        assert!(!controls.take_keymap_dirty());

        controls.reset(SettingsScope::Audio).unwrap();
        assert!(!controls.take_keymap_dirty());
        controls.reset(SettingsScope::Keymap).unwrap();
        assert!(controls.take_keymap_dirty());
    }
}
