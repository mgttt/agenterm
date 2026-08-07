use std::{
    collections::BTreeSet,
    ffi::{OsStr, OsString},
    fs::OpenOptions,
    io::Write,
    path::{Path, PathBuf},
    process::Command,
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const INCREMENTAL_WRAPPER_MODE: &str = "agenterm-incremental-manifest-v1";
const INCREMENTAL_IDENTITY_ALGORITHM: &str = "full-tree-metadata-v1";
const INCREMENTAL_MAX_ENTRIES_PER_ROOT: usize = 100_000;
const INCREMENTAL_MAX_DEPTH: usize = 64;
const INCREMENTAL_MAX_RELATIVE_BYTES: usize = 4096;

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct IncrementalRootSnapshot {
    name: String,
    identity: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct IncrementalBeforeSnapshot {
    schema_version: u32,
    kind: String,
    invocation_id: String,
    target: String,
    incremental_root: String,
    identity_algorithm: String,
    snapshot_complete: bool,
    cargo_lock_observed: bool,
    roots: Vec<IncrementalRootSnapshot>,
}

#[derive(Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct IncrementalTouchState {
    schema_version: u32,
    kind: String,
    invocation_id: String,
    complete: bool,
    rustc_invocations: u64,
    roots: BTreeSet<String>,
}

#[derive(Serialize)]
struct IncrementalManifestRoot {
    name: String,
    before_identity: String,
    after_identity: String,
    touched: bool,
}

#[derive(Serialize)]
struct IncrementalTouchManifest {
    kind: &'static str,
    schema_version: u32,
    invocation_id: String,
    target: String,
    incremental_root: String,
    snapshot_complete: bool,
    rustc_invocations: u64,
    identity_algorithm: &'static str,
    roots: Vec<IncrementalManifestRoot>,
}

fn safe_incremental_invocation_id(value: &str) -> bool {
    (16..=128).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
}

fn exact_absolute(path: &OsStr) -> anyhow::Result<PathBuf> {
    std::path::absolute(Path::new(path)).map_err(anyhow::Error::from)
}

fn direct_file(path: &Path) -> bool {
    crate::is_direct_file(path)
}

fn direct_directory(path: &Path) -> bool {
    crate::is_direct_directory(path)
}

fn modified_unix_millis(metadata: &std::fs::Metadata) -> anyhow::Result<u128> {
    Ok(metadata
        .modified()?
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| anyhow::anyhow!("incremental metadata predates epoch: {error}"))?
        .as_millis())
}

fn incremental_metadata_identity(root: &Path) -> anyhow::Result<String> {
    let root_metadata = std::fs::symlink_metadata(root)?;
    anyhow::ensure!(
        root_metadata.is_dir() && crate::is_direct_directory(root),
        "incremental root is not a direct directory"
    );
    let mut records = vec![format!(
        "d|.|{}|{}",
        root_metadata.len(),
        modified_unix_millis(&root_metadata)?
    )];
    let mut pending = vec![(root.to_path_buf(), 0_usize)];
    while let Some((directory, depth)) = pending.pop() {
        anyhow::ensure!(
            depth < INCREMENTAL_MAX_DEPTH,
            "incremental identity depth exceeded"
        );
        for entry in std::fs::read_dir(&directory)? {
            anyhow::ensure!(
                records.len() < INCREMENTAL_MAX_ENTRIES_PER_ROOT,
                "incremental identity entry count exceeded"
            );
            let entry = entry?;
            let path = entry.path();
            let metadata = std::fs::symlink_metadata(&path)?;
            anyhow::ensure!(
                crate::is_direct_directory(&path) || crate::is_direct_file(&path),
                "incremental identity encountered an indirect entry"
            );
            let relative = path
                .strip_prefix(root)
                .map_err(|_| anyhow::anyhow!("incremental identity escaped its root"))?
                .to_str()
                .ok_or_else(|| anyhow::anyhow!("incremental identity path is not UTF-8"))?
                .replace('\\', "/");
            anyhow::ensure!(
                relative.len() <= INCREMENTAL_MAX_RELATIVE_BYTES,
                "incremental identity relative path exceeded"
            );
            let kind = if metadata.is_dir() {
                pending.push((path, depth + 1));
                'd'
            } else if metadata.is_file() {
                'f'
            } else {
                anyhow::bail!("incremental identity encountered a special entry");
            };
            records.push(format!(
                "{kind}|{relative}|{}|{}",
                metadata.len(),
                modified_unix_millis(&metadata)?
            ));
        }
    }
    records.sort();
    let serialized = serde_json::to_vec(&records)?;
    Ok(Sha256::digest(serialized)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn snapshot_incremental_roots(root: &Path) -> anyhow::Result<Vec<IncrementalRootSnapshot>> {
    anyhow::ensure!(
        direct_directory(root),
        "incremental directory is absent or indirect"
    );
    let mut roots = Vec::new();
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        let metadata = std::fs::symlink_metadata(entry.path())?;
        if !metadata.is_dir() {
            continue;
        }
        anyhow::ensure!(
            crate::is_direct_directory(&entry.path()),
            "incremental compilation-unit root is indirect"
        );
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| anyhow::anyhow!("incremental root name is not UTF-8"))?;
        roots.push(IncrementalRootSnapshot {
            identity: incremental_metadata_identity(&entry.path())?,
            name,
        });
    }
    roots.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(roots)
}

fn atomic_write_json(path: &Path, value: &impl Serialize) -> anyhow::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("incremental state path has no parent"))?;
    anyhow::ensure!(
        direct_directory(parent),
        "incremental state parent is not direct"
    );
    let temporary = parent.join(format!(
        ".{}.{}-{}.tmp",
        path.file_name().and_then(OsStr::to_str).unwrap_or("state"),
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    ));
    let bytes = serde_json::to_vec(value)?;
    let mut output = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)?;
    output.write_all(&bytes)?;
    output.sync_all()?;
    drop(output);
    if let Err(error) = crate::replace_file(&temporary, path) {
        let _ = std::fs::remove_file(&temporary);
        return Err(error.into());
    }
    crate::sync_parent(parent)?;
    Ok(())
}

fn read_bounded_json<T: for<'de> Deserialize<'de>>(path: &Path) -> anyhow::Result<T> {
    let metadata = std::fs::symlink_metadata(path)?;
    anyhow::ensure!(
        metadata.is_file() && crate::is_direct_file(path) && metadata.len() <= 4 * 1024 * 1024,
        "incremental state file is absent, indirect, or oversized"
    );
    Ok(serde_json::from_slice(&std::fs::read(path)?)?)
}

fn incremental_poison_present(path: &Path) -> bool {
    match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Ok(_) | Err(_) => true,
    }
}

fn cargo_lock_is_held(path: &Path) -> bool {
    let Ok(file) = OpenOptions::new().read(true).write(true).open(path) else {
        return false;
    };
    match file.try_lock() {
        Ok(()) => false,
        Err(std::fs::TryLockError::WouldBlock) => true,
        Err(std::fs::TryLockError::Error(_)) => false,
    }
}

fn incremental_argument(arguments: &[OsString]) -> Option<anyhow::Result<PathBuf>> {
    let mut found = None;
    let mut index = 0;
    while index < arguments.len() {
        let argument = arguments[index].to_string_lossy();
        let value = if argument == "-C" {
            index += 1;
            arguments
                .get(index)
                .and_then(|value| value.to_str())
                .and_then(|value| value.strip_prefix("incremental="))
        } else {
            argument.strip_prefix("-Cincremental=")
        };
        if let Some(value) = value {
            if found.is_some() {
                return Some(Err(anyhow::anyhow!(
                    "rustc invocation has multiple incremental roots"
                )));
            }
            found = Some(exact_absolute(OsStr::new(value)));
        }
        index += 1;
    }
    found
}

fn has_crate_name(arguments: &[OsString]) -> bool {
    arguments
        .windows(2)
        .any(|pair| pair[0] == "--crate-name" && !pair[1].is_empty())
}

fn incremental_wrapper_environment() -> anyhow::Result<(String, PathBuf, PathBuf, PathBuf)> {
    anyhow::ensure!(
        std::env::var("AGENTERM_INTERNAL_RUSTC_WRAPPER").as_deref() == Ok(INCREMENTAL_WRAPPER_MODE),
        "incremental wrapper mode is not internally authorized"
    );
    let invocation = std::env::var("AGENTERM_INCREMENTAL_INVOCATION_ID")?;
    anyhow::ensure!(
        safe_incremental_invocation_id(&invocation),
        "incremental invocation identity is invalid"
    );
    let target = exact_absolute(OsStr::new(&std::env::var("AGENTERM_INCREMENTAL_TARGET")?))?;
    let incremental = exact_absolute(OsStr::new(&std::env::var("AGENTERM_INCREMENTAL_ROOT")?))?;
    anyhow::ensure!(
        incremental == target.join("debug").join("incremental"),
        "incremental root does not match the exact debug target"
    );
    let state = exact_absolute(OsStr::new(&std::env::var("AGENTERM_INCREMENTAL_STATE")?))?;
    anyhow::ensure!(
        state
            == target
                .join("debug")
                .join(".agenterm-incremental")
                .join(&invocation),
        "incremental state path does not match the invocation"
    );
    let configured_wrapper = exact_absolute(OsStr::new(&std::env::var("RUSTC_WRAPPER")?))?;
    anyhow::ensure!(
        direct_file(&configured_wrapper),
        "stable rustc wrapper is indirect"
    );
    anyhow::ensure!(
        std::fs::canonicalize(configured_wrapper)?
            == std::fs::canonicalize(std::env::current_exe()?)?,
        "running rustc wrapper does not match RUSTC_WRAPPER"
    );
    Ok((invocation, target, incremental, state))
}

fn coordinate_incremental_wrapper(arguments: &[OsString]) -> anyhow::Result<()> {
    let (invocation, target, incremental, state) = incremental_wrapper_environment()?;
    let Some(incremental_argument) = incremental_argument(arguments) else {
        return Ok(());
    };
    if !has_crate_name(arguments) {
        return Ok(());
    }
    let touched_path = incremental_argument?;
    let touched_name = touched_path
        .file_name()
        .and_then(OsStr::to_str)
        .ok_or_else(|| anyhow::anyhow!("incremental touched root has no UTF-8 basename"))?
        .to_owned();
    anyhow::ensure!(
        touched_path.parent() == Some(incremental.as_path()) && !touched_name.is_empty(),
        "rustc incremental root is outside the exact incremental directory"
    );
    anyhow::ensure!(
        direct_directory(&state),
        "incremental state directory is indirect"
    );
    let barrier_path = state.join("barrier.lock");
    let barrier = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&barrier_path)?;
    barrier.lock()?;

    let before_path = state.join("before.json");
    if !before_path.exists() {
        let cargo_lock_observed = cargo_lock_is_held(&target.join("debug").join(".cargo-lock"));
        let snapshot = snapshot_incremental_roots(&incremental);
        let snapshot_complete = cargo_lock_observed && snapshot.is_ok();
        atomic_write_json(
            &before_path,
            &IncrementalBeforeSnapshot {
                schema_version: 1,
                kind: "agenterm-incremental-before-snapshot".to_owned(),
                invocation_id: invocation.clone(),
                target: target.to_string_lossy().into_owned(),
                incremental_root: incremental.to_string_lossy().into_owned(),
                identity_algorithm: INCREMENTAL_IDENTITY_ALGORITHM.to_owned(),
                snapshot_complete,
                cargo_lock_observed,
                roots: snapshot.unwrap_or_default(),
            },
        )?;
    }

    let touch_path = state.join("touch.json");
    let mut touch = if touch_path.exists() {
        read_bounded_json::<IncrementalTouchState>(&touch_path)?
    } else {
        IncrementalTouchState {
            schema_version: 1,
            kind: "agenterm-incremental-touch-state".to_owned(),
            invocation_id: invocation.clone(),
            complete: true,
            rustc_invocations: 0,
            roots: BTreeSet::new(),
        }
    };
    anyhow::ensure!(
        touch.schema_version == 1
            && touch.kind == "agenterm-incremental-touch-state"
            && touch.invocation_id == invocation
            && touch.complete,
        "incremental touch state identity is invalid"
    );
    touch.rustc_invocations = touch
        .rustc_invocations
        .checked_add(1)
        .ok_or_else(|| anyhow::anyhow!("incremental rustc invocation count overflowed"))?;
    touch.roots.insert(touched_name);
    atomic_write_json(&touch_path, &touch)?;
    drop(barrier);
    Ok(())
}

fn poison_incremental_coordination(error: &anyhow::Error) -> bool {
    let Some(state) = std::env::var_os("AGENTERM_INCREMENTAL_STATE") else {
        return false;
    };
    let Ok(state) = exact_absolute(&state) else {
        return false;
    };
    if !direct_directory(&state) {
        return false;
    }
    if let Ok(mut poison) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(state.join("invalid"))
    {
        return writeln!(poison, "{}: {error:#}", std::process::id())
            .and_then(|()| poison.sync_all())
            .is_ok();
    }
    false
}

pub fn run_incremental_rustc_wrapper(arguments: Vec<OsString>) -> ! {
    let Some((compiler, rustc_arguments)) = arguments.split_first() else {
        eprintln!("incremental rustc wrapper received no compiler");
        std::process::exit(2);
    };
    let poison_persisted = if let Err(error) = coordinate_incremental_wrapper(rustc_arguments) {
        // A durable sticky poison lets compilation continue while finalization
        // fails closed. If even that cannot be persisted, a successful child
        // is converted to failure below rather than authorizing an unsafe
        // manifest. Never add diagnostics to the compiler byte streams.
        poison_incremental_coordination(&error)
    } else {
        true
    };
    match Command::new(compiler).args(rustc_arguments).status() {
        Ok(status) if poison_persisted => std::process::exit(status.code().unwrap_or(1)),
        Ok(status) if status.success() => std::process::exit(2),
        Ok(status) => std::process::exit(status.code().unwrap_or(1)),
        Err(error) => {
            eprintln!("failed to launch wrapped rustc: {error}");
            std::process::exit(2);
        }
    }
}

pub fn is_incremental_rustc_wrapper_process(arguments: &[OsString]) -> bool {
    if std::env::var("AGENTERM_INTERNAL_RUSTC_WRAPPER").as_deref() != Ok(INCREMENTAL_WRAPPER_MODE)
        || arguments
            .first()
            .is_some_and(|argument| argument == "--internal-incremental-finalize")
    {
        return false;
    }
    let Some(configured) = std::env::var_os("RUSTC_WRAPPER") else {
        return false;
    };
    let Ok(configured) = std::fs::canonicalize(configured) else {
        return false;
    };
    std::env::current_exe()
        .and_then(std::fs::canonicalize)
        .is_ok_and(|current| current == configured)
}

pub fn finalize_incremental_manifest(arguments: &[String]) -> anyhow::Result<u8> {
    anyhow::ensure!(
        arguments.len() == 4,
        "expected STATE TARGET MANIFEST INVOCATION"
    );
    anyhow::ensure!(
        std::env::var("AGENTERM_INTERNAL_RUSTC_WRAPPER").as_deref() == Ok(INCREMENTAL_WRAPPER_MODE),
        "incremental finalization is not internally authorized"
    );
    let state = exact_absolute(OsStr::new(&arguments[0]))?;
    let target = exact_absolute(OsStr::new(&arguments[1]))?;
    let manifest = exact_absolute(OsStr::new(&arguments[2]))?;
    let invocation = &arguments[3];
    anyhow::ensure!(
        safe_incremental_invocation_id(invocation),
        "invalid invocation identity"
    );
    let incremental = target.join("debug").join("incremental");
    anyhow::ensure!(
        state
            == target
                .join("debug")
                .join(".agenterm-incremental")
                .join(invocation)
            && manifest == state.join("manifest.json")
            && direct_directory(&state),
        "incremental finalization paths do not match"
    );

    let before = read_bounded_json::<IncrementalBeforeSnapshot>(&state.join("before.json"));
    let touch = read_bounded_json::<IncrementalTouchState>(&state.join("touch.json"));
    let mut snapshot_complete = false;
    let mut rustc_invocations = 0;
    let mut roots = Vec::new();
    if !incremental_poison_present(&state.join("invalid"))
        && let (Ok(before), Ok(touch)) = (before, touch)
    {
        let identities_match = before.schema_version == 1
            && before.kind == "agenterm-incremental-before-snapshot"
            && before.invocation_id == *invocation
            && before.target == target.to_string_lossy()
            && before.incremental_root == incremental.to_string_lossy()
            && before.identity_algorithm == INCREMENTAL_IDENTITY_ALGORITHM
            && before.snapshot_complete
            && before.cargo_lock_observed
            && touch.schema_version == 1
            && touch.kind == "agenterm-incremental-touch-state"
            && touch.invocation_id == *invocation
            && touch.complete
            && touch.rustc_invocations > 0;
        if identities_match {
            let mut after_complete = true;
            for root in before.roots {
                let path = incremental.join(&root.name);
                if !direct_directory(&path) {
                    continue;
                }
                match incremental_metadata_identity(&path) {
                    Ok(after_identity) => roots.push(IncrementalManifestRoot {
                        touched: touch.roots.contains(&root.name),
                        name: root.name,
                        before_identity: root.identity,
                        after_identity,
                    }),
                    Err(_) => {
                        after_complete = false;
                        break;
                    }
                }
            }
            snapshot_complete = after_complete;
            rustc_invocations = touch.rustc_invocations;
        }
    }
    if !snapshot_complete {
        roots.clear();
        rustc_invocations = 0;
    }
    roots.sort_by(|left, right| left.name.cmp(&right.name));
    atomic_write_json(
        &manifest,
        &IncrementalTouchManifest {
            kind: "agenterm-incremental-touch-manifest",
            schema_version: 1,
            invocation_id: invocation.clone(),
            target: target.to_string_lossy().into_owned(),
            incremental_root: incremental.to_string_lossy().into_owned(),
            snapshot_complete,
            rustc_invocations,
            identity_algorithm: INCREMENTAL_IDENTITY_ALGORITHM,
            roots,
        },
    )?;
    Ok(0)
}
