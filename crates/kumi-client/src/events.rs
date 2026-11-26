// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
use kumi_icd::{AudioStatus, AudioStatusTopic, ByteBlock, KeyEvent, KeyEventTopic, LogLine, LogTopic, UartMirrorTopic};
use postcard_rpc::Topic;
use postcard_rpc::header::VarKey;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "path", content = "data")]
pub enum Event {
    #[serde(rename = "audio/status/changed")]
    Audio(AudioStatus),
    #[serde(rename = "keys/events")]
    Key(KeyEvent),
    #[serde(rename = "log")]
    Log(LogLine),
    #[serde(rename = "dev/uart/mirror")]
    Uart(ByteBlock),
}

fn matches<T: Topic>(key: &VarKey) -> bool {
    *key == VarKey::Key8(T::TOPIC_KEY)
}

fn message<T: Topic>(payload: &[u8]) -> Option<T::Message>
where
    T::Message: DeserializeOwned,
{
    postcard::from_bytes(payload).ok()
}

impl Event {
    pub fn is_topic(key: &VarKey) -> bool {
        matches::<AudioStatusTopic>(key)
            || matches::<KeyEventTopic>(key)
            || matches::<LogTopic>(key)
            || matches::<UartMirrorTopic>(key)
    }

    pub fn decode(key: &VarKey, payload: &[u8]) -> Option<Self> {
        match key {
            k if matches::<AudioStatusTopic>(k) => message::<AudioStatusTopic>(payload).map(Event::Audio),
            k if matches::<KeyEventTopic>(k) => message::<KeyEventTopic>(payload).map(Event::Key),
            k if matches::<LogTopic>(k) => message::<LogTopic>(payload).map(Event::Log),
            k if matches::<UartMirrorTopic>(k) => message::<UartMirrorTopic>(payload).map(Event::Uart),
            _ => None,
        }
    }

    pub fn path(&self) -> &'static str {
        match self {
            Event::Audio(_) => AudioStatusTopic::PATH,
            Event::Key(_) => KeyEventTopic::PATH,
            Event::Log(_) => LogTopic::PATH,
            Event::Uart(_) => UartMirrorTopic::PATH,
        }
    }
}
