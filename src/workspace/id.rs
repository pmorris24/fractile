use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct WorkspaceId(u8);

impl WorkspaceId {
    pub const NUMBERED_COUNT: usize = 10;
    pub const COUNT: usize = Self::NUMBERED_COUNT + 26;
    pub const ONE: Self = Self(1);
    pub const TWO: Self = Self(2);
    pub const THREE: Self = Self(3);
    pub const FOUR: Self = Self(4);
    pub const FIVE: Self = Self(5);
    pub const SIX: Self = Self(6);
    pub const EIGHT: Self = Self(8);

    pub fn new(value: u8) -> Result<Self, String> {
        (1..=Self::NUMBERED_COUNT as u8)
            .contains(&value)
            .then_some(Self(value))
            .ok_or_else(|| format!("workspace must be 1 through 10 or A through Z, got {value}"))
    }

    pub fn named(letter: char) -> Result<Self, String> {
        let letter = letter.to_ascii_uppercase();
        letter
            .is_ascii_uppercase()
            .then_some(Self(11 + (letter as u8 - b'A')))
            .ok_or_else(|| format!("workspace must be 1 through 10 or A through Z, got {letter:?}"))
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        value.parse::<u8>().ok().map(Self::new).unwrap_or_else(|| {
            let mut characters = value.chars();
            match (characters.next(), characters.next()) {
                (Some(letter), None) => Self::named(letter),
                _ => Err(format!(
                    "workspace must be 1 through 10 or A through Z, got {value:?}"
                )),
            }
        })
    }

    #[must_use]
    pub const fn number(self) -> Option<u8> {
        if self.0 <= Self::NUMBERED_COUNT as u8 {
            Some(self.0)
        } else {
            None
        }
    }

    #[must_use]
    pub const fn letter(self) -> Option<char> {
        if self.0 > Self::NUMBERED_COUNT as u8 {
            Some((b'A' + self.0 - 11) as char)
        } else {
            None
        }
    }

    pub fn all() -> impl Iterator<Item = Self> {
        (0..Self::COUNT).map(Self::from_index)
    }

    pub fn numbered() -> impl Iterator<Item = Self> {
        Self::all().take(Self::NUMBERED_COUNT)
    }

    pub(crate) fn from_index(index: usize) -> Self {
        Self(
            u8::try_from(index.saturating_add(1))
                .unwrap_or(Self::COUNT as u8)
                .clamp(1, Self::COUNT as u8),
        )
    }

    #[must_use]
    pub(crate) const fn index(self) -> usize {
        (self.0 - 1) as usize
    }

    const fn named_letter(self) -> char {
        (b'A' + self.0.saturating_sub(11)) as char
    }
}

impl fmt::Display for WorkspaceId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.number() {
            Some(number) => write!(formatter, "{number}"),
            None => write!(formatter, "{}", self.named_letter()),
        }
    }
}

impl FromStr for WorkspaceId {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

impl Serialize for WorkspaceId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.number() {
            Some(number) => serializer.serialize_u8(number),
            None => serializer.serialize_char(self.named_letter()),
        }
    }
}

impl<'de> Deserialize<'de> for WorkspaceId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Value {
            Number(u8),
            Name(String),
        }
        match Value::deserialize(deserializer)? {
            Value::Number(value) => Self::new(value),
            Value::Name(value) => Self::parse(&value),
        }
        .map_err(serde::de::Error::custom)
    }
}
