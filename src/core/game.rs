use std::path::PathBuf;

use anyhow::{Ok, bail};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Game {
    Bh3,
    Hk4e,
    Hkrpg,
    Nap,
}

impl TryFrom<&str> for Game {
    type Error = anyhow::Error;

    fn try_from(code: &str) -> Result<Self, Self::Error> {
        match code {
            "\x62\x68\x33" => Ok(Self::Bh3),
            "\x68\x6b\x34\x65" => Ok(Self::Hk4e),
            "\x68\x6b\x72\x70\x67" => Ok(Self::Hkrpg),
            "\x6e\x61\x70" => Ok(Self::Nap),
            _ => bail!("Unsupported game code: {code:?}"),
        }
    }
}

impl Game {
    pub fn code(self) -> &'static str {
        match self {
            Self::Bh3 => "\x62\x68\x33",
            Self::Hk4e => "\x68\x6b\x34\x65",
            Self::Hkrpg => "\x68\x6b\x72\x70\x67",
            Self::Nap => "\x6e\x61\x70",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Bh3 => "\x48\x6f\x6e\x6b\x61\x69\x20\x49\x6d\x70\x61\x63\x74\x20\x33\x72\x64",
            Self::Hk4e => "\x47\x65\x6e\x73\x68\x69\x6e\x20\x49\x6d\x70\x61\x63\x74",
            Self::Hkrpg => "\x48\x6f\x6e\x6b\x61\x69\x3a\x20\x53\x74\x61\x72\x20\x52\x61\x69\x6c",
            Self::Nap => "\x5a\x65\x6e\x6c\x65\x73\x73\x20\x5a\x6f\x6e\x65\x20\x5a\x65\x72\x6f",
        }
    }

    pub fn executable(self) -> &'static str {
        match self {
            Self::Bh3 => "\x42\x48\x33\x2e\x65\x78\x65",
            Self::Hk4e => "\x47\x65\x6e\x73\x68\x69\x6e\x49\x6d\x70\x61\x63\x74\x2e\x65\x78\x65",
            Self::Hkrpg => "\x53\x74\x61\x72\x52\x61\x69\x6c\x2e\x65\x78\x65",
            Self::Nap => {
                "\x5a\x65\x6e\x6c\x65\x73\x73\x5a\x6f\x6e\x65\x5a\x65\x72\x6f\x2e\x65\x78\x65"
            }
        }
    }

    pub fn install_path(self) -> PathBuf {
        PathBuf::from("games").join(self.code())
    }
}
