// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
#![no_std]

mod flags;

pub mod audio;
pub mod bench;
pub mod developer;
pub mod device;
pub mod error;
pub mod events;
pub mod frame;
pub mod keys;
pub mod system;

use postcard_rpc::{TopicDirection, endpoints, topics};

pub use audio::*;
pub use bench::*;
pub use developer::*;
pub use device::*;
pub use error::*;
pub use events::*;
pub use keys::*;
pub use postcard_rpc::standard_icd::PingEndpoint;
pub use system::*;

pub const PROTOCOL_VERSION: u16 = 1;
pub const USB_VENDOR_ID: u16 = 0x1209;
pub const USB_PRODUCT_ID: u16 = 0x0001;
pub const VENDOR_INTERFACE_CLASS: u8 = 0xFF;
pub const VENDOR_INTERFACE_SUBCLASS: u8 = 0x4B;
pub const VENDOR_INTERFACE_PROTOCOL: u8 = 0x01;
pub const MAX_PAYLOAD: usize = 512;
pub const UNSERVED_PATHS: [&str; 1] = ["postcard-rpc/schemas/get"];

pub type CaseResult<T> = Result<T, CaseError>;
pub type Ack = CaseResult<()>;
pub type DiagnosticsResult = CaseResult<Diagnostics>;
pub type EventsResult = CaseResult<EventMask>;
pub type EqResult = CaseResult<Option<EqProfile>>;
pub type MicResult = CaseResult<MicSettings>;
pub type KeymapResult = CaseResult<KeymapChunk>;
pub type CountResult = CaseResult<u32>;
pub type SlotResult = CaseResult<SlotInfo>;
pub type WordsResult = CaseResult<WordBlock>;
pub type I2cResult = CaseResult<I2cReply>;
pub type FlashTestResult = CaseResult<FlashTestReport>;
pub type AudioBenchResult = CaseResult<AudioBench>;
pub type ImpedanceResult = CaseResult<ImpedanceReport>;
pub type MacroResult = CaseResult<Option<MacroProgram>>;
pub type ComboResult = CaseResult<Option<KeyCombo>>;
pub type TelemetryResult = CaseResult<SystemTelemetry>;
pub type EqSelection = Option<u8>;

endpoints! {
    list = ENDPOINT_LIST;
    | EndpointTy           | RequestTy        | ResponseTy        | Path                    |
    | ----------           | ---------        | ----------        | ----                    |
    | GetDeviceInfo        | ()               | DeviceInfo        | "device/info"           |
    | GetDiagnostics       | ()               | DiagnosticsResult | "system/diagnostics"    |
    | GetTelemetry         | ()               | TelemetryResult   | "system/telemetry"      |
    | SubscribeEvents      | EventMask        | EventsResult      | "events/subscribe"      |
    | GetAudioStatus       | ()               | AudioStatus       | "audio/status"          |
    | GetOutputSettings    | ()               | OutputSettings    | "audio/output/get"      |
    | SetOutputSettings    | OutputSettings   | Ack               | "audio/output/set"      |
    | GetDspSettings       | ()               | DspSettings       | "audio/dsp/get"         |
    | SetDspSettings       | DspSettings      | Ack               | "audio/dsp/set"         |
    | GetDacExtras         | ()               | DacExtras         | "audio/dac-extras/get"  |
    | SetDacExtras         | DacExtras        | Ack               | "audio/dac-extras/set"  |
    | MeasureImpedance     | ()               | ImpedanceResult   | "audio/impedance"       |
    | GetAudioMeters       | ()               | AudioMeters       | "audio/meters"          |
    | PlayTestTone         | TestToneRequest  | Ack               | "audio/test-tone"       |
    | GetEq                | u8               | EqResult          | "audio/eq/get"          |
    | SetEq                | EqSlotUpdate     | Ack               | "audio/eq/set"          |
    | SelectEq             | EqSelection      | Ack               | "audio/eq/select"       |
    | GetMic               | ()               | MicResult         | "audio/mic/get"         |
    | SetMic               | MicSettings      | Ack               | "audio/mic/set"         |
    | GetKeymap            | KeymapPage       | KeymapResult      | "keys/map/get"          |
    | SetKey               | KeymapEntry      | Ack               | "keys/map/set"          |
    | ClearKeymap          | ()               | Ack               | "keys/map/clear"        |
    | GetMacro             | u8               | MacroResult       | "keys/macro/get"        |
    | SetMacro             | MacroSlotUpdate  | Ack               | "keys/macro/set"        |
    | GetCombo             | u8               | ComboResult       | "keys/combo/get"        |
    | SetCombo             | ComboSlotUpdate  | Ack               | "keys/combo/set"        |
    | GetKeyConfig         | ()               | KeyConfig         | "keys/config/get"       |
    | SetKeyConfig         | KeyConfig        | Ack               | "keys/config/set"       |
    | GetKeyboardStatus    | ()               | KeyboardStatus    | "keys/status"           |
    | SaveSettings         | ()               | Ack               | "settings/save"         |
    | ResetSettings        | SettingsScope    | Ack               | "settings/reset"        |
    | GetSlots             | ()               | SlotTable         | "system/slots"          |
    | Reboot               | RebootTarget     | Ack               | "system/reboot"         |
    | ConfirmBoot          | ()               | Ack               | "system/confirm"        |
    | UpdateBegin          | UpdateManifest   | Ack               | "update/begin"          |
    | UpdateChunk          | UpdateBlock      | CountResult       | "update/chunk"          |
    | UpdateFinish         | ()               | SlotResult        | "update/finish"         |
    | DevUnlock            | DevUnlockRequest | Ack               | "dev/unlock"            |
    | DevPeek              | PeekRequest      | WordsResult       | "dev/peek"              |
    | DevPoke              | PokeRequest      | CountResult       | "dev/poke"              |
    | DevI2c               | I2cRequest       | I2cResult         | "dev/i2c"               |
    | DevUartSend          | ByteBlock        | Ack               | "dev/uart/send"         |
    | DevFault             | FaultKind        | Ack               | "dev/fault"             |
    | DevFlashTest         | ()               | FlashTestResult   | "dev/flash/test"        |
    | DevAudio             | AudioBenchRequest | AudioBenchResult | "dev/audio"             |
}

topics! {
    list = TOPICS_OUT_LIST;
    direction = TopicDirection::ToClient;
    | TopicTy              | MessageTy        | Path                    |
    | -------              | ---------        | ----                    |
    | AudioStatusTopic     | AudioStatus      | "audio/status/changed"  |
    | KeyEventTopic        | KeyEvent         | "keys/events"           |
    | LogTopic             | LogLine          | "log"                   |
    | UartMirrorTopic      | ByteBlock        | "dev/uart/mirror"       |
}

topics! {
    list = TOPICS_IN_LIST;
    direction = TopicDirection::ToServer;
    | TopicTy              | MessageTy        | Path                    |
    | -------              | ---------        | ----                    |
}
