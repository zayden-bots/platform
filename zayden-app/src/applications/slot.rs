use std::str::FromStr;

use super::ApplicationError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Slot {
    Zayden,
    Maria,
}

impl Slot {
    pub const ALL: [Self; 2] = [Self::Zayden, Self::Maria];

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Zayden => "zayden",
            Self::Maria => "maria",
        }
    }

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Zayden => "Zayden",
            Self::Maria => "Maria",
        }
    }
}

impl FromStr for Slot {
    type Err = ApplicationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|slot| slot.as_str() == s)
            .ok_or_else(|| ApplicationError::UnknownSlot(s.to_owned()))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppKind {
    Default,
    Custom,
}

impl AppKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Custom => "custom",
        }
    }
}

impl FromStr for AppKind {
    type Err = ApplicationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "default" => Ok(Self::Default),
            "custom" => Ok(Self::Custom),
            _ => Err(ApplicationError::UnknownKind(s.to_owned())),
        }
    }
}
