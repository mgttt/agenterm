//! Explicit target references (PRD_02_30).

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TargetRef {
    Current,
    /// Remote desktop reached by OpenSSH `ssh` exec of a remote `agenterm-cu
    /// --target current` worker. Same abstract command set; transport only.
    Ssh,
    /// Desktop behind an RFB/VNC endpoint (`--vnc host[:port]`). First cut
    /// handshakes RFB (security type None / `-nopw`), then runs a local
    /// `agenterm-cu --target current` worker against the shared session
    /// (`DISPLAY` / AT-SPI env). Same abstract command set; transport only.
    Vnc,
}

impl TargetRef {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "current" => Some(Self::Current),
            "ssh" => Some(Self::Ssh),
            "vnc" => Some(Self::Vnc),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Current => "current",
            Self::Ssh => "ssh",
            Self::Vnc => "vnc",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::TargetRef;

    #[test]
    fn parses_current_ssh_and_vnc() {
        assert_eq!(TargetRef::parse("current"), Some(TargetRef::Current));
        assert_eq!(TargetRef::parse("ssh"), Some(TargetRef::Ssh));
        assert_eq!(TargetRef::parse("vnc"), Some(TargetRef::Vnc));
        assert_eq!(TargetRef::parse("rdp"), None);
        assert_eq!(TargetRef::Current.as_str(), "current");
        assert_eq!(TargetRef::Ssh.as_str(), "ssh");
        assert_eq!(TargetRef::Vnc.as_str(), "vnc");
    }
}
