use std::{fmt::Display, str::FromStr};

use serde::{Deserialize, Serialize};

use crate::{error::JPreprocessErrorKind, JPreprocessError};

#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
/// サ変
pub enum SaIrregular {
    /// スル
    Alone,
    /// －スル
    ConjugationSuru,
    /// －ズル
    ConjugationZuru,
}

impl FromStr for SaIrregular {
    type Err = JPreprocessError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "スル" => Ok(Self::Alone),
            "－スル" => Ok(Self::ConjugationSuru),
            "－ズル" => Ok(Self::ConjugationZuru),
            "−スル" => Ok(Self::ConjugationSuru),
            "−ズル" => Ok(Self::ConjugationZuru),
            _ => Err(JPreprocessErrorKind::CTypeParseError
                .with_error(anyhow::anyhow!("Parse failed in SaIrregular"))),
        }
    }
}

impl Display for SaIrregular {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match &self {
            Self::Alone => "スル",
            Self::ConjugationSuru => "−スル",
            Self::ConjugationZuru => "−ズル",
        })
    }
}
