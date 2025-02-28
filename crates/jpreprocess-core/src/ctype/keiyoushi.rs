use std::{fmt::Display, str::FromStr};

use serde::{Deserialize, Serialize};

use crate::{error::JPreprocessErrorKind, JPreprocessError};

#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
/// 形容詞
pub enum Keiyoushi {
    /// アウオ段
    Auo,
    /// イ段
    I,
    // /// イイ
    // Ii,
}

impl FromStr for Keiyoushi {
    type Err = JPreprocessError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "アウオ段" => Ok(Self::Auo),
            "イ段" => Ok(Self::I),
            _ => Err(JPreprocessErrorKind::CTypeParseError
                .with_error(anyhow::anyhow!("Parse failed in Keiyoushi"))),
        }
    }
}

impl Display for Keiyoushi {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match &self {
            Self::Auo => "アウオ段",
            Self::I => "イ段",
        })
    }
}
