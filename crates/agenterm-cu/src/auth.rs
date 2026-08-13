//! Per-target authorization for `agenterm-cu` (PRD_02_31).

use std::collections::BTreeSet;

/// Least-capability grants: observation and actuation are distinct.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum Grant {
    Observe,
    Actuate,
}

impl Grant {
    pub fn parse_many(raw: &str) -> BTreeSet<Self> {
        raw.split(',')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .filter_map(|part| match part.to_ascii_lowercase().as_str() {
                "observe" | "observation" => Some(Self::Observe),
                "actuate" | "actuation" | "act" => Some(Self::Actuate),
                _ => None,
            })
            .collect()
    }

    pub fn from_env() -> BTreeSet<Self> {
        std::env::var("AGENTERM_CU_GRANT")
            .map(|value| Self::parse_many(&value))
            .unwrap_or_default()
    }
}

pub struct Authorization {
    grants: BTreeSet<Grant>,
}

impl Authorization {
    pub fn new(grants: BTreeSet<Grant>) -> Self {
        Self { grants }
    }

    pub fn from_cli_and_env(cli_grant: Option<&str>) -> Self {
        let mut grants = Grant::from_env();
        if let Some(raw) = cli_grant {
            grants.extend(Grant::parse_many(raw));
        }
        Self::new(grants)
    }

    pub fn allows(&self, required: Grant) -> bool {
        self.grants.contains(&required)
    }
}
