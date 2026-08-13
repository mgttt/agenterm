//! Explicit target references (PRD_02_30).

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TargetRef {
    Current,
}

impl TargetRef {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "current" => Some(Self::Current),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Current => "current",
        }
    }
}
