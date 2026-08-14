//! Explicit target references (PRD_02_30).

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TargetRef {
    Current,
    /// Remote desktop reached by OpenSSH `ssh` exec of a remote `agenterm-cu
    /// --target current` worker. Same abstract command set; transport only.
    Ssh,
}

impl TargetRef {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "current" => Some(Self::Current),
            "ssh" => Some(Self::Ssh),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Current => "current",
            Self::Ssh => "ssh",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::TargetRef;

    #[test]
    fn parses_current_and_ssh() {
        assert_eq!(TargetRef::parse("current"), Some(TargetRef::Current));
        assert_eq!(TargetRef::parse("ssh"), Some(TargetRef::Ssh));
        assert_eq!(TargetRef::parse("rdp"), None);
        assert_eq!(TargetRef::Current.as_str(), "current");
        assert_eq!(TargetRef::Ssh.as_str(), "ssh");
    }
}
