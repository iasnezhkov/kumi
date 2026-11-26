// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
use core::fmt;

use serde::de::{self, DeserializeSeed, Deserializer, IgnoredAny, MapAccess, Visitor};
use serde::ser::{SerializeStruct, Serializer};

pub(crate) type Table = &'static [(&'static str, u32)];

const OTHER: &str = "other_bits";

pub(crate) fn known_bits(table: Table) -> u32 {
    table.iter().fold(0, |bits, (_, bit)| bits | bit)
}

pub(crate) fn debug(name: &str, bits: u32, table: Table, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(f, "{name}(")?;
    let mut first = true;
    for (key, bit) in table {
        if bits & bit != 0 {
            write!(f, "{}{key}", if first { "" } else { " | " })?;
            first = false;
        }
    }
    let other = bits & !known_bits(table);
    if other != 0 {
        write!(f, "{}{other:#x}", if first { "" } else { " | " })?;
    }
    write!(f, ")")
}

pub(crate) fn serialize<S: Serializer>(name: &'static str, bits: u32, table: Table, serializer: S) -> Result<S::Ok, S::Error> {
    if !serializer.is_human_readable() {
        return serializer.serialize_newtype_struct(name, &bits);
    }
    let other = bits & !known_bits(table);
    let mut out = serializer.serialize_struct(name, table.len() + usize::from(other != 0))?;
    for (key, bit) in table {
        out.serialize_field(key, &(bits & bit != 0))?;
    }
    if other != 0 {
        out.serialize_field(OTHER, &other)?;
    }
    out.end()
}

pub(crate) fn deserialize<'de, D: Deserializer<'de>>(name: &'static str, table: Table, deserializer: D) -> Result<u32, D::Error> {
    let visitor = Bits { table };
    if deserializer.is_human_readable() {
        deserializer.deserialize_any(visitor)
    } else {
        deserializer.deserialize_newtype_struct(name, visitor)
    }
}

struct Bits {
    table: Table,
}

impl<'de> Visitor<'de> for Bits {
    type Value = u32;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a flag set as an integer or a map of flag names to booleans")
    }

    fn visit_u64<E: de::Error>(self, value: u64) -> Result<u32, E> {
        u32::try_from(value).map_err(|_| E::invalid_value(de::Unexpected::Unsigned(value), &self))
    }

    fn visit_newtype_struct<D: Deserializer<'de>>(self, deserializer: D) -> Result<u32, D::Error> {
        <u32 as serde::Deserialize>::deserialize(deserializer)
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<u32, A::Error> {
        let mut bits = 0;
        while let Some(field) = map.next_key_seed(Field { table: self.table })? {
            match field {
                FieldKind::Flag(bit) => {
                    if map.next_value::<bool>()? {
                        bits |= bit;
                    }
                }
                FieldKind::Other => bits |= map.next_value::<u32>()? & !known_bits(self.table),
                FieldKind::Ignored => {
                    map.next_value::<IgnoredAny>()?;
                }
            }
        }
        Ok(bits)
    }
}

enum FieldKind {
    Flag(u32),
    Other,
    Ignored,
}

struct Field {
    table: Table,
}

impl<'de> DeserializeSeed<'de> for Field {
    type Value = FieldKind;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<FieldKind, D::Error> {
        deserializer.deserialize_str(self)
    }
}

impl<'de> Visitor<'de> for Field {
    type Value = FieldKind;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a flag name")
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<FieldKind, E> {
        if value == OTHER {
            return Ok(FieldKind::Other);
        }
        Ok(self.table.iter().find(|(key, _)| *key == value).map_or(FieldKind::Ignored, |(_, bit)| FieldKind::Flag(*bit)))
    }
}

macro_rules! flags {
    ($(#[$meta:meta])* $name:ident { $($flag:ident = $bit:literal, $key:literal;)* }) => {
        $(#[$meta])*
        #[derive(Clone, Copy, PartialEq, Eq, Hash, Default, postcard_schema::Schema)]
        pub struct $name(u32);

        impl $name {
            pub const NONE: Self = Self(0);
            $(pub const $flag: Self = Self(1 << $bit);)*
            pub const NAMES: &'static [(&'static str, u32)] = &[$(($key, 1 << $bit)),*];

            pub const fn from_bits(bits: u32) -> Self {
                Self(bits)
            }

            pub const fn bits(self) -> u32 {
                self.0
            }

            pub const fn is_empty(self) -> bool {
                self.0 == 0
            }

            pub const fn contains(self, other: Self) -> bool {
                self.0 & other.0 == other.0
            }

            pub const fn union(self, other: Self) -> Self {
                Self(self.0 | other.0)
            }

            pub const fn intersection(self, other: Self) -> Self {
                Self(self.0 & other.0)
            }

            pub const fn with(self, other: Self, enabled: bool) -> Self {
                if enabled { Self(self.0 | other.0) } else { Self(self.0 & !other.0) }
            }
        }

        impl core::ops::BitOr for $name {
            type Output = Self;

            fn bitor(self, other: Self) -> Self {
                self.union(other)
            }
        }

        impl core::ops::BitOrAssign for $name {
            fn bitor_assign(&mut self, other: Self) {
                *self = self.union(other);
            }
        }

        impl core::ops::BitAnd for $name {
            type Output = Self;

            fn bitand(self, other: Self) -> Self {
                self.intersection(other)
            }
        }

        impl core::fmt::Debug for $name {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                $crate::flags::debug(stringify!($name), self.0, Self::NAMES, f)
            }
        }

        impl serde::Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                $crate::flags::serialize(stringify!($name), self.0, Self::NAMES, serializer)
            }
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                $crate::flags::deserialize(stringify!($name), Self::NAMES, deserializer).map(Self)
            }
        }
    };
}

pub(crate) use flags;
