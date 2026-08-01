use libp2p::identity;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

const KEY_FILE: &str = "identity.key";
const MARKER_FILE: &str = "identity.json";
const NEXT_FILE: &str = "identity.key.next";
const PREVIOUS_FILE: &str = "identity.key.previous";

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

#[derive(Debug, Serialize)]
pub struct IdentityLifecycleResult {
    pub schema: &'static str,
    pub operation: &'static str,
    pub peer_id: String,
    pub previous_peer_id: Option<String>,
    pub key_path: String,
    pub backup_path: Option<String>,
    pub marker_migrated: bool,
    pub interrupted_rotation_recovered: bool,
}

#[derive(Debug, Deserialize, Serialize)]
struct IdentityMarker {
    schema: String,
    peer_id: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct IdentityBackup {
    schema: String,
    peer_id: String,
    encoding: String,
    key_hex: String,
    created_unix_ms: u128,
}

#[derive(Default)]
struct RecoveryEvidence {
    interrupted_rotation_recovered: bool,
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
            recover_interrupted_rotation(state_dir)?;
            let path = state_dir.join(KEY_FILE);
            let marker_path = state_dir.join(MARKER_FILE);
            if path.exists() {
                let keypair = read_key(&path)?;
                ensure_marker(&marker_path, &keypair)?;
                return Ok(NodeIdentity {
                    keypair,
                    mode,
                    key_path: Some(path),
                    created: false,
                });
            }
            if marker_path.exists() {
                return Err(
                    "durable identity key is missing; restore the recorded identity explicitly"
                        .to_string(),
                );
            }
            let keypair = identity::Keypair::generate_ed25519();
            let bytes = encode_key(&keypair)?;
            write_new_private(&path, &bytes)?;
            if let Err(error) = write_marker(&marker_path, &keypair) {
                let _ = fs::remove_file(&path);
                return Err(error);
            }
            Ok(NodeIdentity {
                keypair,
                mode,
                key_path: Some(path),
                created: true,
            })
        }
    }
}

pub fn preflight_start(state_dir: &Path, mode: IdentityMode) -> Result<(), String> {
    if mode == IdentityMode::Ephemeral {
        return Ok(());
    }
    fs::create_dir_all(state_dir).map_err(|error| format!("create state directory: {error}"))?;
    recover_interrupted_rotation(state_dir)?;
    let key_path = state_dir.join(KEY_FILE);
    let marker_path = state_dir.join(MARKER_FILE);
    if key_path.exists() {
        let keypair = read_key(&key_path)?;
        ensure_marker(&marker_path, &keypair)?;
        return Ok(());
    }
    if marker_path.exists() {
        return Err(
            "durable identity key is missing; restore the recorded identity explicitly".to_string(),
        );
    }
    Ok(())
}

pub fn status(state_dir: &Path) -> Result<IdentityLifecycleResult, String> {
    let recovery = recover_interrupted_rotation(state_dir)?;
    let key_path = state_dir.join(KEY_FILE);
    let marker_path = state_dir.join(MARKER_FILE);
    if !key_path.exists() {
        if marker_path.exists() {
            return Err(
                "durable identity key is missing; restore the recorded identity explicitly"
                    .to_string(),
            );
        }
        return Err("durable identity has not been initialized".to_string());
    }
    let keypair = read_key(&key_path)?;
    let marker_migrated = ensure_marker(&marker_path, &keypair)?;
    Ok(lifecycle_result(
        "status",
        &keypair,
        None,
        &key_path,
        None,
        marker_migrated,
        recovery,
    ))
}

pub fn backup(state_dir: &Path, output: &Path) -> Result<IdentityLifecycleResult, String> {
    ensure_node_stopped(state_dir)?;
    let recovery = recover_interrupted_rotation(state_dir)?;
    let key_path = state_dir.join(KEY_FILE);
    let keypair = read_key(&key_path)?;
    let marker_migrated = ensure_marker(&state_dir.join(MARKER_FILE), &keypair)?;
    write_backup(output, &keypair)?;
    Ok(lifecycle_result(
        "backup",
        &keypair,
        None,
        &key_path,
        Some(output),
        marker_migrated,
        recovery,
    ))
}

pub fn rotate(state_dir: &Path, backup_path: &Path) -> Result<IdentityLifecycleResult, String> {
    ensure_node_stopped(state_dir)?;
    fs::create_dir_all(state_dir).map_err(|error| format!("create state directory: {error}"))?;
    let recovery = recover_interrupted_rotation(state_dir)?;
    let key_path = state_dir.join(KEY_FILE);
    let previous = read_key(&key_path)?;
    ensure_marker(&state_dir.join(MARKER_FILE), &previous)?;
    write_backup(backup_path, &previous)?;

    let next = identity::Keypair::generate_ed25519();
    let next_path = state_dir.join(NEXT_FILE);
    let previous_path = state_dir.join(PREVIOUS_FILE);
    write_new_private(&next_path, &encode_key(&next)?)?;
    fs::rename(&key_path, &previous_path)
        .map_err(|error| format!("prepare identity rotation: {error}"))?;
    if let Err(error) = fs::rename(&next_path, &key_path) {
        let _ = fs::rename(&previous_path, &key_path);
        let _ = fs::remove_file(&next_path);
        return Err(format!("commit identity rotation: {error}"));
    }
    if let Err(error) = write_marker_replace(&state_dir.join(MARKER_FILE), &next) {
        let _ = fs::remove_file(&key_path);
        let _ = fs::rename(&previous_path, &key_path);
        let _ = ensure_marker(&state_dir.join(MARKER_FILE), &previous);
        return Err(error);
    }
    fs::remove_file(&previous_path)
        .map_err(|error| format!("finish identity rotation: {error}"))?;

    Ok(lifecycle_result(
        "rotate",
        &next,
        Some(previous.public().to_peer_id().to_string()),
        &key_path,
        Some(backup_path),
        false,
        recovery,
    ))
}

pub fn restore(state_dir: &Path, backup_path: &Path) -> Result<IdentityLifecycleResult, String> {
    ensure_node_stopped(state_dir)?;
    fs::create_dir_all(state_dir).map_err(|error| format!("create state directory: {error}"))?;
    let recovery = recover_interrupted_rotation(state_dir)?;
    let key_path = state_dir.join(KEY_FILE);
    if key_path.exists() {
        return Err("durable identity already exists; restore refuses to overwrite it".to_string());
    }
    let marker_path = state_dir.join(MARKER_FILE);
    let keypair = read_backup(backup_path)?;
    if marker_path.exists() {
        let marker = read_marker(&marker_path)?;
        let restored_peer = keypair.public().to_peer_id().to_string();
        if marker.peer_id != restored_peer {
            return Err(format!(
                "backup peer ID {} does not match recorded identity {}",
                restored_peer, marker.peer_id
            ));
        }
    }
    write_new_private(&key_path, &encode_key(&keypair)?)?;
    if let Err(error) = write_marker_replace(&marker_path, &keypair) {
        let _ = fs::remove_file(&key_path);
        return Err(error);
    }
    Ok(lifecycle_result(
        "restore",
        &keypair,
        None,
        &key_path,
        Some(backup_path),
        false,
        recovery,
    ))
}

fn lifecycle_result(
    operation: &'static str,
    keypair: &identity::Keypair,
    previous_peer_id: Option<String>,
    key_path: &Path,
    backup_path: Option<&Path>,
    marker_migrated: bool,
    recovery: RecoveryEvidence,
) -> IdentityLifecycleResult {
    IdentityLifecycleResult {
        schema: "agenterm-net/identity-lifecycle/v1",
        operation,
        peer_id: keypair.public().to_peer_id().to_string(),
        previous_peer_id,
        key_path: key_path.display().to_string(),
        backup_path: backup_path.map(|path| path.display().to_string()),
        marker_migrated,
        interrupted_rotation_recovered: recovery.interrupted_rotation_recovered,
    }
}

fn recover_interrupted_rotation(state_dir: &Path) -> Result<RecoveryEvidence, String> {
    let key_path = state_dir.join(KEY_FILE);
    let next_path = state_dir.join(NEXT_FILE);
    let previous_path = state_dir.join(PREVIOUS_FILE);
    let mut evidence = RecoveryEvidence::default();

    if previous_path.exists() {
        evidence.interrupted_rotation_recovered = true;
        let previous = read_key(&previous_path)?;
        let previous_peer = previous.public().to_peer_id().to_string();
        let current = key_path.exists().then(|| read_key(&key_path)).transpose()?;
        let marker = state_dir
            .join(MARKER_FILE)
            .exists()
            .then(|| read_marker(&state_dir.join(MARKER_FILE)))
            .transpose()?;
        let current_committed = current.as_ref().is_some_and(|current| {
            marker
                .as_ref()
                .is_some_and(|marker| marker.peer_id == current.public().to_peer_id().to_string())
        });
        let previous_recorded = marker
            .as_ref()
            .is_none_or(|marker| marker.peer_id == previous_peer);
        if current_committed {
            fs::remove_file(&previous_path)
                .map_err(|error| format!("clean completed identity rotation: {error}"))?;
        } else if previous_recorded {
            if key_path.exists() {
                fs::remove_file(&key_path)
                    .map_err(|error| format!("remove corrupt rotated identity: {error}"))?;
            }
            fs::rename(&previous_path, &key_path)
                .map_err(|error| format!("recover interrupted identity rotation: {error}"))?;
        } else {
            return Err("identity rotation files do not match the recorded PeerId".to_string());
        }
    }
    if next_path.exists() {
        evidence.interrupted_rotation_recovered = true;
        fs::remove_file(&next_path)
            .map_err(|error| format!("discard uncommitted identity rotation: {error}"))?;
    }
    Ok(evidence)
}

fn ensure_node_stopped(state_dir: &Path) -> Result<(), String> {
    if state_dir.join("node.json").exists() {
        return Err("identity mutation requires an explicitly stopped node".to_string());
    }
    Ok(())
}

fn read_key(path: &Path) -> Result<identity::Keypair, String> {
    let bytes = fs::read(path).map_err(|error| format!("read durable identity: {error}"))?;
    identity::Keypair::from_protobuf_encoding(&bytes)
        .map_err(|error| format!("decode durable identity: {error}"))
}

fn encode_key(keypair: &identity::Keypair) -> Result<Vec<u8>, String> {
    keypair
        .to_protobuf_encoding()
        .map_err(|error| format!("encode durable identity: {error}"))
}

fn ensure_marker(path: &Path, keypair: &identity::Keypair) -> Result<bool, String> {
    let peer_id = keypair.public().to_peer_id().to_string();
    if path.exists() {
        let marker = read_marker(path)?;
        if marker.schema != "agenterm-net/identity-marker/v1" {
            return Err(format!(
                "unsupported identity marker schema {}",
                marker.schema
            ));
        }
        if marker.peer_id != peer_id {
            return Err(format!(
                "durable identity peer ID {peer_id} does not match marker {}",
                marker.peer_id
            ));
        }
        return Ok(false);
    }
    write_marker(path, keypair)?;
    Ok(true)
}

fn read_marker(path: &Path) -> Result<IdentityMarker, String> {
    let bytes = fs::read(path).map_err(|error| format!("read identity marker: {error}"))?;
    serde_json::from_slice(&bytes).map_err(|error| format!("decode identity marker: {error}"))
}

fn write_marker(path: &Path, keypair: &identity::Keypair) -> Result<(), String> {
    let marker = IdentityMarker {
        schema: "agenterm-net/identity-marker/v1".to_string(),
        peer_id: keypair.public().to_peer_id().to_string(),
    };
    let bytes = serde_json::to_vec(&marker).map_err(|error| error.to_string())?;
    write_new_private(path, &bytes)
}

fn write_marker_replace(path: &Path, keypair: &identity::Keypair) -> Result<(), String> {
    if path.exists() {
        fs::remove_file(path).map_err(|error| format!("replace identity marker: {error}"))?;
    }
    write_marker(path, keypair)
}

fn write_backup(path: &Path, keypair: &identity::Keypair) -> Result<(), String> {
    let backup = IdentityBackup {
        schema: "agenterm-net/identity-backup/v1".to_string(),
        peer_id: keypair.public().to_peer_id().to_string(),
        encoding: "libp2p-protobuf-hex".to_string(),
        key_hex: hex_encode(&encode_key(keypair)?),
        created_unix_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
    };
    let bytes = serde_json::to_vec(&backup).map_err(|error| error.to_string())?;
    write_new_private(path, &bytes)
        .map_err(|error| format!("write identity backup without overwrite: {error}"))
}

fn read_backup(path: &Path) -> Result<identity::Keypair, String> {
    let bytes = fs::read(path).map_err(|error| format!("read identity backup: {error}"))?;
    let backup: IdentityBackup = serde_json::from_slice(&bytes)
        .map_err(|error| format!("decode identity backup: {error}"))?;
    if backup.schema != "agenterm-net/identity-backup/v1"
        || backup.encoding != "libp2p-protobuf-hex"
    {
        return Err("unsupported identity backup format".to_string());
    }
    let keypair = identity::Keypair::from_protobuf_encoding(&hex_decode(&backup.key_hex)?)
        .map_err(|error| format!("decode backed-up identity: {error}"))?;
    let peer_id = keypair.public().to_peer_id().to_string();
    if peer_id != backup.peer_id {
        return Err("identity backup peer ID does not match its key".to_string());
    }
    Ok(keypair)
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn hex_decode(value: &str) -> Result<Vec<u8>, String> {
    if value.len() % 2 != 0 {
        return Err("identity backup key has invalid hex length".to_string());
    }
    (0..value.len())
        .step_by(2)
        .map(|index| {
            u8::from_str_radix(&value[index..index + 2], 16)
                .map_err(|_| "identity backup key contains invalid hex".to_string())
        })
        .collect()
}

fn write_new_private(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("create identity directory: {error}"))?;
    }
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|error| format!("create durable identity file: {error}"))?;
    if let Err(error) = file.write_all(bytes).and_then(|_| file.sync_all()) {
        drop(file);
        let _ = fs::remove_file(path);
        return Err(format!("write durable identity file: {error}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(path.join(MARKER_FILE).exists());
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn missing_durable_key_is_loss_not_a_new_identity() {
        let path = test_path("loss");
        load_or_create(&path, IdentityMode::Durable).unwrap();
        fs::remove_file(path.join(KEY_FILE)).unwrap();
        let error = match load_or_create(&path, IdentityMode::Durable) {
            Ok(_) => panic!("missing durable key must not silently rotate identity"),
            Err(error) => error,
        };
        assert!(error.contains("missing"));
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn backup_rotation_and_loss_restore_preserve_explicit_identities() {
        let path = test_path("lifecycle");
        let backup_path = path.join("backups/identity-v1.json");
        let original = load_or_create(&path, IdentityMode::Durable)
            .unwrap()
            .peer_id();
        let rotated = rotate(&path, &backup_path).unwrap();
        assert_eq!(rotated.previous_peer_id.as_deref(), Some(original.as_str()));
        assert_ne!(rotated.peer_id, original);
        fs::remove_file(path.join(KEY_FILE)).unwrap();
        fs::remove_file(path.join(MARKER_FILE)).unwrap();
        let restored = restore(&path, &backup_path).unwrap();
        assert_eq!(restored.peer_id, original);
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn interrupted_rotation_restores_last_known_good_key() {
        let path = test_path("interrupted");
        let original = load_or_create(&path, IdentityMode::Durable)
            .unwrap()
            .peer_id();
        fs::rename(path.join(KEY_FILE), path.join(PREVIOUS_FILE)).unwrap();
        write_new_private(
            &path.join(NEXT_FILE),
            &encode_key(&identity::Keypair::generate_ed25519()).unwrap(),
        )
        .unwrap();
        let observed = status(&path).unwrap();
        assert_eq!(observed.peer_id, original);
        assert!(observed.interrupted_rotation_recovered);
        assert!(!path.join(NEXT_FILE).exists());
        assert!(!path.join(PREVIOUS_FILE).exists());
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn rotation_before_marker_commit_rolls_back_new_key() {
        let path = test_path("pre-marker");
        let original = load_or_create(&path, IdentityMode::Durable)
            .unwrap()
            .peer_id();
        fs::rename(path.join(KEY_FILE), path.join(PREVIOUS_FILE)).unwrap();
        write_new_private(
            &path.join(KEY_FILE),
            &encode_key(&identity::Keypair::generate_ed25519()).unwrap(),
        )
        .unwrap();
        let observed = status(&path).unwrap();
        assert_eq!(observed.peer_id, original);
        assert!(observed.interrupted_rotation_recovered);
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn ephemeral_identity_leaves_no_key() {
        let path = test_path("ephemeral");
        fs::create_dir_all(&path).unwrap();
        let first = load_or_create(&path, IdentityMode::Ephemeral).unwrap();
        let second = load_or_create(&path, IdentityMode::Ephemeral).unwrap();
        assert_ne!(first.peer_id(), second.peer_id());
        assert!(!path.join(KEY_FILE).exists());
        fs::remove_dir_all(path).unwrap();
    }
}
