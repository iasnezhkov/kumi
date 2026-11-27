// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
#![cfg_attr(not(test), no_std)]

use kumi_control::Controls;
use kumi_icd::frame;
use kumi_icd::{
    Ack, AudioBenchRequest, AudioBenchResult, AudioMeters, AudioSpectrum, AudioStatus, ByteBlock, CaseError, CountResult,
    DevUnlockRequest, DeviceInfo, DiagnosticsResult, EventMask, EventsResult, FaultKind, FlashTestResult, I2cRequest, I2cResult,
    ImpedanceResult, KeyboardStatus, PeekRequest, PokeRequest, PresenceStatus, RebootTarget, SettingsScope, SlotResult,
    SlotTable, TelemetryResult, TestToneRequest, UpdateBlock, UpdateManifest, WordsResult,
};
use kumi_icd::{
    ClearKeymap, ConfirmBoot, DevAudio, DevFault, DevFlashTest, DevI2c, DevPeek, DevPoke, DevUartSend, DevUnlock, GetAudioMeters,
    GetAudioSpectrum, GetAudioStatus, GetCombo, GetDacExtras, GetDeviceInfo, GetDiagnostics, GetDspExtras, GetDspSettings, GetEq,
    GetEqBase, GetKeyConfig, GetKeyboardStatus, GetKeymap, GetMacro, GetMediaGestures, GetMic, GetOutputSettings, GetPresence,
    GetSlots, GetTelemetry, GetToneEq, MeasureImpedance, PingEndpoint, PlayTestTone, Reboot, ResetSettings, SaveSettings,
    SelectEq, SetCombo, SetDacExtras, SetDspExtras, SetDspSettings, SetEq, SetEqBase, SetKey, SetKeyConfig, SetMacro,
    SetMediaGestures, SetMic, SetOutputSettings, SetToneEq, SubscribeEvents, UpdateBegin, UpdateChunk, UpdateFinish,
};
use kumi_settings::StoredSettings;
use postcard_rpc::header::VarKey;
use postcard_rpc::standard_icd::{ERROR_KEY, WireError};
use postcard_rpc::{Endpoint, Key};
use serde::Serialize;
use serde::de::DeserializeOwned;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispatchError {
    UnknownKey,
    KeyTooSmall,
    Decode,
    Encode,
}

impl From<DispatchError> for WireError {
    fn from(error: DispatchError) -> Self {
        match error {
            DispatchError::UnknownKey => WireError::UnknownKey,
            DispatchError::KeyTooSmall => WireError::KeyTooSmall,
            DispatchError::Decode => WireError::DeserFailed,
            DispatchError::Encode => WireError::SerFailed,
        }
    }
}

pub trait Provider {
    fn audio_supported(&self) -> bool {
        true
    }

    fn device_info(&mut self) -> DeviceInfo;
    fn audio_status(&mut self) -> AudioStatus;
    fn keyboard_status(&mut self) -> KeyboardStatus;
    fn slots(&mut self) -> SlotTable;
    fn save(&mut self, settings: &StoredSettings) -> Ack;
    fn reboot(&mut self, target: RebootTarget) -> Ack;
    fn confirm_boot(&mut self) -> Ack;

    fn diagnostics(&mut self) -> DiagnosticsResult {
        Err(CaseError::Unsupported)
    }
    fn telemetry(&mut self) -> TelemetryResult {
        Err(CaseError::Unsupported)
    }
    fn presence(&mut self) -> PresenceStatus {
        PresenceStatus::default()
    }
    fn require_tier1(&mut self) -> Ack {
        Ok(())
    }
    fn measure_impedance(&mut self) -> ImpedanceResult {
        Err(CaseError::Unsupported)
    }
    fn audio_meters(&mut self) -> AudioMeters {
        AudioMeters::default()
    }
    fn audio_spectrum(&mut self) -> AudioSpectrum {
        AudioSpectrum::default()
    }
    fn play_test_tone(&mut self, request: TestToneRequest) -> Ack {
        let _ = request;
        Err(CaseError::Unsupported)
    }
    fn subscribe(&mut self, events: EventMask) -> EventsResult {
        let _ = events;
        Err(CaseError::Unsupported)
    }
    fn update_begin(&mut self, manifest: UpdateManifest) -> Ack {
        let _ = manifest;
        Err(CaseError::Unsupported)
    }
    fn update_chunk(&mut self, block: UpdateBlock) -> CountResult {
        let _ = block;
        Err(CaseError::Unsupported)
    }
    fn update_finish(&mut self) -> SlotResult {
        Err(CaseError::Unsupported)
    }
    fn dev_unlock(&mut self, request: DevUnlockRequest) -> Ack {
        let _ = request;
        Err(CaseError::Unsupported)
    }
    fn dev_peek(&mut self, request: PeekRequest) -> WordsResult {
        let _ = request;
        Err(CaseError::Unsupported)
    }
    fn dev_poke(&mut self, request: PokeRequest) -> CountResult {
        let _ = request;
        Err(CaseError::Unsupported)
    }
    fn dev_i2c(&mut self, request: I2cRequest) -> I2cResult {
        let _ = request;
        Err(CaseError::Unsupported)
    }
    fn dev_uart(&mut self, block: ByteBlock) -> Ack {
        let _ = block;
        Err(CaseError::Unsupported)
    }
    fn dev_fault(&mut self, kind: FaultKind) -> Ack {
        let _ = kind;
        Err(CaseError::Unsupported)
    }
    fn dev_flash_test(&mut self) -> FlashTestResult {
        Err(CaseError::Unsupported)
    }
    fn dev_audio(&mut self, request: AudioBenchRequest) -> AudioBenchResult {
        let _ = request;
        Err(CaseError::Unsupported)
    }
}

fn reply<E, F>(payload: &[u8], out: &mut [u8], handler: F) -> Result<(Key, usize), DispatchError>
where
    E: Endpoint,
    E::Request: DeserializeOwned,
    E::Response: Serialize,
    F: FnOnce(E::Request) -> E::Response,
{
    let request = postcard::from_bytes::<E::Request>(payload).map_err(|_| DispatchError::Decode)?;
    let response = handler(request);
    let used = postcard::to_slice(&response, out).map_err(|_| DispatchError::Encode)?.len();
    Ok((E::RESP_KEY, used))
}

fn is<E: Endpoint>(key: &VarKey) -> bool {
    *key == VarKey::Key8(E::REQ_KEY)
}

fn valid(ok: bool) -> Result<(), CaseError> {
    if ok { Ok(()) } else { Err(CaseError::InvalidArgument) }
}

fn audio<P: Provider>(provider: &P) -> Result<(), CaseError> {
    if provider.audio_supported() { Ok(()) } else { Err(CaseError::Unsupported) }
}

pub fn dispatch<P: Provider>(
    controls: &mut Controls,
    provider: &mut P,
    key: Key,
    payload: &[u8],
    out: &mut [u8],
) -> Result<usize, DispatchError> {
    route(controls, provider, &VarKey::Key8(key), payload, out).map(|(_, used)| used)
}

fn route<P: Provider>(
    controls: &mut Controls,
    provider: &mut P,
    key: &VarKey,
    payload: &[u8],
    out: &mut [u8],
) -> Result<(Key, usize), DispatchError> {
    if matches!(key, VarKey::Key1(_)) {
        return Err(DispatchError::KeyTooSmall);
    }
    match key {
        k if is::<PingEndpoint>(k) => reply::<PingEndpoint, _>(payload, out, |value: u32| value),
        k if is::<GetDeviceInfo>(k) => reply::<GetDeviceInfo, _>(payload, out, |()| provider.device_info()),
        k if is::<GetDiagnostics>(k) => reply::<GetDiagnostics, _>(payload, out, |()| provider.diagnostics()),
        k if is::<GetTelemetry>(k) => reply::<GetTelemetry, _>(payload, out, |()| provider.telemetry()),
        k if is::<GetPresence>(k) => reply::<GetPresence, _>(payload, out, |()| provider.presence()),
        k if is::<SubscribeEvents>(k) => reply::<SubscribeEvents, _>(payload, out, |events| provider.subscribe(events)),
        k if is::<GetAudioStatus>(k) => reply::<GetAudioStatus, _>(payload, out, |()| provider.audio_status()),
        k if is::<GetOutputSettings>(k) => reply::<GetOutputSettings, _>(payload, out, |()| controls.output()),
        k if is::<SetOutputSettings>(k) => {
            reply::<SetOutputSettings, _>(payload, out, |value| audio(provider).and_then(|()| controls.set_output(value)))
        }
        k if is::<GetDspSettings>(k) => reply::<GetDspSettings, _>(payload, out, |()| controls.dsp()),
        k if is::<SetDspSettings>(k) => {
            reply::<SetDspSettings, _>(payload, out, |value| audio(provider).and_then(|()| controls.set_dsp(value)))
        }
        k if is::<GetDacExtras>(k) => reply::<GetDacExtras, _>(payload, out, |()| controls.dac_extras()),
        k if is::<SetDacExtras>(k) => {
            reply::<SetDacExtras, _>(payload, out, |value| audio(provider).and_then(|()| controls.set_dac_extras(value)))
        }
        k if is::<MeasureImpedance>(k) => {
            reply::<MeasureImpedance, _>(payload, out, |()| audio(provider).and_then(|()| provider.measure_impedance()))
        }
        k if is::<GetAudioMeters>(k) => reply::<GetAudioMeters, _>(payload, out, |()| provider.audio_meters()),
        k if is::<GetAudioSpectrum>(k) => reply::<GetAudioSpectrum, _>(payload, out, |()| provider.audio_spectrum()),
        k if is::<GetDspExtras>(k) => reply::<GetDspExtras, _>(payload, out, |()| controls.dsp_extras()),
        k if is::<SetDspExtras>(k) => {
            reply::<SetDspExtras, _>(payload, out, |value| audio(provider).and_then(|()| controls.set_dsp_extras(value)))
        }
        k if is::<GetEqBase>(k) => reply::<GetEqBase, _>(payload, out, |()| controls.eq_base()),
        k if is::<SetEqBase>(k) => {
            reply::<SetEqBase, _>(payload, out, |value| audio(provider).and_then(|()| controls.set_eq_base(value)))
        }
        k if is::<GetToneEq>(k) => reply::<GetToneEq, _>(payload, out, |()| controls.tone_eq()),
        k if is::<SetToneEq>(k) => {
            reply::<SetToneEq, _>(payload, out, |value| audio(provider).and_then(|()| controls.set_tone_eq(value)))
        }
        k if is::<PlayTestTone>(k) => reply::<PlayTestTone, _>(payload, out, |request: TestToneRequest| {
            audio(provider)?;
            valid(request.is_valid())?;
            provider.play_test_tone(request)
        }),
        k if is::<GetEq>(k) => reply::<GetEq, _>(payload, out, |slot| controls.eq(slot)),
        k if is::<SetEq>(k) => reply::<SetEq, _>(payload, out, |value| audio(provider).and_then(|()| controls.set_eq(value))),
        k if is::<SelectEq>(k) => {
            reply::<SelectEq, _>(payload, out, |value| audio(provider).and_then(|()| controls.select_eq(value)))
        }
        k if is::<GetMic>(k) => reply::<GetMic, _>(payload, out, |()| Ok(controls.mic())),
        k if is::<SetMic>(k) => reply::<SetMic, _>(payload, out, |value| audio(provider).and_then(|()| controls.set_mic(value))),
        k if is::<GetKeymap>(k) => reply::<GetKeymap, _>(payload, out, |page| controls.keymap(page)),
        k if is::<SetKey>(k) => reply::<SetKey, _>(payload, out, |entry| controls.set_key(entry)),
        k if is::<ClearKeymap>(k) => reply::<ClearKeymap, _>(payload, out, |()| controls.clear_keymap()),
        k if is::<GetMacro>(k) => reply::<GetMacro, _>(payload, out, |slot| controls.macro_slot(slot)),
        k if is::<SetMacro>(k) => reply::<SetMacro, _>(payload, out, |update| controls.set_macro(update)),
        k if is::<GetCombo>(k) => reply::<GetCombo, _>(payload, out, |slot| controls.combo_slot(slot)),
        k if is::<SetCombo>(k) => reply::<SetCombo, _>(payload, out, |update| controls.set_combo(update)),
        k if is::<GetKeyConfig>(k) => reply::<GetKeyConfig, _>(payload, out, |()| controls.key_config()),
        k if is::<SetKeyConfig>(k) => reply::<SetKeyConfig, _>(payload, out, |value| controls.set_key_config(value)),
        k if is::<GetMediaGestures>(k) => reply::<GetMediaGestures, _>(payload, out, |()| controls.media_gestures()),
        k if is::<SetMediaGestures>(k) => reply::<SetMediaGestures, _>(payload, out, |value| controls.set_media_gestures(value)),
        k if is::<GetKeyboardStatus>(k) => reply::<GetKeyboardStatus, _>(payload, out, |()| provider.keyboard_status()),
        k if is::<SaveSettings>(k) => reply::<SaveSettings, _>(payload, out, |()| {
            let res = provider.save(controls.settings());
            if res.is_ok() {
                controls.clear_dirty();
            }
            res
        }),
        k if is::<ResetSettings>(k) => reply::<ResetSettings, _>(payload, out, |scope| {
            if scope == SettingsScope::Audio {
                audio(provider)?;
            }
            controls.reset(scope)
        }),
        k if is::<GetSlots>(k) => reply::<GetSlots, _>(payload, out, |()| provider.slots()),
        k if is::<Reboot>(k) => reply::<Reboot, _>(payload, out, |target| provider.reboot(target)),
        k if is::<ConfirmBoot>(k) => reply::<ConfirmBoot, _>(payload, out, |()| provider.confirm_boot()),
        k if is::<UpdateBegin>(k) => reply::<UpdateBegin, _>(payload, out, |manifest| provider.update_begin(manifest)),
        k if is::<UpdateChunk>(k) => reply::<UpdateChunk, _>(payload, out, |block| provider.update_chunk(block)),
        k if is::<UpdateFinish>(k) => reply::<UpdateFinish, _>(payload, out, |()| provider.update_finish()),
        k if is::<DevUnlock>(k) => reply::<DevUnlock, _>(payload, out, |request: DevUnlockRequest| {
            valid(request.is_valid()).and_then(|()| provider.dev_unlock(request))
        }),
        k if is::<DevPeek>(k) => reply::<DevPeek, _>(payload, out, |request: PeekRequest| {
            valid(request.is_valid()).and_then(|()| provider.dev_peek(request))
        }),
        k if is::<DevPoke>(k) => reply::<DevPoke, _>(payload, out, |request: PokeRequest| {
            valid(request.is_valid()).and_then(|()| provider.dev_poke(request))
        }),
        k if is::<DevI2c>(k) => reply::<DevI2c, _>(payload, out, |request: I2cRequest| {
            valid(request.is_valid()).and_then(|()| provider.dev_i2c(request))
        }),
        k if is::<DevUartSend>(k) => reply::<DevUartSend, _>(payload, out, |block| provider.dev_uart(block)),
        k if is::<DevFault>(k) => reply::<DevFault, _>(payload, out, |kind| provider.dev_fault(kind)),
        k if is::<DevFlashTest>(k) => reply::<DevFlashTest, _>(payload, out, |()| provider.dev_flash_test()),
        k if is::<DevAudio>(k) => reply::<DevAudio, _>(payload, out, |request: AudioBenchRequest| {
            valid(request.is_valid()).and_then(|()| provider.dev_audio(request))
        }),
        _ => Err(DispatchError::UnknownKey),
    }
}

pub fn serve_frame<P: Provider>(controls: &mut Controls, provider: &mut P, request: &[u8], out: &mut [u8]) -> Option<usize> {
    let (header, body) = frame::decode(request)?;
    let header_len = frame::header_len(&header.seq_no);
    let (key, used) = match route(controls, provider, &header.key, body, out.get_mut(header_len..)?) {
        Ok(done) => done,
        Err(error) => (ERROR_KEY, postcard::to_slice(&WireError::from(error), out.get_mut(header_len..)?).ok()?.len()),
    };
    frame::write_header(key, header.seq_no, out)?;
    Some(header_len + used)
}
