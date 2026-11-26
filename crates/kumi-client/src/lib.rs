// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
use std::fmt;

use kumi_icd::{
    AudioBench, AudioBenchRequest, AudioMeters, AudioStart, AudioStatus, BYTE_BLOCK, ByteBlock, CaseError, CaseResult,
    ClearKeymap, ComboSlotUpdate, ConfirmBoot, DacExtras, DevAudio, DevFault, DevFlashTest, DevI2c, DevPeek, DevPoke,
    DevUartSend, DevUnlock, DevUnlockRequest, DeviceInfo, Diagnostics, DspSettings, EqProfile, EqSelection, EqSlotUpdate,
    EventMask, FaultKind, FlashTestReport, GetAudioMeters, GetAudioStatus, GetCombo, GetDacExtras, GetDeviceInfo, GetDiagnostics,
    GetDspSettings, GetEq, GetKeyConfig, GetKeyboardStatus, GetKeymap, GetMacro, GetMic, GetOutputSettings, GetSlots,
    GetTelemetry, I2cReply, I2cRequest, ImpedanceReport, KeyAction, KeyCombo, KeyConfig, KeyboardStatus, KeymapChunk,
    KeymapEntry, KeymapPage, LAYERS, MacroProgram, MacroSlotUpdate, MeasureImpedance, MicSettings, OutputSettings, PeekRequest,
    PingEndpoint, PlayTestTone, PokeRequest, Reboot, RebootTarget, ResetSettings, SaveSettings, SelectEq, SetCombo, SetDacExtras,
    SetDspSettings, SetEq, SetKey, SetKeyConfig, SetMacro, SetMic, SetOutputSettings, SettingsScope, SlotTable, SubscribeEvents,
    SystemTelemetry, TestToneRequest, WordBlock,
};
use postcard_rpc::{Endpoint, Key};
use serde::Serialize;
use serde::de::DeserializeOwned;

#[cfg(feature = "adb")]
pub mod bridge;
pub mod events;
pub mod update;
#[cfg(feature = "usb")]
pub mod usb;
#[cfg(feature = "virtual-case")]
pub mod virtual_case;
pub mod wire;

pub use events::Event;
pub use kumi_icd;
use wire::{FrameError, FrameLink, FrameTransport};

pub const PROTOCOL_VERSION: u16 = kumi_icd::PROTOCOL_VERSION;

pub trait Transport {
    type Error;

    fn request(&mut self, request: Key, response: Key, payload: &[u8]) -> Result<Vec<u8>, Self::Error>;
}

#[derive(Debug, PartialEq)]
#[non_exhaustive]
pub enum ClientError<E> {
    Transport(E),
    Encode,
    Decode,
    Case(CaseError),
}

impl<E> ClientError<E> {
    pub fn case_error(&self) -> Option<CaseError> {
        match self {
            ClientError::Case(error) => Some(*error),
            _ => None,
        }
    }

    pub fn is_unsupported(&self) -> bool {
        self.case_error() == Some(CaseError::Unsupported)
    }

    pub fn is_not_unlocked(&self) -> bool {
        self.case_error() == Some(CaseError::NotUnlocked)
    }
}

impl<E> ClientError<FrameError<E>> {
    pub fn is_unknown_endpoint(&self) -> bool {
        matches!(self, ClientError::Transport(FrameError::Wire(postcard_rpc::standard_icd::WireError::UnknownKey)))
    }

    pub fn is_link(&self) -> bool {
        matches!(self, ClientError::Transport(FrameError::Link(_) | FrameError::NoReply))
    }
}

impl<E: fmt::Display> fmt::Display for ClientError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ClientError::Transport(error) => write!(f, "transport: {error}"),
            ClientError::Encode => f.write_str("the request does not fit the protocol types"),
            ClientError::Decode => f.write_str("the reply does not decode as the endpoint's response"),
            ClientError::Case(error) => write!(f, "the case answered {error}"),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for ClientError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ClientError::Transport(error) => Some(error),
            ClientError::Case(error) => Some(error),
            _ => None,
        }
    }
}

pub struct Client<T> {
    transport: T,
}

impl<T: Transport> Client<T> {
    pub fn new(transport: T) -> Self {
        Self { transport }
    }

    pub fn transport_mut(&mut self) -> &mut T {
        &mut self.transport
    }

    pub fn into_transport(self) -> T {
        self.transport
    }

    pub fn call<E: Endpoint>(&mut self, request: &E::Request) -> Result<E::Response, ClientError<T::Error>>
    where
        E::Request: Serialize,
        E::Response: DeserializeOwned,
    {
        let payload = postcard::to_allocvec(request).map_err(|_| ClientError::Encode)?;
        let response = self.transport.request(E::REQ_KEY, E::RESP_KEY, &payload).map_err(ClientError::Transport)?;
        postcard::from_bytes::<E::Response>(&response).map_err(|_| ClientError::Decode)
    }

    pub fn call_case<E, R>(&mut self, request: &E::Request) -> Result<R, ClientError<T::Error>>
    where
        E: Endpoint<Response = CaseResult<R>>,
        E::Request: Serialize,
        R: DeserializeOwned,
    {
        self.call::<E>(request)?.map_err(ClientError::Case)
    }

    pub fn ping(&mut self, value: u32) -> Result<u32, ClientError<T::Error>> {
        self.call::<PingEndpoint>(&value)
    }

    pub fn device_info(&mut self) -> Result<DeviceInfo, ClientError<T::Error>> {
        self.call::<GetDeviceInfo>(&())
    }

    pub fn diagnostics(&mut self) -> Result<Diagnostics, ClientError<T::Error>> {
        self.call_case::<GetDiagnostics, _>(&())
    }

    pub fn telemetry(&mut self) -> Result<SystemTelemetry, ClientError<T::Error>> {
        self.call_case::<GetTelemetry, _>(&())
    }

    pub fn subscribe(&mut self, events: EventMask) -> Result<EventMask, ClientError<T::Error>> {
        self.call_case::<SubscribeEvents, _>(&events)
    }

    pub fn audio_status(&mut self) -> Result<AudioStatus, ClientError<T::Error>> {
        self.call::<GetAudioStatus>(&())
    }

    pub fn keyboard_status(&mut self) -> Result<KeyboardStatus, ClientError<T::Error>> {
        self.call::<GetKeyboardStatus>(&())
    }

    pub fn slots(&mut self) -> Result<SlotTable, ClientError<T::Error>> {
        self.call::<GetSlots>(&())
    }

    pub fn output(&mut self) -> Result<OutputSettings, ClientError<T::Error>> {
        self.call::<GetOutputSettings>(&())
    }

    pub fn set_output(&mut self, output: OutputSettings) -> Result<(), ClientError<T::Error>> {
        self.call_case::<SetOutputSettings, _>(&output)
    }

    pub fn dsp(&mut self) -> Result<DspSettings, ClientError<T::Error>> {
        self.call::<GetDspSettings>(&())
    }

    pub fn set_dsp(&mut self, dsp: DspSettings) -> Result<(), ClientError<T::Error>> {
        self.call_case::<SetDspSettings, _>(&dsp)
    }

    pub fn dac_extras(&mut self) -> Result<DacExtras, ClientError<T::Error>> {
        self.call::<GetDacExtras>(&())
    }

    pub fn set_dac_extras(&mut self, extras: DacExtras) -> Result<(), ClientError<T::Error>> {
        self.call_case::<SetDacExtras, _>(&extras)
    }

    pub fn measure_impedance(&mut self) -> Result<ImpedanceReport, ClientError<T::Error>> {
        self.call_case::<MeasureImpedance, _>(&())
    }

    pub fn audio_meters(&mut self) -> Result<AudioMeters, ClientError<T::Error>> {
        self.call::<GetAudioMeters>(&())
    }

    pub fn play_test_tone(&mut self, request: TestToneRequest) -> Result<(), ClientError<T::Error>> {
        self.call_case::<PlayTestTone, _>(&request)
    }

    pub fn eq(&mut self, slot: u8) -> Result<Option<EqProfile>, ClientError<T::Error>> {
        self.call_case::<GetEq, _>(&slot)
    }

    pub fn set_eq(&mut self, update: EqSlotUpdate) -> Result<(), ClientError<T::Error>> {
        self.call_case::<SetEq, _>(&update)
    }

    pub fn select_eq(&mut self, selection: EqSelection) -> Result<(), ClientError<T::Error>> {
        self.call_case::<SelectEq, _>(&selection)
    }

    pub fn mic(&mut self) -> Result<MicSettings, ClientError<T::Error>> {
        self.call_case::<GetMic, _>(&())
    }

    pub fn set_mic(&mut self, mic: MicSettings) -> Result<(), ClientError<T::Error>> {
        self.call_case::<SetMic, _>(&mic)
    }

    pub fn keymap(&mut self, page: KeymapPage) -> Result<KeymapChunk, ClientError<T::Error>> {
        self.call_case::<GetKeymap, _>(&page)
    }

    pub fn keymap_all(&mut self, layer: u8) -> Result<Vec<KeymapEntry>, ClientError<T::Error>> {
        let mut entries = Vec::new();
        let mut start = 0u8;
        loop {
            let chunk = self.keymap(KeymapPage { layer, start })?;
            let next = chunk.entries.last().map(|entry| entry.source.checked_add(1));
            entries.extend_from_slice(&chunk.entries);
            match next {
                Some(Some(next)) if chunk.more && next > start => start = next,
                _ => return Ok(entries),
            }
        }
    }

    pub fn keymap_layers(&mut self) -> Result<Vec<KeymapEntry>, ClientError<T::Error>> {
        let mut entries = Vec::new();
        for layer in 0..LAYERS {
            entries.extend(self.keymap_all(layer)?);
        }
        Ok(entries)
    }

    pub fn set_key(&mut self, entry: KeymapEntry) -> Result<(), ClientError<T::Error>> {
        self.call_case::<SetKey, _>(&entry)
    }

    pub fn unmap_key(&mut self, layer: u8, source: u8) -> Result<(), ClientError<T::Error>> {
        self.set_key(KeymapEntry { layer, source, action: KeyAction::Transparent })
    }

    pub fn clear_keymap(&mut self) -> Result<(), ClientError<T::Error>> {
        self.call_case::<ClearKeymap, _>(&())
    }

    pub fn macro_slot(&mut self, slot: u8) -> Result<Option<MacroProgram>, ClientError<T::Error>> {
        self.call_case::<GetMacro, _>(&slot)
    }

    pub fn set_macro(&mut self, update: MacroSlotUpdate) -> Result<(), ClientError<T::Error>> {
        self.call_case::<SetMacro, _>(&update)
    }

    pub fn combo_slot(&mut self, slot: u8) -> Result<Option<KeyCombo>, ClientError<T::Error>> {
        self.call_case::<GetCombo, _>(&slot)
    }

    pub fn set_combo(&mut self, update: ComboSlotUpdate) -> Result<(), ClientError<T::Error>> {
        self.call_case::<SetCombo, _>(&update)
    }

    pub fn key_config(&mut self) -> Result<KeyConfig, ClientError<T::Error>> {
        self.call::<GetKeyConfig>(&())
    }

    pub fn set_key_config(&mut self, config: KeyConfig) -> Result<(), ClientError<T::Error>> {
        self.call_case::<SetKeyConfig, _>(&config)
    }

    pub fn save_settings(&mut self) -> Result<(), ClientError<T::Error>> {
        self.call_case::<SaveSettings, _>(&())
    }

    pub fn reset_settings(&mut self, scope: SettingsScope) -> Result<(), ClientError<T::Error>> {
        self.call_case::<ResetSettings, _>(&scope)
    }

    pub fn reboot(&mut self, target: RebootTarget) -> Result<(), ClientError<T::Error>> {
        self.call_case::<Reboot, _>(&target)
    }

    pub fn confirm_boot(&mut self) -> Result<(), ClientError<T::Error>> {
        self.call_case::<ConfirmBoot, _>(&())
    }

    pub fn dev_unlock(&mut self, seconds: u16) -> Result<(), ClientError<T::Error>> {
        self.call_case::<DevUnlock, _>(&DevUnlockRequest { seconds })
    }

    pub fn dev_peek(&mut self, address: u32, words: u8) -> Result<WordBlock, ClientError<T::Error>> {
        self.call_case::<DevPeek, _>(&PeekRequest { address, words })
    }

    pub fn dev_poke(&mut self, address: u32, value: u32, mask: u32) -> Result<u32, ClientError<T::Error>> {
        self.call_case::<DevPoke, _>(&PokeRequest { address, value, mask })
    }

    pub fn dev_i2c(&mut self, bus: u8, address: u8, write: &[u8], read: u8) -> Result<I2cReply, ClientError<T::Error>> {
        let write = heapless::Vec::<u8, BYTE_BLOCK>::from_slice(write).map_err(|_| ClientError::Encode)?;
        self.call_case::<DevI2c, _>(&I2cRequest { bus, address, write, read })
    }

    pub fn dev_uart_send(&mut self, bytes: &[u8]) -> Result<(), ClientError<T::Error>> {
        let bytes = heapless::Vec::<u8, BYTE_BLOCK>::from_slice(bytes).map_err(|_| ClientError::Encode)?;
        self.call_case::<DevUartSend, _>(&ByteBlock { bytes })
    }

    pub fn dev_fault(&mut self, kind: FaultKind) -> Result<(), ClientError<T::Error>> {
        self.call_case::<DevFault, _>(&kind)
    }

    pub fn dev_flash_test(&mut self) -> Result<FlashTestReport, ClientError<T::Error>> {
        self.call_case::<DevFlashTest, _>(&())
    }

    pub fn audio_bench(&mut self, request: AudioBenchRequest) -> Result<AudioBench, ClientError<T::Error>> {
        self.call_case::<DevAudio, _>(&request)
    }

    pub fn audio_bench_calibrate(&mut self) -> Result<AudioBench, ClientError<T::Error>> {
        self.audio_bench(AudioBenchRequest::Calibrate)
    }

    pub fn audio_bench_start(&mut self, start: AudioStart) -> Result<AudioBench, ClientError<T::Error>> {
        self.audio_bench(AudioBenchRequest::Start(start))
    }

    pub fn audio_bench_stop(&mut self) -> Result<AudioBench, ClientError<T::Error>> {
        self.audio_bench(AudioBenchRequest::Stop)
    }

    pub fn audio_bench_status(&mut self) -> Result<AudioBench, ClientError<T::Error>> {
        self.audio_bench(AudioBenchRequest::Status)
    }
}

impl<L: FrameLink> Client<FrameTransport<L>> {
    pub fn over(link: L) -> Self {
        Self::new(FrameTransport::new(link))
    }

    pub fn link_mut(&mut self) -> &mut L {
        self.transport.link_mut()
    }

    pub fn events(&mut self, max_frames: usize) -> Result<Vec<Event>, ClientError<FrameError<L::Error>>> {
        self.transport.poll_events(max_frames).map_err(ClientError::Transport)
    }

    pub fn take_events(&mut self) -> Vec<Event> {
        self.transport.take_events()
    }
}
