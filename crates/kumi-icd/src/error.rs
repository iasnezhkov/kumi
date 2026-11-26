// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
use core::fmt;

use postcard_schema::Schema;
use postcard_schema::schema::{DataModelType, NamedType};
use serde::de::{self, Deserializer, Visitor};
use serde::{Deserialize, Serialize, Serializer};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CaseError {
    Unsupported,
    InvalidArgument,
    Busy,
    NotUnlocked,
    HardwareFault,
    StorageFull,
    VerifyFailed,
    WrongState,
    Unknown(u16),
}

impl CaseError {
    pub const KNOWN: [CaseError; 8] = [
        CaseError::Unsupported,
        CaseError::InvalidArgument,
        CaseError::Busy,
        CaseError::NotUnlocked,
        CaseError::HardwareFault,
        CaseError::StorageFull,
        CaseError::VerifyFailed,
        CaseError::WrongState,
    ];
    pub const NAMES: [&'static str; 8] =
        ["Unsupported", "InvalidArgument", "Busy", "NotUnlocked", "HardwareFault", "StorageFull", "VerifyFailed", "WrongState"];

    pub const fn code(self) -> u16 {
        match self {
            CaseError::Unsupported => 0,
            CaseError::InvalidArgument => 1,
            CaseError::Busy => 2,
            CaseError::NotUnlocked => 3,
            CaseError::HardwareFault => 4,
            CaseError::StorageFull => 5,
            CaseError::VerifyFailed => 6,
            CaseError::WrongState => 7,
            CaseError::Unknown(code) => code,
        }
    }

    pub const fn from_code(code: u16) -> Self {
        if (code as usize) < Self::KNOWN.len() { Self::KNOWN[code as usize] } else { CaseError::Unknown(code) }
    }

    pub fn name(self) -> Option<&'static str> {
        match self {
            CaseError::Unknown(_) => None,
            known => Some(Self::NAMES[usize::from(known.code())]),
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::NAMES.iter().position(|known| *known == name).map(|index| Self::KNOWN[index])
    }
}

impl fmt::Display for CaseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.name() {
            Some(name) => f.write_str(name),
            None => write!(f, "Error{}", self.code()),
        }
    }
}

impl core::error::Error for CaseError {}

impl Schema for CaseError {
    const SCHEMA: &'static NamedType = &NamedType { name: "CaseError", ty: &DataModelType::NewtypeStruct(u16::SCHEMA) };
}

impl Serialize for CaseError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if !serializer.is_human_readable() {
            return serializer.serialize_newtype_struct("CaseError", &self.code());
        }
        match self.name() {
            Some(name) => serializer.serialize_str(name),
            None => serializer.serialize_u16(self.code()),
        }
    }
}

impl<'de> Deserialize<'de> for CaseError {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        if deserializer.is_human_readable() {
            deserializer.deserialize_any(Code)
        } else {
            deserializer.deserialize_newtype_struct("CaseError", Code)
        }
    }
}

struct Code;

impl<'de> Visitor<'de> for Code {
    type Value = CaseError;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a case error name or code")
    }

    fn visit_u64<E: de::Error>(self, value: u64) -> Result<CaseError, E> {
        u16::try_from(value).map(CaseError::from_code).map_err(|_| E::invalid_value(de::Unexpected::Unsigned(value), &self))
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<CaseError, E> {
        CaseError::from_name(value).ok_or_else(|| E::unknown_variant(value, &CaseError::NAMES))
    }

    fn visit_newtype_struct<D: Deserializer<'de>>(self, deserializer: D) -> Result<CaseError, D::Error> {
        u16::deserialize(deserializer).map(CaseError::from_code)
    }
}
