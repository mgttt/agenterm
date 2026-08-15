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
    /// Remote desktop reached by RDP (`--rdp host[:port]`). Cut 3.46 is a
    /// parseable fail-closed placeholder: no socket connect, no TLS/CredSSP,
    /// no session worker. Every authorized RDP command returns
    /// `rdp_unavailable`. Live transport/session/UIA evidence is a later
    /// Windows-agent cut.
    Rdp,
}

impl TargetRef {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "current" => Some(Self::Current),
            "ssh" => Some(Self::Ssh),
            "vnc" => Some(Self::Vnc),
            "rdp" => Some(Self::Rdp),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Current => "current",
            Self::Ssh => "ssh",
            Self::Vnc => "vnc",
            Self::Rdp => "rdp",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::TargetRef;

    #[test]
    fn parses_current_ssh_vnc_and_rdp() {
        assert_eq!(TargetRef::parse("current"), Some(TargetRef::Current));
        assert_eq!(TargetRef::parse("ssh"), Some(TargetRef::Ssh));
        assert_eq!(TargetRef::parse("vnc"), Some(TargetRef::Vnc));
        assert_eq!(TargetRef::parse("rdp"), Some(TargetRef::Rdp));
        assert_eq!(TargetRef::parse("rdp-gateway"), None);
        assert_eq!(TargetRef::Current.as_str(), "current");
        assert_eq!(TargetRef::Ssh.as_str(), "ssh");
        assert_eq!(TargetRef::Vnc.as_str(), "vnc");
        assert_eq!(TargetRef::Rdp.as_str(), "rdp");
    }

    #[test]
    fn rdp_serde_round_trip_is_rdp() {
        let raw = serde_json::to_string(&TargetRef::Rdp).expect("serialize");
        assert_eq!(raw, "\"rdp\"");
        let back: TargetRef = serde_json::from_str(&raw).expect("deserialize");
        assert_eq!(back, TargetRef::Rdp);
    }
}
