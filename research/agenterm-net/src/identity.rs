use libp2p::identity;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum IdentityMode {
    Ephemeral,
    Durable,
}

impl IdentityMode {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "ephemeral" => Ok(Self::Ephemeral),
            "durable" => Ok(Self::Durable),
            _ => Err("identity must be ephemeral or durable".to_string()),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Ephemeral => "ephemeral",
            Self::Durable => "durable",
        }
    }
}

pub struct NodeIdentity {
    pub keypair: identity::Keypair,
    pub mode: IdentityMode,
    pub key_path: Option<PathBuf>,
    pub created: bool,
}

impl NodeIdentity {
    pub fn peer_id(&self) -> String {
        self.keypair.public().to_peer_id().to_string()
    }
}

pub fn load_or_create(state_dir: &Path, mode: IdentityMode) -> Result<NodeIdentity, String> {
    match mode {
        IdentityMode::Ephemeral => Ok(NodeIdentity {
            keypair: identity::Keypair::generate_ed25519(),
            mode,
            key_path: None,
            created: true,
        }),
        IdentityMode::Durable => {
            fs::create_dir_all(state_dir)
                .map_err(|error| format!("create state directory: {error}"))?;
            let path = state_dir.join("identity.key");
            if path.exists() {
                let bytes =
                    fs::read(&path).map_err(|error| format!("read durable identity: {error}"))?;
                let keypair = identity::Keypair::from_protobuf_encoding(&bytes)
                    .map_err(|error| format!("decode durable identity: {error}"))?;
                return Ok(NodeIdentity {
                    keypair,
                    mode,
                    key_path: Some(path),
                    created: false,
                });
            }
            let keypair = identity::Keypair::generate_ed25519();
            let bytes = keypair
                .to_protobuf_encoding()
                .map_err(|error| format!("encode durable identity: {error}"))?;
            write_new_private(&path, &bytes)?;
            Ok(NodeIdentity {
                keypair,
                mode,
                key_path: Some(path),
                created: true,
            })
        }
    }
}

fn write_new_private(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|error| format!("create durable identity: {error}"))?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|error| format!("write durable identity: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_path(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "agenterm-net-identity-{label}-{}-{nonce:x}",
            std::process::id(),
        ))
    }

    #[test]
    fn durable_identity_survives_reload() {
        let path = test_path("durable");
        let first = load_or_create(&path, IdentityMode::Durable).unwrap();
        assert!(first.created);
        let second = load_or_create(&path, IdentityMode::Durable).unwrap();
        assert!(!second.created);
        assert_eq!(first.peer_id(), second.peer_id());
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn ephemeral_identity_leaves_no_key() {
        let path = test_path("ephemeral");
        fs::create_dir_all(&path).unwrap();
        let first = load_or_create(&path, IdentityMode::Ephemeral).unwrap();
        let second = load_or_create(&path, IdentityMode::Ephemeral).unwrap();
        assert_ne!(first.peer_id(), second.peer_id());
        assert!(!path.join("identity.key").exists());
        fs::remove_dir_all(path).unwrap();
    }
}
