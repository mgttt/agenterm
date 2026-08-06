use std::{
    collections::{BTreeSet, HashSet},
    ffi::{OsStr, OsString},
    fs::OpenOptions,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::Command,
    process::ExitCode,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

use agenterm::script_protocol::{
    ReplRequestValidator, ReplSessionCommand, ReplSessionEvent, ReplSessionQuery,
    ReplSessionResponse, ReplSessionWireConfig, ReplWireCellResult, ReplWireFailure,
    ReplWireInputState, ReplWireQueryResult, ReplWireValue, ReplWireVariable, SCRIPT_API_VERSION,
    SCRIPT_ENVELOPE_VERSION, SCRIPT_FRAME_MAX_BYTES, SCRIPT_FRAME_VERSION,
    SCRIPT_INVOCATION_MAX_BYTES, ScriptBrokerRequest, ScriptBrokerResponse, ScriptBudgets,
    ScriptCancelDisposition, ScriptExitClass, ScriptFailure, ScriptFailureCategory, ScriptFrame,
    ScriptFrameEncodeError, ScriptFramePayload, ScriptFrameRead, ScriptFrameRejection,
    ScriptFrameTracker, ScriptInvocation, ScriptOperation, ScriptProfile, ScriptResult,
    encode_script_frame, read_script_frame, write_encoded_script_frame,
};
use agenterm::script_repl::{
    ReplCancelHandle, ReplCellFailure, ReplCellResult, ReplInputState, ReplSession,
    ReplSessionConfig,
};
use rhai::{Dynamic, Engine, EvalAltResult, Scope};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

type PendingBroker = Option<(String, mpsc::SyncSender<ScriptBrokerResponse>)>;
type SharedFrameOutput = Arc<Mutex<Box<dyn Write + Send>>>;

const CHECK_MANY_MANIFEST_MAX_BYTES: usize = 64 * 1024;
const CHECK_MANY_FILES_MAX: usize = 256;
const CHECK_MANY_PATH_MAX_BYTES: usize = 4 * 1024;
const CHECK_MANY_TOTAL_SOURCE_MAX_BYTES: usize = 8 * 1024 * 1024;
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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CheckManyManifest {
    schema_version: u32,
    kind: String,
    files: Vec<String>,
}

#[derive(Serialize)]
struct CheckManyFailure {
    path: String,
    code: String,
    message: String,
    invocation_id: String,
    exit_class: String,
}

#[derive(Serialize)]
struct CheckManyReport {
    schema_version: u32,
    kind: &'static str,
    ok: bool,
    checked_files: usize,
    total_source_bytes: usize,
    duration_ms: u64,
    failures: Vec<CheckManyFailure>,
}

#[derive(Clone)]
struct BrokerClient {
    invocation_id: String,
    output: SharedFrameOutput,
    pending: Arc<Mutex<PendingBroker>>,
    next_request: Arc<std::sync::atomic::AtomicUsize>,
    requests_remaining: Arc<std::sync::atomic::AtomicUsize>,
    timeout: Duration,
}

impl BrokerClient {
    fn call_json(
        &self,
        operation: &str,
        arguments: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        if self
            .requests_remaining
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
                value.checked_sub(1)
            })
            .is_err()
        {
            return Err("broker_request_budget_exceeded".to_owned());
        }
        let sequence = self.next_request.fetch_add(1, Ordering::Relaxed);
        let request_id = format!("broker-{sequence}");
        let (sender, receiver) = mpsc::sync_channel(1);
        {
            let mut pending = self.pending.lock().expect("pending broker lock poisoned");
            if pending.is_some() {
                return Err("broker_request_already_outstanding".to_owned());
            }
            *pending = Some((request_id.clone(), sender));
        }
        let frame = ScriptFrame {
            frame_version: SCRIPT_FRAME_VERSION,
            frame_id: format!("{}-{request_id}", self.invocation_id),
            payload: ScriptFramePayload::BrokerRequest {
                invocation_id: self.invocation_id.clone(),
                request_id: request_id.clone(),
                request: ScriptBrokerRequest {
                    operation: operation.to_owned(),
                    arguments,
                },
            },
        };
        if let Err(error) = write_shared_frame(&self.output, &frame) {
            self.pending
                .lock()
                .expect("pending broker lock poisoned")
                .take();
            return Err(format!("broker_request_send_failed: {error}"));
        }
        let response = receiver.recv_timeout(self.timeout).map_err(|_| {
            self.pending
                .lock()
                .expect("pending broker lock poisoned")
                .take();
            "broker_response_timeout".to_owned()
        })?;
        if let Some(error) = response.error {
            return Err(format!("{}: {}", error.code, error.message));
        }
        Ok(response.value.unwrap_or(serde_json::Value::Null))
    }
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
    agenterm::is_direct_file(path)
}

fn direct_directory(path: &Path) -> bool {
    agenterm::is_direct_directory(path)
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
        root_metadata.is_dir() && agenterm::is_direct_directory(root),
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
                agenterm::is_direct_directory(&path) || agenterm::is_direct_file(&path),
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
            agenterm::is_direct_directory(&entry.path()),
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
    if let Err(error) = agenterm::replace_file(&temporary, path) {
        let _ = std::fs::remove_file(&temporary);
        return Err(error.into());
    }
    agenterm::sync_parent(parent)?;
    Ok(())
}

fn read_bounded_json<T: for<'de> Deserialize<'de>>(path: &Path) -> anyhow::Result<T> {
    let metadata = std::fs::symlink_metadata(path)?;
    anyhow::ensure!(
        metadata.is_file() && agenterm::is_direct_file(path) && metadata.len() <= 4 * 1024 * 1024,
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

fn run_incremental_rustc_wrapper(arguments: Vec<OsString>) -> ! {
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

fn is_incremental_rustc_wrapper_process(arguments: &[OsString]) -> bool {
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

fn finalize_incremental_manifest(arguments: &[String]) -> anyhow::Result<u8> {
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

fn main() -> ExitCode {
    let os_arguments = std::env::args_os().skip(1).collect::<Vec<_>>();
    if is_incremental_rustc_wrapper_process(&os_arguments) {
        run_incremental_rustc_wrapper(os_arguments);
    }
    match run() {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("{error:#}");
            ExitCode::from(2)
        }
    }
}

fn run() -> anyhow::Result<u8> {
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    match arguments.as_slice() {
        [mode, rest @ ..] if mode == "--internal-incremental-finalize" => {
            finalize_incremental_manifest(rest)
        }
        [mode] if mode == "--worker" => run_worker(),
        [mode] if mode == "--framed-worker" => run_framed_worker(),
        [mode, rest @ ..] if mode == "check-many" => run_check_many(rest),
        [mode] if mode == "--version" || mode == "-V" => {
            println!("agenterm-rhai {}", env!("CARGO_PKG_VERSION"));
            Ok(0)
        }
        _ => u8::try_from(agenterm::run_script_entry_with_args(arguments))
            .map_err(|_| anyhow::anyhow!("script entry returned an invalid exit code")),
    }
}

fn run_check_many(arguments: &[String]) -> anyhow::Result<u8> {
    let started = Instant::now();
    let mut manifest_path = None::<String>;
    let mut project_root = None::<String>;
    let mut profile = ScriptProfile::Local;
    let mut budgets = ScriptBudgets::default();
    let mut json = false;
    let hard_limits = ScriptBudgets::hard_limits();
    let mut index = 0;
    while index < arguments.len() {
        let option = arguments[index].as_str();
        let value = |name: &str| -> anyhow::Result<&str> {
            arguments
                .get(index + 1)
                .map(String::as_str)
                .ok_or_else(|| anyhow::anyhow!("check-many option {name} requires a value"))
        };
        match option {
            "--manifest" => {
                manifest_path = Some(value(option)?.to_owned());
                index += 2;
            }
            "--project-root" => {
                project_root = Some(value(option)?.to_owned());
                index += 2;
            }
            "--profile" => {
                profile = match value(option)? {
                    "pure" | "observe" | "local" => ScriptProfile::Local,
                    other => anyhow::bail!("unknown script profile: {other}"),
                };
                index += 2;
            }
            "--timeout-ms" => {
                budgets.wall_time_ms =
                    parse_check_many_budget(option, value(option)?, hard_limits.wall_time_ms)?;
                index += 2;
            }
            "--max-operations" => {
                budgets.operations =
                    parse_check_many_budget(option, value(option)?, hard_limits.operations)?;
                index += 2;
            }
            "--max-collection-items" => {
                budgets.collection_items = usize::try_from(parse_check_many_budget(
                    option,
                    value(option)?,
                    hard_limits.collection_items as u64,
                )?)?;
                index += 2;
            }
            "--max-string-bytes" => {
                budgets.string_bytes = usize::try_from(parse_check_many_budget(
                    option,
                    value(option)?,
                    hard_limits.string_bytes as u64,
                )?)?;
                index += 2;
            }
            "--max-output-bytes" => {
                budgets.output_bytes = usize::try_from(parse_check_many_budget(
                    option,
                    value(option)?,
                    hard_limits.output_bytes as u64,
                )?)?;
                index += 2;
            }
            "--json" => {
                json = true;
                index += 1;
            }
            other => anyhow::bail!("unknown check-many option: {other}"),
        }
    }
    let manifest_path =
        manifest_path.ok_or_else(|| anyhow::anyhow!("check-many requires --manifest FILE"))?;
    let root = std::fs::canonicalize(project_root.as_deref().unwrap_or("."))
        .map_err(|error| anyhow::anyhow!("check_many_project_root: {error}"))?;
    if !root.is_dir() {
        anyhow::bail!("check_many_project_root: expected a directory");
    }
    let manifest = read_check_many_manifest(std::path::Path::new(&manifest_path))?;
    let mut failures = Vec::new();
    let mut checked_files = 0;
    let mut total_source_bytes = 0_usize;
    let mut seen = HashSet::new();
    for (ordinal, label) in manifest.files.into_iter().enumerate() {
        if started.elapsed() >= Duration::from_millis(budgets.wall_time_ms) {
            failures.push(check_many_host_failure(
                label,
                "limit_wall_time",
                "check-many reached its aggregate wall-time budget".to_owned(),
                ordinal,
                "limit",
            ));
            continue;
        }
        if label.is_empty() || label.len() > CHECK_MANY_PATH_MAX_BYTES {
            failures.push(check_many_host_failure(
                label,
                "check_many_path",
                format!("path must contain from 1 to {CHECK_MANY_PATH_MAX_BYTES} bytes"),
                ordinal,
                "configuration",
            ));
            continue;
        }
        let requested = std::path::Path::new(&label);
        let requested = if requested.is_absolute() {
            requested.to_path_buf()
        } else {
            root.join(requested)
        };
        let path = match std::fs::canonicalize(&requested) {
            Ok(path) if path.is_file() => path,
            Ok(_) => {
                failures.push(check_many_host_failure(
                    label,
                    "host_source_read",
                    "script source is not a file".to_owned(),
                    ordinal,
                    "host",
                ));
                continue;
            }
            Err(error) => {
                failures.push(check_many_host_failure(
                    label,
                    "host_source_resolve",
                    error.to_string(),
                    ordinal,
                    "host",
                ));
                continue;
            }
        };
        if !seen.insert(path.clone()) {
            failures.push(check_many_host_failure(
                label,
                "check_many_duplicate",
                "manifest resolves the same file more than once".to_owned(),
                ordinal,
                "configuration",
            ));
            continue;
        }
        let source = match read_check_many_source(&path, budgets.source_bytes) {
            Ok(source) => source,
            Err((code, message, exit_class)) => {
                failures.push(check_many_host_failure(
                    label, code, message, ordinal, exit_class,
                ));
                continue;
            }
        };
        let Some(next_total) = total_source_bytes.checked_add(source.len()) else {
            failures.push(check_many_host_failure(
                label,
                "check_many_total_source_bytes",
                "aggregate source byte count overflowed".to_owned(),
                ordinal,
                "limit",
            ));
            continue;
        };
        if next_total > CHECK_MANY_TOTAL_SOURCE_MAX_BYTES {
            failures.push(check_many_host_failure(
                label,
                "check_many_total_source_bytes",
                format!(
                    "aggregate source exceeds the {CHECK_MANY_TOTAL_SOURCE_MAX_BYTES} byte limit"
                ),
                ordinal,
                "limit",
            ));
            continue;
        }
        total_source_bytes = next_total;
        checked_files += 1;
        let invocation_id = format!("check-many-{}-{ordinal}", std::process::id());
        let result = execute(ScriptInvocation {
            envelope_version: SCRIPT_ENVELOPE_VERSION,
            invocation_id: invocation_id.clone(),
            api_version: SCRIPT_API_VERSION,
            operation: ScriptOperation::Check,
            profile,
            source_label: path.display().to_string(),
            source,
            project_root: Some(root.display().to_string()),
            invocation_temp_root: None,
            arguments: Vec::new(),
            budgets: budgets.clone(),
            observation: None,
        });
        if let Some(failure) = result.failure {
            failures.push(CheckManyFailure {
                path: label,
                code: failure.code,
                message: failure.message,
                invocation_id,
                exit_class: result.exit_class.as_str().to_owned(),
            });
        } else if !result.ok {
            failures.push(check_many_host_failure(
                label,
                "host_worker_protocol",
                "check returned an inconsistent result".to_owned(),
                ordinal,
                "host",
            ));
        }
    }
    let report = CheckManyReport {
        schema_version: 1,
        kind: "agenterm-rhai-check-many",
        ok: failures.is_empty(),
        checked_files,
        total_source_bytes,
        duration_ms: started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
        failures,
    };
    if json {
        serde_json::to_writer_pretty(std::io::stdout().lock(), &report)?;
        std::io::stdout().lock().write_all(b"\n")?;
    } else if report.ok {
        println!("OK ({} files)", report.checked_files);
    } else {
        let mut stderr = std::io::stderr().lock();
        for failure in &report.failures {
            writeln!(
                stderr,
                "{}: {}",
                failure.path,
                serde_json::json!({
                    "code": failure.code,
                    "message": failure.message,
                    "invocation_id": failure.invocation_id,
                    "exit_class": failure.exit_class,
                })
            )?;
        }
    }
    Ok(report
        .failures
        .first()
        .map_or(0, |failure| match failure.exit_class.as_str() {
            "configuration" => 2,
            "limit" => 3,
            "child" => 4,
            "cancelled" => 5,
            "fleet" => 6,
            _ => 1,
        }))
}

fn parse_check_many_budget(option: &str, value: &str, maximum: u64) -> anyhow::Result<u64> {
    let parsed = value
        .parse::<u64>()
        .map_err(|_| anyhow::anyhow!("check-many {option} must be numeric"))?;
    if !(1..=maximum).contains(&parsed) {
        anyhow::bail!("check-many {option} must be from 1 to {maximum}");
    }
    Ok(parsed)
}

fn read_check_many_manifest(path: &std::path::Path) -> anyhow::Result<CheckManyManifest> {
    let metadata = std::fs::metadata(path)
        .map_err(|error| anyhow::anyhow!("check_many_manifest_read: {error}"))?;
    if !metadata.is_file() || metadata.len() > CHECK_MANY_MANIFEST_MAX_BYTES as u64 {
        anyhow::bail!(
            "check_many_manifest_size: manifest must be a file of at most {CHECK_MANY_MANIFEST_MAX_BYTES} bytes"
        );
    }
    let file = std::fs::File::open(path)
        .map_err(|error| anyhow::anyhow!("check_many_manifest_read: {error}"))?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize + 1);
    file.take((CHECK_MANY_MANIFEST_MAX_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > CHECK_MANY_MANIFEST_MAX_BYTES {
        anyhow::bail!("check_many_manifest_size: manifest grew beyond its byte limit");
    }
    let manifest: CheckManyManifest = serde_json::from_slice(&bytes)
        .map_err(|error| anyhow::anyhow!("check_many_manifest_json: {error}"))?;
    if manifest.schema_version != 1 || manifest.kind != "agenterm-rhai-check-manifest" {
        anyhow::bail!("check_many_manifest_schema");
    }
    if manifest.files.is_empty() || manifest.files.len() > CHECK_MANY_FILES_MAX {
        anyhow::bail!("check_many_manifest_files: expected from 1 to {CHECK_MANY_FILES_MAX} files");
    }
    Ok(manifest)
}

fn read_check_many_source(
    path: &std::path::Path,
    maximum: usize,
) -> Result<String, (&'static str, String, &'static str)> {
    let file = std::fs::File::open(path)
        .map_err(|error| ("host_source_read", error.to_string(), "host"))?;
    let mut bytes = Vec::with_capacity(maximum.min(64 * 1024).saturating_add(1));
    file.take(maximum.saturating_add(1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| ("host_source_read", error.to_string(), "host"))?;
    if bytes.len() > maximum {
        return Err((
            "limit_source_bytes",
            format!("script source exceeds the {maximum} byte limit"),
            "limit",
        ));
    }
    let source = String::from_utf8(bytes).map_err(|error| {
        (
            "host_source_read",
            format!("script source is not UTF-8: {error}"),
            "host",
        )
    })?;
    Ok(normalize_script_source(source))
}

fn normalize_script_source(mut source: String) -> String {
    if source.starts_with("#!") {
        source.replace_range(..2, "//");
    }
    source
}

fn check_many_host_failure(
    path: String,
    code: &str,
    message: String,
    ordinal: usize,
    exit_class: &str,
) -> CheckManyFailure {
    CheckManyFailure {
        path,
        code: code.to_owned(),
        message,
        invocation_id: format!("check-many-{}-{ordinal}", std::process::id()),
        exit_class: exit_class.to_owned(),
    }
}

fn run_worker() -> anyhow::Result<u8> {
    let mut input = Vec::new();
    std::io::stdin()
        .take(SCRIPT_INVOCATION_MAX_BYTES + 1)
        .read_to_end(&mut input)?;
    let result = if input.len() as u64 > SCRIPT_INVOCATION_MAX_BYTES {
        protocol_failure(
            "protocol_invocation_too_large",
            format!("invocation exceeds the {SCRIPT_INVOCATION_MAX_BYTES} byte protocol limit"),
        )
    } else {
        match serde_json::from_slice(&input) {
            Ok(invocation) => execute(invocation),
            Err(error) => protocol_failure("protocol_invalid_invocation", error.to_string()),
        }
    };
    serde_json::to_writer(std::io::stdout().lock(), &result)?;
    std::io::stdout().lock().write_all(b"\n")?;
    Ok(u8::from(!result.ok))
}

fn run_framed_worker() -> anyhow::Result<u8> {
    let _interrupt_guard = agenterm::install_console_interrupt_ignore_guard()?;
    process_concurrent_framed_worker(std::io::stdin().lock(), std::io::stdout())?;
    Ok(0)
}

struct ReplResponseSink {
    output: SharedFrameOutput,
    next_sequence: Mutex<u64>,
}

impl ReplResponseSink {
    fn send(
        &self,
        session_id: &str,
        generation: u64,
        event: ReplSessionEvent,
    ) -> anyhow::Result<()> {
        let mut sequence = self
            .next_sequence
            .lock()
            .expect("REPL response sequence lock poisoned");
        let current = *sequence;
        let next = current
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("protocol_repl_sequence_overflow"))?;
        write_shared_frame(
            &self.output,
            &ScriptFrame {
                frame_version: SCRIPT_FRAME_VERSION,
                frame_id: format!("repl-response-{generation}-{current}"),
                payload: ScriptFramePayload::ReplResponse(ReplSessionResponse {
                    session_id: session_id.to_owned(),
                    generation,
                    sequence: current,
                    event,
                }),
            },
        )?;
        *sequence = next;
        Ok(())
    }

    fn failure(
        &self,
        session_id: &str,
        generation: u64,
        code: impl Into<String>,
        category: impl Into<String>,
        message: impl Into<String>,
    ) -> anyhow::Result<()> {
        self.send(
            session_id,
            generation,
            ReplSessionEvent::Failure {
                failure: ReplWireFailure {
                    code: code.into(),
                    category: category.into(),
                    message: message.into(),
                },
            },
        )
    }
}

#[derive(Clone)]
struct ReplBrokerClient {
    output: SharedFrameOutput,
    pending: Arc<Mutex<PendingBroker>>,
    active_cell: Arc<Mutex<Option<String>>>,
    next_request: Arc<std::sync::atomic::AtomicUsize>,
    requests_remaining: Arc<std::sync::atomic::AtomicUsize>,
    timeout: Duration,
}

impl ReplBrokerClient {
    fn call_json(
        &self,
        operation: &str,
        arguments: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        if self
            .requests_remaining
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
                value.checked_sub(1)
            })
            .is_err()
        {
            return Err("broker_request_budget_exceeded".to_owned());
        }
        let cell_id = self
            .active_cell
            .lock()
            .expect("REPL active cell lock poisoned")
            .clone()
            .ok_or_else(|| "broker_request_without_active_cell".to_owned())?;
        let sequence = self.next_request.fetch_add(1, Ordering::Relaxed);
        let request_id = format!("repl-broker-{sequence}");
        let (sender, receiver) = mpsc::sync_channel(1);
        {
            let mut pending = self.pending.lock().expect("pending broker lock poisoned");
            if pending.is_some() {
                return Err("broker_request_already_outstanding".to_owned());
            }
            *pending = Some((request_id.clone(), sender));
        }
        let frame = ScriptFrame {
            frame_version: SCRIPT_FRAME_VERSION,
            frame_id: format!("{cell_id}-{request_id}"),
            payload: ScriptFramePayload::BrokerRequest {
                invocation_id: cell_id,
                request_id: request_id.clone(),
                request: ScriptBrokerRequest {
                    operation: operation.to_owned(),
                    arguments,
                },
            },
        };
        if let Err(error) = write_shared_frame(&self.output, &frame) {
            self.pending
                .lock()
                .expect("pending broker lock poisoned")
                .take();
            return Err(format!("broker_request_send_failed: {error}"));
        }
        let response = receiver.recv_timeout(self.timeout).map_err(|_| {
            self.pending
                .lock()
                .expect("pending broker lock poisoned")
                .take();
            "broker_response_timeout".to_owned()
        })?;
        if let Some(error) = response.error {
            return Err(format!("{}: {}", error.code, error.message));
        }
        Ok(response.value.unwrap_or(serde_json::Value::Null))
    }
}

enum ReplSessionTask {
    Inspect(String),
    Evaluate {
        cell_id: String,
        source: String,
        baseline: u64,
    },
    Query(ReplSessionQuery),
    Reset,
    Close,
    Shutdown,
}

struct ReplWorkerSession {
    session_id: String,
    generation: u64,
    commands: mpsc::Sender<ReplSessionTask>,
    cancel: ReplCancelHandle,
    active_cell: Arc<Mutex<Option<String>>>,
    broker_requests_remaining: Arc<std::sync::atomic::AtomicUsize>,
    broker_request_limit: usize,
    join: Option<std::thread::JoinHandle<()>>,
}

impl ReplWorkerSession {
    fn shutdown(&mut self) {
        let _ = self.commands.send(ReplSessionTask::Shutdown);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

fn spawn_repl_session(
    session_id: String,
    generation: u64,
    config: ReplSessionWireConfig,
    responder: Arc<ReplResponseSink>,
    pending_broker: Arc<Mutex<PendingBroker>>,
    validator: Arc<Mutex<ReplRequestValidator>>,
) -> Result<ReplWorkerSession, String> {
    let active_cell = Arc::new(Mutex::new(None));
    let broker_requests_remaining = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let broker_request_limit = config.budgets.broker_requests;
    let (commands, command_rx) = mpsc::channel();
    let (started_tx, started_rx) = mpsc::sync_channel(1);
    let thread_session_id = session_id.clone();
    let thread_active_cell = Arc::clone(&active_cell);
    let thread_requests_remaining = Arc::clone(&broker_requests_remaining);
    let join = std::thread::spawn(move || {
        let broker = ReplBrokerClient {
            output: Arc::clone(&responder.output),
            pending: pending_broker,
            active_cell: Arc::clone(&thread_active_cell),
            next_request: Arc::new(std::sync::atomic::AtomicUsize::new(1)),
            requests_remaining: Arc::clone(&thread_requests_remaining),
            timeout: Duration::from_millis(config.budgets.wait_time_ms),
        };
        let fleet = Arc::new(move |operation: &str, arguments: serde_json::Value| {
            broker.call_json(operation, arguments)
        });
        let mut session = match ReplSession::new(ReplSessionConfig {
            budgets: config.budgets,
            arguments: config.arguments,
            project_root: config.project_root.map(PathBuf::from),
            invocation_temp_root: config.invocation_temp_root.map(PathBuf::from),
            fleet: Some(fleet),
        }) {
            Ok(session) => session,
            Err(error) => {
                let _ = started_tx.send(Err(error));
                return;
            }
        };
        if started_tx.send(Ok(session.cancel_handle())).is_err() {
            return;
        }
        if responder
            .send(
                &thread_session_id,
                generation,
                ReplSessionEvent::Ready {
                    worker_pid: std::process::id(),
                },
            )
            .is_err()
        {
            return;
        }
        while let Ok(task) = command_rx.recv() {
            match task {
                ReplSessionTask::Inspect(source) => {
                    let state = repl_input_state(session.inspect_input(&source));
                    if responder
                        .send(
                            &thread_session_id,
                            generation,
                            ReplSessionEvent::InputState { state },
                        )
                        .is_err()
                    {
                        break;
                    }
                }
                ReplSessionTask::Evaluate {
                    cell_id,
                    source,
                    baseline,
                } => {
                    if responder
                        .send(
                            &thread_session_id,
                            generation,
                            ReplSessionEvent::CellStarted {
                                cell_id: cell_id.clone(),
                            },
                        )
                        .is_err()
                    {
                        break;
                    }
                    let result =
                        repl_cell_result(session.evaluate_with_baseline(&source, baseline));
                    validator
                        .lock()
                        .expect("REPL validator lock poisoned")
                        .complete_evaluation(&thread_session_id, generation)
                        .expect("admitted REPL evaluation must complete in evaluating phase");
                    *thread_active_cell
                        .lock()
                        .expect("REPL active cell lock poisoned") = None;
                    if responder
                        .send(
                            &thread_session_id,
                            generation,
                            ReplSessionEvent::CellResult { cell_id, result },
                        )
                        .is_err()
                    {
                        break;
                    }
                }
                ReplSessionTask::Query(query) => {
                    let result = repl_query_result(&session, query);
                    if responder
                        .send(
                            &thread_session_id,
                            generation,
                            ReplSessionEvent::QueryResult { query, result },
                        )
                        .is_err()
                    {
                        break;
                    }
                }
                ReplSessionTask::Reset => {
                    session.reset();
                    if responder
                        .send(&thread_session_id, generation, ReplSessionEvent::ResetDone)
                        .is_err()
                    {
                        break;
                    }
                }
                ReplSessionTask::Close => {
                    let _ =
                        responder.send(&thread_session_id, generation, ReplSessionEvent::Closed);
                    break;
                }
                ReplSessionTask::Shutdown => break,
            }
        }
    });
    match started_rx.recv() {
        Ok(Ok(cancel)) => Ok(ReplWorkerSession {
            session_id,
            generation,
            commands,
            cancel,
            active_cell,
            broker_requests_remaining,
            broker_request_limit,
            join: Some(join),
        }),
        Ok(Err(error)) => {
            let _ = join.join();
            Err(error)
        }
        Err(_) => {
            let _ = join.join();
            Err("host_repl_session_start: session thread stopped before startup".to_owned())
        }
    }
}

fn repl_input_state(state: ReplInputState) -> ReplWireInputState {
    match state {
        ReplInputState::Complete => ReplWireInputState::Complete,
        ReplInputState::Incomplete => ReplWireInputState::Incomplete,
        ReplInputState::Invalid(failure) => ReplWireInputState::Invalid(repl_failure(failure)),
    }
}

fn repl_failure(failure: ReplCellFailure) -> ReplWireFailure {
    ReplWireFailure {
        code: failure.code,
        category: failure.category,
        message: failure.message,
    }
}

fn repl_cell_result(result: ReplCellResult) -> ReplWireCellResult {
    ReplWireCellResult {
        cell_sequence: result.sequence,
        ok: result.ok,
        stdout: result.stdout,
        value: result.value.map(|value| ReplWireValue {
            type_name: value.type_name,
            serializable: value.serializable,
            value: value.value,
        }),
        failure: result.failure.map(repl_failure),
        state_committed: result.state_committed,
        duration_ms: result.duration_ms,
    }
}

fn repl_query_result(session: &ReplSession, query: ReplSessionQuery) -> ReplWireQueryResult {
    let variables = || {
        session
            .variables()
            .into_iter()
            .map(|(name, type_name)| ReplWireVariable { name, type_name })
            .collect()
    };
    match query {
        ReplSessionQuery::State => ReplWireQueryResult::State {
            history: session.history().to_vec(),
            variables: variables(),
            functions: session.functions(),
            limits: session.budgets().clone(),
        },
        ReplSessionQuery::History => ReplWireQueryResult::History(session.history().to_vec()),
        ReplSessionQuery::Variables => ReplWireQueryResult::Variables(variables()),
        ReplSessionQuery::Functions => ReplWireQueryResult::Functions(session.functions()),
        ReplSessionQuery::Limits => ReplWireQueryResult::Limits(session.budgets().clone()),
    }
}

fn process_concurrent_framed_worker<R: Read>(
    mut input: R,
    output: impl Write + Send + 'static,
) -> anyhow::Result<()> {
    let output: SharedFrameOutput = Arc::new(Mutex::new(Box::new(output)));
    let active = Arc::new(Mutex::new(None::<(String, Arc<AtomicBool>)>));
    let pending_broker = Arc::new(Mutex::new(
        None::<(String, mpsc::SyncSender<ScriptBrokerResponse>)>,
    ));
    let completed = Arc::new(Mutex::new(HashSet::<String>::new()));
    let mut frame_tracker = ScriptFrameTracker::default();
    let repl_validator = Arc::new(Mutex::new(ReplRequestValidator::default()));
    let repl_responder = Arc::new(ReplResponseSink {
        output: Arc::clone(&output),
        next_sequence: Mutex::new(0),
    });
    let mut repl_session = None::<ReplWorkerSession>;
    let mut repl_started = false;
    let mut workers = Vec::new();
    loop {
        let frame = match read_script_frame(&mut input)? {
            ScriptFrameRead::Eof => break,
            ScriptFrameRead::Frame(frame) => *frame,
            ScriptFrameRead::Rejected(rejection) => {
                let recoverable = rejection.recoverable;
                write_shared_rejection(&output, rejection)?;
                if !recoverable {
                    break;
                }
                continue;
            }
        };
        if let ScriptFramePayload::ReplRequest(request) = &frame.payload {
            if !repl_started
                && matches!(&request.command, ReplSessionCommand::Open { .. })
                && active
                    .lock()
                    .expect("active invocation lock poisoned")
                    .is_some()
            {
                repl_responder.failure(
                    &request.session_id,
                    request.generation,
                    "protocol_worker_busy",
                    "protocol",
                    "legacy invocation is active; REPL session cannot open",
                )?;
                break;
            }
            let admission = repl_validator
                .lock()
                .expect("REPL validator lock poisoned")
                .admit_frame(&frame);
            if let Err(rejection) = admission {
                if let Some(session) = repl_session.as_ref() {
                    repl_responder.failure(
                        &session.session_id,
                        session.generation,
                        rejection.code,
                        "protocol",
                        rejection.message,
                    )?;
                } else {
                    write_shared_protocol_frame(
                        &output,
                        &frame.frame_id,
                        "unknown",
                        rejection.code,
                        rejection.message,
                    )?;
                    break;
                }
                continue;
            }
            let request = match frame.payload {
                ScriptFramePayload::ReplRequest(request) => request,
                _ => unreachable!("borrowed REPL request changed payload kind"),
            };
            let mut close_worker = false;
            match request.command {
                ReplSessionCommand::Open { config } => {
                    repl_started = true;
                    match spawn_repl_session(
                        request.session_id.clone(),
                        request.generation,
                        config,
                        Arc::clone(&repl_responder),
                        Arc::clone(&pending_broker),
                        Arc::clone(&repl_validator),
                    ) {
                        Ok(session) => repl_session = Some(session),
                        Err(error) => {
                            repl_responder.failure(
                                &request.session_id,
                                request.generation,
                                "host_repl_session_start",
                                "host",
                                error,
                            )?;
                            close_worker = true;
                        }
                    }
                }
                ReplSessionCommand::Evaluate { cell_id, source } => {
                    if let Some(session) = repl_session.as_ref() {
                        let baseline = session.cancel.capture_epoch();
                        *session
                            .active_cell
                            .lock()
                            .expect("REPL active cell lock poisoned") = Some(cell_id.clone());
                        session
                            .broker_requests_remaining
                            .store(session.broker_request_limit, Ordering::Release);
                        if session
                            .commands
                            .send(ReplSessionTask::Evaluate {
                                cell_id,
                                source,
                                baseline,
                            })
                            .is_err()
                        {
                            *session
                                .active_cell
                                .lock()
                                .expect("REPL active cell lock poisoned") = None;
                            repl_responder.failure(
                                &request.session_id,
                                request.generation,
                                "host_repl_session_unavailable",
                                "host",
                                "REPL session thread is unavailable",
                            )?;
                            close_worker = true;
                        }
                    } else {
                        repl_responder.failure(
                            &request.session_id,
                            request.generation,
                            "host_repl_session_unavailable",
                            "host",
                            "REPL session is unavailable",
                        )?;
                        close_worker = true;
                    }
                }
                ReplSessionCommand::Inspect { source } => {
                    if repl_session.as_ref().is_none_or(|session| {
                        session
                            .commands
                            .send(ReplSessionTask::Inspect(source))
                            .is_err()
                    }) {
                        repl_responder.failure(
                            &request.session_id,
                            request.generation,
                            "host_repl_session_unavailable",
                            "host",
                            "REPL session is unavailable",
                        )?;
                        close_worker = true;
                    }
                }
                ReplSessionCommand::Query { query } => {
                    if repl_session.as_ref().is_none_or(|session| {
                        session
                            .commands
                            .send(ReplSessionTask::Query(query))
                            .is_err()
                    }) {
                        repl_responder.failure(
                            &request.session_id,
                            request.generation,
                            "host_repl_session_unavailable",
                            "host",
                            "REPL session is unavailable",
                        )?;
                        close_worker = true;
                    }
                }
                ReplSessionCommand::Reset => {
                    if repl_session.as_ref().is_none_or(|session| {
                        session.commands.send(ReplSessionTask::Reset).is_err()
                    }) {
                        repl_responder.failure(
                            &request.session_id,
                            request.generation,
                            "host_repl_session_unavailable",
                            "host",
                            "REPL session is unavailable",
                        )?;
                        close_worker = true;
                    }
                }
                ReplSessionCommand::Cancel { .. } => {
                    if let Some(session) = repl_session.as_ref() {
                        session.cancel.cancel();
                    } else {
                        repl_responder.failure(
                            &request.session_id,
                            request.generation,
                            "host_repl_session_unavailable",
                            "host",
                            "REPL session is unavailable",
                        )?;
                        close_worker = true;
                    }
                }
                ReplSessionCommand::Close => {
                    if repl_session.as_ref().is_none_or(|session| {
                        session.commands.send(ReplSessionTask::Close).is_err()
                    }) {
                        repl_responder.failure(
                            &request.session_id,
                            request.generation,
                            "host_repl_session_unavailable",
                            "host",
                            "REPL session is unavailable",
                        )?;
                    }
                    close_worker = true;
                }
            }
            if close_worker {
                break;
            }
            continue;
        }
        if matches!(&frame.payload, ScriptFramePayload::ReplResponse(_)) {
            if let Some(session) = repl_session.as_ref() {
                repl_responder.failure(
                    &session.session_id,
                    session.generation,
                    "protocol_repl_unexpected_response",
                    "protocol",
                    "REPL response frames are worker output and cannot be sent to the worker",
                )?;
            } else {
                write_shared_protocol_frame(
                    &output,
                    &frame.frame_id,
                    "unknown",
                    "protocol_repl_unexpected_response",
                    "REPL response frames are worker output and cannot be sent to the worker",
                )?;
            }
            break;
        }
        if repl_started && matches!(&frame.payload, ScriptFramePayload::BrokerResponse { .. }) {
            let session = repl_session
                .as_ref()
                .expect("started REPL with broker traffic has a live session");
            if frame.frame_version != SCRIPT_FRAME_VERSION {
                repl_responder.failure(
                    &session.session_id,
                    session.generation,
                    "protocol_unsupported_frame_version",
                    "protocol",
                    format!(
                        "broker frame version {} does not match {SCRIPT_FRAME_VERSION}",
                        frame.frame_version
                    ),
                )?;
                continue;
            }
            if frame.frame_id.is_empty() || frame.frame_id.len() > 128 {
                repl_responder.failure(
                    &session.session_id,
                    session.generation,
                    "protocol_invalid_frame_id",
                    "protocol",
                    "frame_id must contain from 1 to 128 bytes",
                )?;
                continue;
            }
            let ScriptFramePayload::BrokerResponse {
                invocation_id,
                request_id,
                response,
            } = frame.payload
            else {
                unreachable!("matched REPL broker response changed payload kind")
            };
            let active_matches = session
                .active_cell
                .lock()
                .expect("REPL active cell lock poisoned")
                .as_ref()
                == Some(&invocation_id);
            let pending = pending_broker
                .lock()
                .expect("pending broker lock poisoned")
                .take();
            match pending {
                Some((expected, sender)) if active_matches && expected == request_id => {
                    let _ = sender.send(response);
                }
                Some(pending) => {
                    *pending_broker.lock().expect("pending broker lock poisoned") = Some(pending);
                    repl_responder.failure(
                        &session.session_id,
                        session.generation,
                        "protocol_broker_response_mismatch",
                        "protocol",
                        "broker response does not match the active REPL request",
                    )?;
                }
                None => repl_responder.failure(
                    &session.session_id,
                    session.generation,
                    "protocol_broker_response_unexpected",
                    "protocol",
                    "no REPL broker request is outstanding",
                )?,
            }
            continue;
        }
        if repl_started {
            let session = repl_session
                .as_ref()
                .expect("started REPL has a live session");
            let (code, message) = match &frame.payload {
                ScriptFramePayload::Invoke(_) | ScriptFramePayload::Cancel { .. } => (
                    "protocol_repl_session_active",
                    "legacy invocation traffic cannot run after a REPL session has started",
                ),
                ScriptFramePayload::BrokerRequest { .. } => (
                    "protocol_unexpected_broker_request",
                    "broker request frames are worker output and cannot be sent to the worker",
                ),
                ScriptFramePayload::Result(_) => (
                    "protocol_unexpected_result",
                    "result frames are worker output and cannot be sent to the worker",
                ),
                ScriptFramePayload::BrokerResponse { .. }
                | ScriptFramePayload::ReplRequest(_)
                | ScriptFramePayload::ReplResponse(_) => {
                    unreachable!("session traffic was routed before legacy rejection")
                }
            };
            repl_responder.failure(
                &session.session_id,
                session.generation,
                code,
                "protocol",
                message,
            )?;
            continue;
        }
        let frame = match frame_tracker.admit(frame) {
            Ok(frame) => frame,
            Err(rejection) => {
                write_shared_rejection(&output, rejection)?;
                continue;
            }
        };
        let frame_id = frame.frame_id;
        match frame.payload {
            ScriptFramePayload::Invoke(invocation) => {
                let invocation_id = invocation.invocation_id.clone();
                let cancellation = Arc::new(AtomicBool::new(false));
                {
                    let mut active_guard = active.lock().expect("active invocation lock poisoned");
                    if active_guard.is_some() {
                        write_shared_protocol_frame(
                            &output,
                            &frame_id,
                            &invocation_id,
                            "protocol_worker_busy",
                            "this worker already has an active invocation",
                        )?;
                        continue;
                    }
                    *active_guard = Some((invocation_id.clone(), Arc::clone(&cancellation)));
                }
                let output_for_worker = Arc::clone(&output);
                let active_for_worker = Arc::clone(&active);
                let completed_for_worker = Arc::clone(&completed);
                let pending_for_worker = Arc::clone(&pending_broker);
                let broker = if matches!(
                    invocation.operation,
                    ScriptOperation::Eval | ScriptOperation::Run
                ) {
                    Some(BrokerClient {
                        invocation_id: invocation_id.clone(),
                        output: Arc::clone(&output),
                        pending: pending_for_worker,
                        next_request: Arc::new(std::sync::atomic::AtomicUsize::new(1)),
                        requests_remaining: Arc::new(std::sync::atomic::AtomicUsize::new(
                            invocation.budgets.broker_requests,
                        )),
                        timeout: Duration::from_millis(invocation.budgets.wait_time_ms),
                    })
                } else {
                    None
                };
                workers.push(std::thread::spawn(move || {
                    let result = execute_with_cancellation_and_broker(
                        invocation,
                        Some(cancellation),
                        broker,
                    );
                    let mut active_guard = active_for_worker
                        .lock()
                        .expect("active invocation lock poisoned");
                    let _ = write_shared_frame(&output_for_worker, &result_frame(frame_id, result));
                    completed_for_worker
                        .lock()
                        .expect("completed invocation lock poisoned")
                        .insert(invocation_id);
                    *active_guard = None;
                }));
            }
            ScriptFramePayload::Cancel { invocation_id } => {
                let active_invocation = active
                    .lock()
                    .expect("active invocation lock poisoned")
                    .as_ref()
                    .map(|(active_id, cancellation)| (active_id.clone(), Arc::clone(cancellation)));
                let disposition = ScriptCancelDisposition::classify(
                    &invocation_id,
                    active_invocation
                        .as_ref()
                        .map(|(active_id, _)| active_id.as_str()),
                    completed
                        .lock()
                        .expect("completed invocation lock poisoned")
                        .contains(&invocation_id),
                );
                match disposition {
                    ScriptCancelDisposition::Requested => active_invocation
                        .expect("requested cancellation has an active invocation")
                        .1
                        .store(true, Ordering::Relaxed),
                    ScriptCancelDisposition::TooLate | ScriptCancelDisposition::Unknown => {
                        let (code, message) = disposition
                            .rejection()
                            .expect("non-requested cancellation has a rejection");
                        write_shared_protocol_frame(
                            &output,
                            &frame_id,
                            &invocation_id,
                            code,
                            message,
                        )?;
                    }
                }
            }
            ScriptFramePayload::BrokerResponse {
                invocation_id,
                request_id,
                response,
            } => {
                let legacy_active_matches = active
                    .lock()
                    .expect("active invocation lock poisoned")
                    .as_ref()
                    .is_some_and(|(active_id, _)| active_id == &invocation_id);
                let pending = pending_broker
                    .lock()
                    .expect("pending broker lock poisoned")
                    .take();
                match pending {
                    Some((expected, sender)) if legacy_active_matches && expected == request_id => {
                        let _ = sender.send(response);
                    }
                    Some(pending) => {
                        *pending_broker.lock().expect("pending broker lock poisoned") =
                            Some(pending);
                        write_shared_protocol_frame(
                            &output,
                            &frame_id,
                            &invocation_id,
                            "protocol_broker_response_mismatch",
                            "broker response does not match the active request",
                        )?;
                    }
                    None => write_shared_protocol_frame(
                        &output,
                        &frame_id,
                        &invocation_id,
                        "protocol_broker_response_unexpected",
                        "no broker request is outstanding",
                    )?,
                }
            }
            ScriptFramePayload::BrokerRequest { invocation_id, .. } => {
                write_shared_protocol_frame(
                    &output,
                    &frame_id,
                    &invocation_id,
                    "protocol_unexpected_broker_request",
                    "broker request frames are worker output and cannot be sent to the worker",
                )?;
            }
            ScriptFramePayload::Result(result) => {
                write_shared_protocol_frame(
                    &output,
                    &frame_id,
                    &result.invocation_id,
                    "protocol_unexpected_result",
                    "result frames are worker output and cannot be sent to the worker",
                )?;
            }
            ScriptFramePayload::ReplRequest(_) | ScriptFramePayload::ReplResponse(_) => {
                unreachable!("REPL frames are routed before the legacy tracker")
            }
        }
    }
    if let Some(session) = repl_session.as_mut() {
        session.shutdown();
    }
    for worker in workers {
        let _ = worker.join();
    }
    Ok(())
}

fn write_shared_frame(output: &SharedFrameOutput, frame: &ScriptFrame) -> anyhow::Result<()> {
    let mut output = output.lock().expect("framed stdout lock poisoned");
    write_frame(&mut *output, frame)
}

fn write_shared_protocol_frame(
    output: &SharedFrameOutput,
    frame_id: &str,
    invocation_id: &str,
    code: &str,
    message: impl Into<String>,
) -> anyhow::Result<()> {
    write_shared_frame(
        output,
        &result_frame(
            frame_id.to_owned(),
            protocol_failure_for(invocation_id, code, message),
        ),
    )
}

fn write_shared_rejection(
    output: &SharedFrameOutput,
    rejection: ScriptFrameRejection,
) -> anyhow::Result<()> {
    write_shared_protocol_frame(
        output,
        &rejection.frame_id,
        &rejection.invocation_id,
        rejection.code,
        rejection.message,
    )
}

#[cfg(test)]
fn process_framed_stream<R: Read, W: Write>(mut input: R, mut output: W) -> anyhow::Result<()> {
    let mut frame_tracker = ScriptFrameTracker::default();
    let mut completed_invocations = HashSet::new();
    loop {
        let frame = match read_script_frame(&mut input)? {
            ScriptFrameRead::Eof => return Ok(()),
            ScriptFrameRead::Frame(frame) => *frame,
            ScriptFrameRead::Rejected(rejection) => {
                let recoverable = rejection.recoverable;
                write_protocol_frame(
                    &mut output,
                    &rejection.frame_id,
                    &rejection.invocation_id,
                    rejection.code,
                    rejection.message,
                )?;
                if !recoverable {
                    return Ok(());
                }
                continue;
            }
        };
        let frame = match frame_tracker.admit(frame) {
            Ok(frame) => frame,
            Err(rejection) => {
                write_protocol_frame(
                    &mut output,
                    &rejection.frame_id,
                    &rejection.invocation_id,
                    rejection.code,
                    rejection.message,
                )?;
                continue;
            }
        };
        let response = process_frame(frame, &mut completed_invocations);
        write_frame(&mut output, &response)?;
    }
}

#[cfg(test)]
fn process_frame(frame: ScriptFrame, completed_invocations: &mut HashSet<String>) -> ScriptFrame {
    let frame_id = frame.frame_id;
    let result = match frame.payload {
        ScriptFramePayload::Invoke(invocation) => {
            completed_invocations.insert(invocation.invocation_id.clone());
            execute(invocation)
        }
        ScriptFramePayload::Cancel { invocation_id } => {
            let disposition = ScriptCancelDisposition::classify(
                &invocation_id,
                None,
                completed_invocations.contains(&invocation_id),
            );
            let (code, message) = disposition
                .rejection()
                .expect("synchronous harness has no active invocation");
            protocol_failure_for(&invocation_id, code, message)
        }
        ScriptFramePayload::BrokerRequest { invocation_id, .. }
        | ScriptFramePayload::BrokerResponse { invocation_id, .. } => protocol_failure_for(
            &invocation_id,
            "protocol_broker_unavailable",
            "broker frames are reserved but unavailable in this worker version",
        ),
        ScriptFramePayload::Result(result) => protocol_failure_for(
            &result.invocation_id,
            "protocol_unexpected_result",
            "result frames are worker output and cannot be sent to the worker",
        ),
        ScriptFramePayload::ReplRequest(request) => protocol_failure_for(
            &request.session_id,
            "protocol_repl_requires_concurrent_worker",
            "REPL requests require the concurrent framed worker",
        ),
        ScriptFramePayload::ReplResponse(response) => protocol_failure_for(
            &response.session_id,
            "protocol_repl_unexpected_response",
            "REPL response frames are worker output and cannot be sent to the worker",
        ),
    };
    result_frame(frame_id, result)
}

fn result_frame(frame_id: String, result: ScriptResult) -> ScriptFrame {
    ScriptFrame {
        frame_version: SCRIPT_FRAME_VERSION,
        frame_id,
        payload: ScriptFramePayload::Result(result),
    }
}

#[cfg(test)]
fn write_protocol_frame<W: Write>(
    output: &mut W,
    frame_id: &str,
    invocation_id: &str,
    code: &str,
    message: impl Into<String>,
) -> anyhow::Result<()> {
    write_frame(
        output,
        &result_frame(
            frame_id.to_owned(),
            protocol_failure_for(invocation_id, code, message),
        ),
    )
}

fn write_frame<W: Write>(output: &mut W, frame: &ScriptFrame) -> anyhow::Result<()> {
    let bytes = match encode_script_frame(frame) {
        Ok(bytes) => bytes,
        Err(ScriptFrameEncodeError::TooLarge { .. }) => {
            let invocation_id = match &frame.payload {
                ScriptFramePayload::Result(result) => result.invocation_id.as_str(),
                _ => "unknown",
            };
            let replacement = result_frame(
                frame.frame_id.clone(),
                protocol_failure_for(
                    invocation_id,
                    "protocol_result_frame_too_large",
                    format!("encoded result exceeds the {SCRIPT_FRAME_MAX_BYTES} byte frame limit"),
                ),
            );
            encode_script_frame(&replacement)?
        }
        Err(error) => return Err(error.into()),
    };
    write_encoded_script_frame(output, &bytes)?;
    Ok(())
}

fn execute(invocation: ScriptInvocation) -> ScriptResult {
    execute_with_cancellation(invocation, None)
}

fn execute_with_cancellation(
    invocation: ScriptInvocation,
    cancellation: Option<Arc<AtomicBool>>,
) -> ScriptResult {
    execute_with_cancellation_and_broker(invocation, cancellation, None)
}

fn execute_with_cancellation_and_broker(
    invocation: ScriptInvocation,
    cancellation: Option<Arc<AtomicBool>>,
    broker: Option<BrokerClient>,
) -> ScriptResult {
    let started = Instant::now();
    let mut result = ScriptResult {
        envelope_version: SCRIPT_ENVELOPE_VERSION,
        invocation_id: invocation.invocation_id.clone(),
        api_version: SCRIPT_API_VERSION,
        ok: false,
        exit_class: ScriptExitClass::Configuration,
        operation: Some(invocation.operation),
        profile: Some(invocation.profile),
        stdout: String::new(),
        value: None,
        failure: None,
        duration_ms: 0,
    };
    let execution = execute_inner(&invocation, cancellation, broker);
    match execution {
        Ok((stdout, value)) => {
            result.ok = true;
            result.exit_class = ScriptExitClass::Success;
            result.stdout = stdout;
            result.value = value;
        }
        Err(failure) => {
            result.exit_class = match failure.category {
                ScriptFailureCategory::Configuration => ScriptExitClass::Configuration,
                ScriptFailureCategory::Limit => ScriptExitClass::Limit,
                ScriptFailureCategory::Script => ScriptExitClass::Script,
                ScriptFailureCategory::Child => ScriptExitClass::Child,
                ScriptFailureCategory::Cancelled => ScriptExitClass::Cancelled,
                ScriptFailureCategory::Fleet => ScriptExitClass::Fleet,
                ScriptFailureCategory::Protocol => ScriptExitClass::Protocol,
                ScriptFailureCategory::Host => ScriptExitClass::Host,
            };
            result.failure = Some(failure);
        }
    }
    result.duration_ms = started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
    result
}

fn execute_inner(
    invocation: &ScriptInvocation,
    cancellation: Option<Arc<AtomicBool>>,
    broker: Option<BrokerClient>,
) -> Result<(String, Option<serde_json::Value>), ScriptFailure> {
    let _temp_scope = agenterm::script_stdlib::enter_invocation_temp_root(
        invocation
            .invocation_temp_root
            .as_deref()
            .map(std::path::Path::new),
    );
    if invocation.envelope_version != SCRIPT_ENVELOPE_VERSION {
        return Err(protocol_error(
            "unsupported_envelope",
            format!(
                "worker supports envelope {}, requested {}",
                SCRIPT_ENVELOPE_VERSION, invocation.envelope_version
            ),
        ));
    }
    if invocation.api_version != SCRIPT_API_VERSION {
        return Err(protocol_error(
            "unsupported_api",
            format!(
                "worker supports API {}, requested {}",
                SCRIPT_API_VERSION, invocation.api_version
            ),
        ));
    }
    validate_budgets(&invocation.budgets)?;
    if invocation.source.len() > invocation.budgets.source_bytes {
        return Err(limit_error(
            "limit_source_bytes",
            "script source exceeds its byte budget",
        ));
    }
    if invocation.operation == ScriptOperation::Api {
        return Ok((String::new(), Some(agenterm::script_catalog::catalog())));
    }

    if let Some(result) = agenterm::script_backend::try_execute_rh_invocation(
        invocation.operation,
        &invocation.source,
        broker.as_ref().map(|broker| {
            let broker = broker.clone();
            agenterm::script_rh_host::broker_fleet_bridge(move |operation, arguments| {
                broker.call_json(operation, arguments)
            })
        }),
    )
    .map_err(|error| configuration_error("rh_backend", error.to_string()))?
    {
        return Ok((result.stdout, result.value));
    }

    let output = Arc::new(Mutex::new(String::new()));
    let output_exceeded = Arc::new(AtomicBool::new(false));
    let wall_time_exceeded = Arc::new(AtomicBool::new(false));
    let cancellation = cancellation.unwrap_or_else(|| Arc::new(AtomicBool::new(false)));
    let output_for_print = Arc::clone(&output);
    let exceeded_for_print = Arc::clone(&output_exceeded);
    let output_limit = invocation.budgets.output_bytes;
    let deadline = Instant::now()
        .checked_add(std::time::Duration::from_millis(
            invocation.budgets.wall_time_ms,
        ))
        .unwrap_or_else(Instant::now);
    let wall_time_for_progress = Arc::clone(&wall_time_exceeded);
    let cancellation_for_progress = Arc::clone(&cancellation);
    let mut engine = Engine::new();
    agenterm::script_runtime::configure_engine(&mut engine, &invocation.budgets);
    engine.on_print(move |text| {
        let mut output = output_for_print
            .lock()
            .expect("script output lock poisoned");
        let remaining = output_limit.saturating_sub(output.len());
        if text.len().saturating_add(1) > remaining {
            exceeded_for_print.store(true, Ordering::Relaxed);
        }
        let mut take = text.len().min(remaining);
        while take > 0 && !text.is_char_boundary(take) {
            take -= 1;
        }
        output.push_str(&text[..take]);
        if output.len() < output_limit {
            output.push('\n');
        }
    });
    engine.on_progress(move |_| {
        if cancellation_for_progress.load(Ordering::Relaxed) {
            Some(Dynamic::from("script cancellation requested"))
        } else if Instant::now() >= deadline {
            wall_time_for_progress.store(true, Ordering::Relaxed);
            Some(Dynamic::from("wall-time budget exceeded"))
        } else {
            None
        }
    });
    if invocation.operation == ScriptOperation::Check {
        engine
            .compile(&invocation.source)
            .map_err(|error| classify_compile_error(error.to_string()))?;
        if let Some(project_root) = invocation.project_root.as_deref() {
            let module_sources = agenterm::script_project::validate_project_imports(
                &engine,
                std::path::Path::new(project_root),
                &invocation.source,
            )
            .map_err(classify_compile_error)?;
            for module_source in module_sources {
                validate_available_apis(&module_source, &invocation.budgets)?;
            }
        }
        validate_available_apis(&invocation.source, &invocation.budgets)?;
        return Ok((String::new(), None));
    }

    if let Some(project_root) = invocation.project_root.as_deref() {
        let resolver = agenterm::script_project::ProjectModuleResolver::new(std::path::Path::new(
            project_root,
        ))
        .map_err(|error| configuration_error("script_project_root", error))?;
        engine.set_module_resolver(resolver);
    }
    let ast = engine
        .compile_into_self_contained(&Scope::new(), &invocation.source)
        .map_err(|error| classify_compile_error(error.to_string()))?;

    let mut scope = Scope::new();
    let arguments = rhai::serde::to_dynamic(&invocation.arguments)
        .map_err(|error| configuration_error("configuration_arguments", error.to_string()))?;
    scope.push_dynamic("args", arguments);
    if let Some(broker) = broker {
        let call = Arc::new(move |operation: &str, arguments: serde_json::Value| {
            broker.call_json(operation, arguments)
        });
        scope.push("fleet", agenterm::script_fleet::bind(call));
    }
    let value = engine
        .eval_ast_with_scope::<Dynamic>(&mut scope, &ast)
        .map_err(|error| {
            let message = error.to_string();
            if cancellation.load(Ordering::Relaxed) {
                cancelled_error("limit_cancelled", message)
            } else if wall_time_exceeded.load(Ordering::Relaxed) {
                limit_error("limit_wall_time", message)
            } else if message.contains("Too many operations") {
                limit_error("limit_operations", message)
            } else if message.contains("exceeds the maximum")
                || message.contains("Maximum call stack depth")
                || message.contains("Stack overflow")
            {
                classify_engine_limit(message)
            } else if output_exceeded.load(Ordering::Relaxed) {
                limit_error("limit_output_bytes", message)
            } else {
                classify_runtime_error(&error, message)
            }
        })?;
    let value = if value.is_unit() {
        None
    } else {
        Some(
            rhai::serde::from_dynamic(&value)
                .map_err(|error| script_error("script_result_type", error.to_string()))?,
        )
    };
    let stdout = output
        .lock()
        .map(|output| output.clone())
        .unwrap_or_default();
    if output_exceeded.load(Ordering::Relaxed) {
        return Err(limit_error(
            "limit_output_bytes",
            "script output reached its byte budget",
        ));
    }
    Ok((stdout, value))
}

fn validate_budgets(budgets: &ScriptBudgets) -> Result<(), ScriptFailure> {
    let maximums = ScriptBudgets::hard_limits();
    macro_rules! validate {
        ($field:ident) => {
            if budgets.$field == 0 || budgets.$field > maximums.$field {
                return Err(configuration_error(
                    concat!("configuration_budget_", stringify!($field)),
                    format!(
                        "{} must be from 1 to {}",
                        stringify!($field),
                        maximums.$field
                    ),
                ));
            }
        };
    }
    validate!(source_bytes);
    validate!(operations);
    validate!(call_depth);
    validate!(expression_depth);
    validate!(collection_items);
    validate!(string_bytes);
    validate!(output_bytes);
    validate!(wall_time_ms);
    validate!(broker_requests);
    validate!(broker_return_bytes);
    validate!(capture_bytes);
    validate!(event_items);
    validate!(wait_time_ms);
    Ok(())
}

fn validate_available_apis(source: &str, _budgets: &ScriptBudgets) -> Result<(), ScriptFailure> {
    for surface_path in qualified_function_calls(source) {
        let entry = agenterm::script_catalog::entries()
            .into_iter()
            .find(|entry| entry.surface_path == surface_path);
        let Some(entry) = entry else {
            return Err(script_error(
                "script_api_unknown",
                format!("unknown shipped scripting API: {surface_path}"),
            ));
        };
        if entry.status != agenterm::script_catalog::ScriptApiStatus::Shipped {
            return Err(script_error(
                "script_api_unavailable",
                format!("API {surface_path} is not shipped"),
            ));
        }
    }
    if let Some(method) = agent_method_calls(source).into_iter().next() {
        let replacement = match method.as_str() {
            "workspace" => "fleet.workspace.info()",
            "tabs" => "fleet.tabs.list()",
            "active_tab" => "fleet.tabs.active()",
            "ui_snapshot" => "fleet.ui.snapshot()",
            "capture" => "fleet.terminal(tab).capture(max_bytes)",
            "events_read" => "fleet.events.read(epoch, after, limit)",
            "events_wait" => "fleet.events.wait(epoch, after, kind, timeout_ms)",
            _ => "the canonical fleet object",
        };
        return Err(script_error(
            "script_api_migrated",
            format!("agent.{method} was removed in Script API v2; use {replacement}"),
        ));
    }
    for surface_path in fleet_method_calls(source) {
        if matches!(surface_path.as_str(), "fleet.terminal" | "fleet.operations") {
            continue;
        }
        let entry = agenterm::script_catalog::entries()
            .into_iter()
            .find(|entry| entry.surface_path == surface_path);
        let Some(entry) = entry else {
            return Err(script_error(
                "script_api_unknown",
                format!("unknown shipped scripting API: {surface_path}"),
            ));
        };
        if entry.status != agenterm::script_catalog::ScriptApiStatus::Shipped {
            return Err(script_error(
                "script_api_unavailable",
                format!("API {surface_path} is not shipped"),
            ));
        }
    }
    for call in external_function_calls(source) {
        match call.as_str() {
            "print" | "debug" | "type_of" | "is_def_var" | "is_shared" | "eval" | "to_string"
            | "to_debug" => {}
            "new_tab" => {
                return Err(script_error(
                    "script_api_unavailable",
                    "API new_tab is not shipped",
                ));
            }
            _ => {
                return Err(script_error(
                    "script_api_unknown",
                    format!("unknown shipped scripting API: {call}"),
                ));
            }
        }
    }
    Ok(())
}

fn qualified_function_calls(source: &str) -> Vec<String> {
    let bytes = source.as_bytes();
    let mut calls = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if matches!(bytes[index], b'"' | b'\'' | b'`') {
            let quote = bytes[index];
            index += 1;
            while index < bytes.len() {
                if bytes[index] == b'\\' {
                    index = (index + 2).min(bytes.len());
                } else if bytes[index] == quote {
                    index += 1;
                    break;
                } else {
                    index += 1;
                }
            }
            continue;
        }
        if bytes[index] == b'/' && bytes.get(index + 1) == Some(&b'/') {
            index += 2;
            while index < bytes.len() && bytes[index] != b'\n' {
                index += 1;
            }
            continue;
        }
        if bytes[index] == b'/' && bytes.get(index + 1) == Some(&b'*') {
            index += 2;
            while index + 1 < bytes.len() && &bytes[index..index + 2] != b"*/" {
                index += 1;
            }
            index = (index + 2).min(bytes.len());
            continue;
        }
        if source[index..].starts_with("std::") || source[index..].starts_with("rhai::") {
            let start = index;
            while index < bytes.len()
                && (bytes[index] == b'_'
                    || bytes[index] == b':'
                    || bytes[index].is_ascii_alphanumeric())
            {
                index += 1;
            }
            let mut next = index;
            while next < bytes.len() && bytes[next].is_ascii_whitespace() {
                next += 1;
            }
            if bytes.get(next) == Some(&b'(') {
                calls.push(source[start..index].to_owned());
            }
        } else {
            index += 1;
        }
    }
    calls
}

fn agent_method_calls(source: &str) -> Vec<String> {
    let bytes = source.as_bytes();
    let mut methods = Vec::new();
    let mut index = 0;
    while index + 6 < bytes.len() {
        if matches!(bytes[index], b'"' | b'\'' | b'`') {
            let quote = bytes[index];
            index += 1;
            while index < bytes.len() {
                if bytes[index] == b'\\' {
                    index = (index + 2).min(bytes.len());
                } else if bytes[index] == quote {
                    index += 1;
                    break;
                } else {
                    index += 1;
                }
            }
            continue;
        }
        if bytes[index] == b'/' && bytes.get(index + 1) == Some(&b'/') {
            index += 2;
            while index < bytes.len() && bytes[index] != b'\n' {
                index += 1;
            }
            continue;
        }
        if bytes[index] == b'/' && bytes.get(index + 1) == Some(&b'*') {
            index += 2;
            while index + 1 < bytes.len() && &bytes[index..index + 2] != b"*/" {
                index += 1;
            }
            index = (index + 2).min(bytes.len());
            continue;
        }
        if source[index..].starts_with("agent.") {
            let start = index + 6;
            let mut end = start;
            while end < bytes.len() && (bytes[end] == b'_' || bytes[end].is_ascii_alphanumeric()) {
                end += 1;
            }
            let mut next = end;
            while next < bytes.len() && bytes[next].is_ascii_whitespace() {
                next += 1;
            }
            if next < bytes.len() && bytes[next] == b'(' {
                methods.push(source[start..end].to_owned());
            }
            index = end.max(index + 1);
        } else {
            index += 1;
        }
    }
    methods
}

fn fleet_method_calls(source: &str) -> Vec<String> {
    let bytes = source.as_bytes();
    let mut paths = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if matches!(bytes[index], b'"' | b'\'' | b'`') {
            let quote = bytes[index];
            index += 1;
            while index < bytes.len() {
                if bytes[index] == b'\\' {
                    index = (index + 2).min(bytes.len());
                } else if bytes[index] == quote {
                    index += 1;
                    break;
                } else {
                    index += 1;
                }
            }
            continue;
        }
        if bytes[index] == b'/' && bytes.get(index + 1) == Some(&b'/') {
            index += 2;
            while index < bytes.len() && bytes[index] != b'\n' {
                index += 1;
            }
            continue;
        }
        if bytes[index] == b'/' && bytes.get(index + 1) == Some(&b'*') {
            index += 2;
            while index + 1 < bytes.len() && &bytes[index..index + 2] != b"*/" {
                index += 1;
            }
            index = (index + 2).min(bytes.len());
            continue;
        }
        if source[index..].starts_with("fleet.") {
            let start = index;
            index += "fleet.".len();
            while index < bytes.len()
                && (bytes[index] == b'_'
                    || bytes[index] == b'.'
                    || bytes[index].is_ascii_alphanumeric())
            {
                index += 1;
            }
            let mut next = index;
            while next < bytes.len() && bytes[next].is_ascii_whitespace() {
                next += 1;
            }
            if bytes.get(next) == Some(&b'(') {
                paths.push(source[start..index].to_owned());
            }
        } else {
            index += 1;
        }
    }
    paths
}

fn external_function_calls(source: &str) -> Vec<String> {
    let bytes = source.as_bytes();
    let mut calls = Vec::new();
    let mut declared = Vec::new();
    let mut index = 0;
    let mut previous_identifier = None::<String>;
    while index < bytes.len() {
        match bytes[index] {
            b'"' | b'\'' | b'`' => {
                let quote = bytes[index];
                index += 1;
                while index < bytes.len() {
                    if bytes[index] == b'\\' {
                        index = (index + 2).min(bytes.len());
                    } else if bytes[index] == quote {
                        index += 1;
                        break;
                    } else {
                        index += 1;
                    }
                }
            }
            b'/' if bytes.get(index + 1) == Some(&b'/') => {
                index += 2;
                while index < bytes.len() && bytes[index] != b'\n' {
                    index += 1;
                }
            }
            b'/' if bytes.get(index + 1) == Some(&b'*') => {
                index += 2;
                while index + 1 < bytes.len() && !(bytes[index] == b'*' && bytes[index + 1] == b'/')
                {
                    index += 1;
                }
                index = (index + 2).min(bytes.len());
            }
            byte if byte == b'_' || byte.is_ascii_alphabetic() => {
                let start = index;
                index += 1;
                while index < bytes.len()
                    && (bytes[index] == b'_' || bytes[index].is_ascii_alphanumeric())
                {
                    index += 1;
                }
                let identifier = &source[start..index];
                let mut next = index;
                while next < bytes.len() && bytes[next].is_ascii_whitespace() {
                    next += 1;
                }
                if previous_identifier.as_deref() == Some("fn") {
                    declared.push(identifier.to_owned());
                } else if bytes.get(next) == Some(&b'(') {
                    let mut previous = start;
                    while previous > 0 && bytes[previous - 1].is_ascii_whitespace() {
                        previous -= 1;
                    }
                    let is_method_or_qualified =
                        previous > 0 && matches!(bytes[previous - 1], b'.' | b':');
                    let is_keyword = matches!(
                        identifier,
                        "if" | "for" | "while" | "loop" | "switch" | "catch" | "throw"
                    );
                    if !is_method_or_qualified
                        && !is_keyword
                        && !declared.iter().any(|name| name == identifier)
                    {
                        calls.push(identifier.to_owned());
                    }
                }
                previous_identifier = Some(identifier.to_owned());
            }
            byte if byte.is_ascii_whitespace() => index += 1,
            _ => {
                previous_identifier = None;
                index += 1;
            }
        }
    }
    calls.retain(|call| !declared.iter().any(|name| name == call));
    calls
}

fn classify_engine_limit(message: String) -> ScriptFailure {
    let lowercase = message.to_ascii_lowercase();
    let code = if lowercase.contains("call") || lowercase.contains("stack") {
        "limit_call_depth"
    } else if lowercase.contains("expression") {
        "limit_expression_depth"
    } else if lowercase.contains("string") {
        "limit_string_bytes"
    } else {
        "limit_collection_items"
    };
    limit_error(code, message)
}

fn classify_compile_error(message: String) -> ScriptFailure {
    let lowercase = message.to_ascii_lowercase();
    if lowercase.contains("script_module_root_escape") {
        script_error("script_module_root_escape", message)
    } else if lowercase.contains("script_module_cycle") {
        script_error("script_module_cycle", message)
    } else if lowercase.contains("script_module_missing") || lowercase.contains("module not found")
    {
        script_error("script_module_missing", message)
    } else if lowercase.contains("script_module_import_literal") {
        script_error("script_module_import_literal", message)
    } else if lowercase.contains("script_module_") {
        script_error("script_module_invalid", message)
    } else if lowercase.contains("expression")
        && (lowercase.contains("depth") || lowercase.contains("complexity"))
    {
        limit_error("limit_expression_depth", message)
    } else if (lowercase.contains("array") || lowercase.contains("map"))
        && lowercase.contains("maximum")
    {
        limit_error("limit_collection_items", message)
    } else if lowercase.contains("string") && lowercase.contains("maximum") {
        limit_error("limit_string_bytes", message)
    } else {
        script_error("script_parse", message)
    }
}

fn protocol_failure(code: impl Into<String>, message: impl Into<String>) -> ScriptResult {
    protocol_failure_for("unknown", code, message)
}

fn protocol_failure_for(
    invocation_id: &str,
    code: impl Into<String>,
    message: impl Into<String>,
) -> ScriptResult {
    ScriptResult {
        envelope_version: SCRIPT_ENVELOPE_VERSION,
        invocation_id: invocation_id.to_owned(),
        api_version: SCRIPT_API_VERSION,
        ok: false,
        exit_class: ScriptExitClass::Protocol,
        operation: None,
        profile: None,
        stdout: String::new(),
        value: None,
        failure: Some(protocol_error(code, message)),
        duration_ms: 0,
    }
}

fn configuration_error(code: impl Into<String>, message: impl Into<String>) -> ScriptFailure {
    failure(code, message, ScriptFailureCategory::Configuration)
}

fn limit_error(code: impl Into<String>, message: impl Into<String>) -> ScriptFailure {
    failure(code, message, ScriptFailureCategory::Limit)
}

fn script_error(code: impl Into<String>, message: impl Into<String>) -> ScriptFailure {
    failure(code, message, ScriptFailureCategory::Script)
}

fn child_error(code: impl Into<String>, message: impl Into<String>) -> ScriptFailure {
    failure(code, message, ScriptFailureCategory::Child)
}

fn cancelled_error(code: impl Into<String>, message: impl Into<String>) -> ScriptFailure {
    failure(code, message, ScriptFailureCategory::Cancelled)
}

fn fleet_error(code: impl Into<String>, message: impl Into<String>) -> ScriptFailure {
    failure(code, message, ScriptFailureCategory::Fleet)
}

fn classify_runtime_error(error: &EvalAltResult, message: String) -> ScriptFailure {
    if let Some(fields) = agenterm::script_error::runtime_error_fields(error) {
        return match fields.class.as_str() {
            "configuration" => configuration_error(fields.code, fields.safe_message),
            "limit" => limit_error(fields.code, fields.safe_message),
            "child" => child_error(fields.code, fields.safe_message),
            "cancelled" => cancelled_error(fields.code, fields.safe_message),
            "fleet" => fleet_error(fields.code, fields.safe_message),
            "protocol" => protocol_error(fields.code, fields.safe_message),
            "host" => failure(
                fields.code,
                fields.safe_message,
                ScriptFailureCategory::Host,
            ),
            _ => script_error(fields.code, fields.safe_message),
        };
    }
    let classified = message
        .split(|character: char| {
            !character.is_ascii_alphanumeric() && character != '_' && character != '-'
        })
        .find_map(|token| {
            if matches!(
                token,
                "process_spawn"
                    | "child_nonzero"
                    | "process_stdout_unavailable"
                    | "process_stderr_unavailable"
                    | "process_stdin_write"
                    | "process_child_state_poisoned"
                    | "process_child_missing"
                    | "process_child_completed"
                    | "process_try_wait"
                    | "process_kill"
                    | "process_timeout"
                    | "process_stdout_not_utf8"
                    | "process_stderr_not_utf8"
            ) {
                Some((ScriptFailureCategory::Child, token.to_owned()))
            } else if matches!(
                token,
                "fleet_catalog_encode"
                    | "fleet_receipt_invalid"
                    | "fleet_result_decode"
                    | "broker_host_error"
                    | "broker_invalid_receipt"
                    | "broker_invalid_response"
                    | "broker_post_state_missing"
                    | "broker_receipt_missing"
                    | "broker_transport"
                    | "broker_response_timeout"
                    | "server_restart"
                    | "journal_gap"
                    | "future_sequence"
                    | "event_wait_timeout"
            ) {
                Some((ScriptFailureCategory::Fleet, token.to_owned()))
            } else {
                None
            }
        });
    match classified {
        Some((ScriptFailureCategory::Child, code)) => child_error(code, message),
        Some((ScriptFailureCategory::Fleet, code)) => fleet_error(code, message),
        _ => script_error("script_runtime", message),
    }
}

fn protocol_error(code: impl Into<String>, message: impl Into<String>) -> ScriptFailure {
    failure(code, message, ScriptFailureCategory::Protocol)
}

fn failure(
    code: impl Into<String>,
    message: impl Into<String>,
    category: ScriptFailureCategory,
) -> ScriptFailure {
    ScriptFailure {
        code: code.into(),
        message: message.into(),
        category,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Read, Write};

    struct ChannelReader {
        receiver: mpsc::Receiver<Vec<u8>>,
        current: Cursor<Vec<u8>>,
    }

    impl ChannelReader {
        fn new(receiver: mpsc::Receiver<Vec<u8>>) -> Self {
            Self {
                receiver,
                current: Cursor::new(Vec::new()),
            }
        }
    }

    impl Read for ChannelReader {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            loop {
                let read = self.current.read(buffer)?;
                if read != 0 {
                    return Ok(read);
                }
                match self.receiver.recv() {
                    Ok(bytes) => self.current = Cursor::new(bytes),
                    Err(_) => return Ok(0),
                }
            }
        }
    }

    #[derive(Clone, Default)]
    struct SharedBuffer(Arc<Mutex<Vec<u8>>>);

    impl Write for SharedBuffer {
        fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
            self.0
                .lock()
                .expect("shared test output lock")
                .extend_from_slice(buffer);
            Ok(buffer.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    struct ReplHarness {
        sender: Option<mpsc::Sender<Vec<u8>>>,
        output: SharedBuffer,
        worker: Option<std::thread::JoinHandle<anyhow::Result<()>>>,
    }

    impl ReplHarness {
        fn new() -> Self {
            let (sender, receiver) = mpsc::channel();
            let output = SharedBuffer::default();
            let worker_output = output.clone();
            let worker = std::thread::spawn(move || {
                process_concurrent_framed_worker(ChannelReader::new(receiver), worker_output)
            });
            Self {
                sender: Some(sender),
                output,
                worker: Some(worker),
            }
        }

        fn send(&self, frame: &ScriptFrame) {
            self.sender
                .as_ref()
                .expect("live REPL input")
                .send(encoded_frame(frame))
                .expect("send REPL frame");
        }

        fn wait_for_frames(&self, expected: usize) -> Vec<ScriptFrame> {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                let bytes = self
                    .output
                    .0
                    .lock()
                    .expect("shared test output lock")
                    .clone();
                let frames = decoded_complete_frames(&bytes);
                if frames.len() >= expected {
                    return frames;
                }
                assert!(
                    Instant::now() < deadline,
                    "timed out waiting for {expected} frames; got {}",
                    frames.len()
                );
                std::thread::yield_now();
            }
        }

        fn finish(mut self) {
            self.sender.take();
            self.worker
                .take()
                .expect("REPL worker thread")
                .join()
                .expect("join REPL worker")
                .expect("REPL worker result");
        }
    }

    impl Drop for ReplHarness {
        fn drop(&mut self) {
            self.sender.take();
            if let Some(worker) = self.worker.take() {
                let _ = worker.join();
            }
        }
    }

    fn invocation(operation: ScriptOperation, source: &str) -> ScriptInvocation {
        ScriptInvocation {
            envelope_version: SCRIPT_ENVELOPE_VERSION,
            invocation_id: "unit-invocation".to_owned(),
            api_version: SCRIPT_API_VERSION,
            operation,
            profile: ScriptProfile::Pure,
            source_label: "unit".to_owned(),
            source: source.to_owned(),
            project_root: None,
            invocation_temp_root: None,
            arguments: Vec::new(),
            budgets: ScriptBudgets::default(),
            observation: None,
        }
    }

    fn failure_code(result: &ScriptResult) -> &str {
        result
            .failure
            .as_ref()
            .map(|failure| failure.code.as_str())
            .expect("expected failure")
    }

    fn invoke_frame(frame_id: &str, invocation_id: &str, source: &str) -> ScriptFrame {
        let mut invocation = invocation(ScriptOperation::Eval, source);
        invocation.invocation_id = invocation_id.to_owned();
        ScriptFrame {
            frame_version: SCRIPT_FRAME_VERSION,
            frame_id: frame_id.to_owned(),
            payload: ScriptFramePayload::Invoke(invocation),
        }
    }

    fn encoded_frame(frame: &ScriptFrame) -> Vec<u8> {
        let mut bytes = Vec::new();
        write_frame(&mut bytes, frame).expect("encode frame");
        bytes
    }

    fn decoded_frames(bytes: &[u8]) -> Vec<ScriptFrame> {
        let mut input = Cursor::new(bytes);
        let mut frames = Vec::new();
        loop {
            match read_script_frame(&mut input).expect("decode frame") {
                ScriptFrameRead::Eof => break,
                ScriptFrameRead::Frame(frame) => frames.push(*frame),
                ScriptFrameRead::Rejected(rejection) => {
                    panic!("unexpected rejected frame: {rejection:?}")
                }
            }
        }
        frames
    }

    fn decoded_complete_frames(bytes: &[u8]) -> Vec<ScriptFrame> {
        let mut frames = Vec::new();
        let mut offset = 0;
        while bytes.len().saturating_sub(offset) >= std::mem::size_of::<u32>() {
            let length = u32::from_be_bytes(
                bytes[offset..offset + 4]
                    .try_into()
                    .expect("length prefix slice"),
            ) as usize;
            let end = offset.saturating_add(4).saturating_add(length);
            if end > bytes.len() {
                break;
            }
            let mut input = Cursor::new(&bytes[offset..end]);
            match read_script_frame(&mut input) {
                Ok(ScriptFrameRead::Frame(frame)) => frames.push(*frame),
                Ok(ScriptFrameRead::Eof) => panic!("complete test frame decoded as EOF"),
                Ok(ScriptFrameRead::Rejected(rejection)) => {
                    panic!("unexpected rejected frame: {rejection:?}")
                }
                Err(error) => panic!("failed to decode test frame: {error}"),
            }
            offset = end;
        }
        frames
    }

    fn repl_frame(sequence: u64, command: ReplSessionCommand) -> ScriptFrame {
        ScriptFrame {
            frame_version: SCRIPT_FRAME_VERSION,
            frame_id: format!("repl-request-{sequence}"),
            payload: ScriptFramePayload::ReplRequest(
                agenterm::script_protocol::ReplSessionRequest {
                    session_id: "worker-session".to_owned(),
                    generation: 1,
                    sequence,
                    command,
                },
            ),
        }
    }

    fn repl_open(sequence: u64) -> ScriptFrame {
        let budgets = ScriptBudgets {
            operations: ScriptBudgets::hard_limits().operations,
            wall_time_ms: 5_000,
            ..ScriptBudgets::default()
        };
        repl_frame(
            sequence,
            ReplSessionCommand::Open {
                config: ReplSessionWireConfig {
                    budgets,
                    arguments: vec!["argument".to_owned()],
                    project_root: None,
                    invocation_temp_root: None,
                },
            },
        )
    }

    fn repl_events(frames: &[ScriptFrame]) -> Vec<&ReplSessionEvent> {
        frames
            .iter()
            .filter_map(|frame| match &frame.payload {
                ScriptFramePayload::ReplResponse(response) => Some(&response.event),
                _ => None,
            })
            .collect()
    }

    fn frame_result(frame: &ScriptFrame) -> &ScriptResult {
        match &frame.payload {
            ScriptFramePayload::Result(result) => result,
            _ => panic!("expected result frame"),
        }
    }

    #[test]
    fn api_catalog_reports_defaults_maximums_availability_and_exit_classes() {
        let result = execute(invocation(ScriptOperation::Api, ""));
        assert!(result.ok);
        assert_eq!(result.operation, Some(ScriptOperation::Api));
        assert_eq!(result.profile, Some(ScriptProfile::Pure));
        let catalog = result.value.expect("API catalog");
        assert_eq!(
            catalog["limits"]["defaults"]["wall_time_ms"],
            ScriptBudgets::default().wall_time_ms
        );
        assert_eq!(
            catalog["limits"]["hard_maximums"]["wall_time_ms"],
            ScriptBudgets::hard_limits().wall_time_ms
        );
        assert_eq!(
            catalog["limits"]["invocation_bytes"],
            SCRIPT_INVOCATION_MAX_BYTES
        );
        assert_eq!(catalog["exit_classes"]["configuration"], 2);
        assert_eq!(catalog["exit_classes"]["limit"], 3);
        assert_eq!(catalog["exit_classes"]["child"], 4);
        assert_eq!(catalog["exit_classes"]["cancelled"], 5);
        assert_eq!(catalog["exit_classes"]["fleet"], 6);
        assert_eq!(
            catalog["typed_error"]["catchable_slices"][0],
            "std.process.Output.require_success"
        );
        assert_eq!(
            catalog["typed_error"]["fields"].as_array().map(Vec::len),
            Some(8)
        );
        assert_eq!(catalog["schema_version"], 3);
        let apis = catalog["entries"].as_array().expect("API entries");
        let new_tab = apis
            .iter()
            .find(|api| api["stable_id"] == "fleet.tabs.new")
            .expect("deferred control API");
        assert_eq!(new_tab["status"], "planned");
        let workspace = apis
            .iter()
            .find(|api| api["surface_path"] == "fleet.workspace.info")
            .expect("typed workspace API");
        assert_eq!(workspace["operation_id"], "workspace.info");
        assert_eq!(workspace["status"], "shipped");
        assert_eq!(
            catalog["limits"]["defaults"]["broker_requests"],
            ScriptBudgets::default().broker_requests
        );
    }

    #[test]
    fn check_rejects_unknown_and_unavailable_apis() {
        let unknown = execute(invocation(ScriptOperation::Check, "made_up_api()"));
        assert_eq!(failure_code(&unknown), "script_api_unknown");
        assert_eq!(unknown.exit_class, ScriptExitClass::Script);

        let unavailable = execute(invocation(ScriptOperation::Check, "new_tab()"));
        assert_eq!(failure_code(&unavailable), "script_api_unavailable");
        assert_eq!(unavailable.exit_class, ScriptExitClass::Script);
    }

    #[test]
    fn check_accepts_shipped_api_methods_and_user_functions() {
        let source = r#"
            fn twice(value) { value * 2 }
            let values = [1, 2];
            print(twice(values.len()));
        "#;
        assert!(execute(invocation(ScriptOperation::Check, source)).ok);
    }

    #[test]
    fn check_exposes_typed_fleet_api_to_every_legacy_label_and_reports_v2_migration() {
        let mut observe = invocation(ScriptOperation::Check, "fleet.workspace.info()");
        observe.profile = ScriptProfile::Observe;
        assert!(execute(observe).ok);

        let pure = execute(invocation(ScriptOperation::Check, "fleet.workspace.info()"));
        assert!(pure.ok);

        let mut unknown = invocation(ScriptOperation::Check, "fleet.not_shipped()");
        unknown.profile = ScriptProfile::Observe;
        assert_eq!(failure_code(&execute(unknown)), "script_api_unknown");

        let mut migrated = invocation(ScriptOperation::Check, "agent.workspace()");
        migrated.profile = ScriptProfile::Observe;
        let migrated = execute(migrated);
        assert_eq!(failure_code(&migrated), "script_api_migrated");
        assert!(
            migrated
                .failure
                .as_ref()
                .is_some_and(|failure| failure.message.contains("fleet.workspace.info()"))
        );
        assert!(agent_method_calls(r#""agent.hidden()"; // agent.comment()"#).is_empty());
        assert_eq!(
            fleet_method_calls(
                r#"fleet.workspace.info(); fleet.ui.tabs.set_width(240); fleet.terminal("@1").capture(32)"#
            ),
            [
                "fleet.workspace.info",
                "fleet.ui.tabs.set_width",
                "fleet.terminal"
            ]
        );
    }

    #[test]
    fn local_profile_runs_base_rhai_and_accepts_fleet_contract() {
        let mut local = invocation(ScriptOperation::Eval, "40 + 2");
        local.profile = ScriptProfile::Local;
        let local = execute(local);
        assert!(local.ok);
        assert_eq!(local.value, Some(serde_json::json!(42)));
        assert_eq!(local.profile, Some(ScriptProfile::Local));

        let mut fleet = invocation(ScriptOperation::Check, "fleet.workspace.info()");
        fleet.profile = ScriptProfile::Local;
        assert!(execute(fleet).ok);

        let mut shipped = invocation(
            ScriptOperation::Check,
            r#"rhai::json::parse(`{"answer":42}`)"#,
        );
        shipped.profile = ScriptProfile::Local;
        assert!(execute(shipped).ok);

        let mut unknown = invocation(ScriptOperation::Check, "std::fs::not_shipped(`x`)");
        unknown.profile = ScriptProfile::Local;
        assert_eq!(failure_code(&execute(unknown)), "script_api_unknown");

        assert!(
            qualified_function_calls(r#""std::fs::hidden()"; // rhai::json::hidden()"#).is_empty()
        );
    }

    #[test]
    fn invalid_or_excessive_budget_is_configuration_failure() {
        let mut zero = invocation(ScriptOperation::Eval, "1");
        zero.budgets.operations = 0;
        let zero = execute(zero);
        assert_eq!(failure_code(&zero), "configuration_budget_operations");
        assert_eq!(zero.exit_class, ScriptExitClass::Configuration);

        let mut excessive = invocation(ScriptOperation::Eval, "1");
        excessive.budgets.output_bytes = ScriptBudgets::hard_limits().output_bytes + 1;
        assert_eq!(
            failure_code(&execute(excessive)),
            "configuration_budget_output_bytes"
        );
    }

    #[test]
    fn operation_output_and_wall_time_limits_are_typed() {
        let mut operations = invocation(ScriptOperation::Eval, "loop {}");
        operations.budgets.operations = 100;
        let operations = execute(operations);
        assert_eq!(failure_code(&operations), "limit_operations");
        assert_eq!(operations.exit_class, ScriptExitClass::Limit);

        let mut output = invocation(ScriptOperation::Eval, r#"print("abcdef")"#);
        output.budgets.output_bytes = 3;
        assert_eq!(failure_code(&execute(output)), "limit_output_bytes");

        let mut wall_time = invocation(ScriptOperation::Eval, "loop {}");
        wall_time.budgets.operations = ScriptBudgets::hard_limits().operations;
        wall_time.budgets.wall_time_ms = 1;
        let wall_time = execute(wall_time);
        assert_eq!(failure_code(&wall_time), "limit_wall_time");
        assert_eq!(wall_time.exit_class, ScriptExitClass::Limit);
    }

    #[test]
    fn cooperative_cancellation_is_typed_before_wall_time() {
        let mut invocation = invocation(ScriptOperation::Eval, "loop {}");
        invocation.budgets.wall_time_ms = ScriptBudgets::hard_limits().wall_time_ms;
        invocation.budgets.operations = ScriptBudgets::hard_limits().operations;
        let cancellation = Arc::new(AtomicBool::new(true));
        let result = execute_with_cancellation(invocation, Some(cancellation));
        assert_eq!(failure_code(&result), "limit_cancelled");
        assert_eq!(result.exit_class, ScriptExitClass::Cancelled);
    }

    #[test]
    fn runtime_failures_preserve_child_and_fleet_exit_classes() {
        let child_error: Box<EvalAltResult> = "process_spawn: executable missing (line 1)".into();
        let child = classify_runtime_error(&child_error, child_error.to_string());
        assert_eq!(child.code, "process_spawn");
        assert_eq!(child.category, ScriptFailureCategory::Child);

        let fleet_error: Box<EvalAltResult> = "server_restart: epoch changed (line 1)".into();
        let fleet = classify_runtime_error(&fleet_error, fleet_error.to_string());
        assert_eq!(fleet.code, "server_restart");
        assert_eq!(fleet.category, ScriptFailureCategory::Fleet);

        let script_error_value: Box<EvalAltResult> = "user failure (line 1)".into();
        let script = classify_runtime_error(&script_error_value, script_error_value.to_string());
        assert_eq!(script.code, "script_runtime");
        assert_eq!(script.category, ScriptFailureCategory::Script);

        let user_error: Box<EvalAltResult> = "user operation failed (line 1)".into();
        let classified = classify_runtime_error(&user_error, user_error.to_string());
        assert_eq!(classified.code, "script_runtime");
        assert_eq!(classified.category, ScriptFailureCategory::Script);
    }

    #[test]
    fn source_and_expression_limits_are_typed() {
        let mut source = invocation(ScriptOperation::Check, "12345");
        source.budgets.source_bytes = 4;
        assert_eq!(failure_code(&execute(source)), "limit_source_bytes");

        let mut expression = invocation(
            ScriptOperation::Check,
            "((((((((((((((((((((((((((((((((1))))))))))))))))))))))))))))))))",
        );
        expression.budgets.expression_depth = 4;
        let expression = execute(expression);
        assert_eq!(
            failure_code(&expression),
            "limit_expression_depth",
            "{:?}",
            expression.failure
        );
    }

    #[test]
    fn call_collection_and_string_limits_are_typed() {
        let mut call_depth = invocation(
            ScriptOperation::Eval,
            "fn recurse() { recurse(); } recurse();",
        );
        call_depth.budgets.call_depth = 2;
        assert_eq!(failure_code(&execute(call_depth)), "limit_call_depth");

        let mut collection = invocation(ScriptOperation::Eval, "[1, 2, 3]");
        collection.budgets.collection_items = 2;
        let collection = execute(collection);
        assert_eq!(
            failure_code(&collection),
            "limit_collection_items",
            "{:?}",
            collection.failure
        );

        let mut string = invocation(ScriptOperation::Eval, r#""abcdef""#);
        string.budgets.string_bytes = 3;
        assert_eq!(failure_code(&execute(string)), "limit_string_bytes");
    }

    #[test]
    fn malformed_and_oversized_invocations_have_protocol_envelopes() {
        let malformed = protocol_failure("protocol_invalid_invocation", "bad JSON");
        assert!(!malformed.ok);
        assert_eq!(malformed.exit_class, ScriptExitClass::Protocol);
        assert_eq!(
            malformed
                .failure
                .as_ref()
                .expect("protocol failure")
                .category,
            ScriptFailureCategory::Protocol
        );
        assert!(malformed.operation.is_none());
        assert!(malformed.profile.is_none());

        let oversized = vec![0_u8; SCRIPT_INVOCATION_MAX_BYTES as usize + 1];
        assert!(oversized.len() as u64 > SCRIPT_INVOCATION_MAX_BYTES);
    }

    #[test]
    fn api_scanner_ignores_strings_comments_and_method_calls() {
        let source = r#"
            // hidden_api()
            let text = "also_hidden()";
            values.len();
            /* another_hidden() */
        "#;
        assert!(external_function_calls(source).is_empty());
    }

    #[test]
    fn api_scanner_accepts_forward_function_declarations() {
        let source = "twice(21); fn twice(value) { value * 2 }";
        assert!(external_function_calls(source).is_empty());
    }

    #[test]
    fn api_scanner_treats_parenthesized_throw_as_a_keyword() {
        let source = r#"throw ("typed_failure:" + value.to_string());"#;
        assert!(external_function_calls(source).is_empty());
    }

    #[test]
    fn framed_worker_runs_multiple_invocations_without_stdout_corruption() {
        let mut input = encoded_frame(&invoke_frame(
            "frame-one",
            "invocation-one",
            r#"print("inside-result"); 21 * 2"#,
        ));
        input.extend(encoded_frame(&invoke_frame(
            "frame-two",
            "invocation-two",
            "6 * 7",
        )));
        let mut output = Vec::new();
        process_framed_stream(Cursor::new(input), &mut output).expect("framed stream");

        let frames = decoded_frames(&output);
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].frame_id, "frame-one");
        assert_eq!(frame_result(&frames[0]).stdout, "inside-result\n");
        assert_eq!(frame_result(&frames[0]).value, Some(serde_json::json!(42)));
        assert_eq!(frames[1].frame_id, "frame-two");
        assert_eq!(frame_result(&frames[1]).value, Some(serde_json::json!(42)));
    }

    #[test]
    fn framed_repl_persists_queries_resets_and_closes_one_session() {
        let harness = ReplHarness::new();
        harness.send(&repl_open(0));
        assert!(matches!(
            repl_events(&harness.wait_for_frames(1))[0],
            ReplSessionEvent::Ready { .. }
        ));

        harness.send(&repl_frame(
            1,
            ReplSessionCommand::Inspect {
                source: "fn add(n) {".to_owned(),
            },
        ));
        assert!(matches!(
            repl_events(&harness.wait_for_frames(2))[1],
            ReplSessionEvent::InputState {
                state: ReplWireInputState::Incomplete
            }
        ));

        harness.send(&repl_frame(
            2,
            ReplSessionCommand::Evaluate {
                cell_id: "cell-1".to_owned(),
                source: "let x = 40; fn add(n) { 40 + n }".to_owned(),
            },
        ));
        let frames = harness.wait_for_frames(4);
        assert!(matches!(
            repl_events(&frames)[3],
            ReplSessionEvent::CellResult {
                result: ReplWireCellResult { ok: true, .. },
                ..
            }
        ));

        harness.send(&repl_frame(
            3,
            ReplSessionCommand::Evaluate {
                cell_id: "cell-2".to_owned(),
                source: "[x, add(2)]".to_owned(),
            },
        ));
        let frames = harness.wait_for_frames(6);
        let events = repl_events(&frames);
        let ReplSessionEvent::CellResult { result, .. } = events[5] else {
            panic!("second cell did not return a result");
        };
        assert!(result.ok, "{result:?}");
        assert_eq!(
            result.value.as_ref().and_then(|value| value.value.as_ref()),
            Some(&serde_json::json!([40, 42]))
        );

        harness.send(&repl_frame(
            4,
            ReplSessionCommand::Query {
                query: ReplSessionQuery::State,
            },
        ));
        let frames = harness.wait_for_frames(7);
        let ReplSessionEvent::QueryResult {
            result:
                ReplWireQueryResult::State {
                    history,
                    variables,
                    functions,
                    ..
                },
            ..
        } = repl_events(&frames)[6]
        else {
            panic!("state query did not return typed state");
        };
        assert_eq!(history.len(), 2);
        assert!(variables.iter().any(|variable| variable.name == "x"));
        assert!(functions.iter().any(|function| function == "add(n)"));

        harness.send(&repl_frame(5, ReplSessionCommand::Reset));
        assert!(matches!(
            repl_events(&harness.wait_for_frames(8))[7],
            ReplSessionEvent::ResetDone
        ));
        harness.send(&repl_frame(
            6,
            ReplSessionCommand::Query {
                query: ReplSessionQuery::Variables,
            },
        ));
        let frames = harness.wait_for_frames(9);
        let ReplSessionEvent::QueryResult {
            result: ReplWireQueryResult::Variables(variables),
            ..
        } = repl_events(&frames)[8]
        else {
            panic!("variables query did not return variables");
        };
        assert!(!variables.iter().any(|variable| variable.name == "x"));

        harness.send(&repl_frame(7, ReplSessionCommand::Close));
        assert!(matches!(
            repl_events(&harness.wait_for_frames(10))[9],
            ReplSessionEvent::Closed
        ));
        harness.finish();
    }

    #[test]
    fn framed_repl_cancels_an_active_cpu_cell_without_losing_the_session() {
        let harness = ReplHarness::new();
        harness.send(&repl_open(0));
        harness.wait_for_frames(1);
        harness.send(&repl_frame(
            1,
            ReplSessionCommand::Evaluate {
                cell_id: "loop-cell".to_owned(),
                source: "while true {}".to_owned(),
            },
        ));
        // Send cancellation without waiting for CellStarted. The stdin thread
        // captured the baseline at Evaluate admission, so this cannot be lost
        // even when the session thread has not begun evaluation yet.
        harness.send(&repl_frame(
            2,
            ReplSessionCommand::Cancel {
                cell_id: "loop-cell".to_owned(),
            },
        ));
        let frames = harness.wait_for_frames(3);
        let events = repl_events(&frames);
        assert!(matches!(
            events[1],
            ReplSessionEvent::CellStarted { cell_id } if cell_id == "loop-cell"
        ));
        let ReplSessionEvent::CellResult { result, .. } = events[2] else {
            panic!("cancelled cell did not return a result");
        };
        assert_eq!(
            result.failure.as_ref().map(|failure| failure.code.as_str()),
            Some("limit_cancelled")
        );
        assert!(!result.state_committed);

        harness.send(&repl_frame(
            3,
            ReplSessionCommand::Evaluate {
                cell_id: "recovery-cell".to_owned(),
                source: "40 + 2".to_owned(),
            },
        ));
        let frames = harness.wait_for_frames(5);
        let ReplSessionEvent::CellResult { result, .. } = repl_events(&frames)[4] else {
            panic!("recovery cell did not return a result");
        };
        assert!(result.ok, "{result:?}");
        harness.send(&repl_frame(4, ReplSessionCommand::Close));
        harness.wait_for_frames(6);
        harness.finish();
    }

    #[test]
    fn framed_repl_and_legacy_protocols_remain_explicitly_isolated() {
        let harness = ReplHarness::new();
        harness.send(&repl_open(0));
        harness.wait_for_frames(1);
        harness.send(&ScriptFrame {
            frame_version: SCRIPT_FRAME_VERSION,
            frame_id: "unexpected-repl-broker-response".to_owned(),
            payload: ScriptFramePayload::BrokerResponse {
                invocation_id: "no-cell".to_owned(),
                request_id: "no-request".to_owned(),
                response: ScriptBrokerResponse {
                    ok: true,
                    value: None,
                    error: None,
                },
            },
        });
        let frames = harness.wait_for_frames(2);
        assert!(matches!(
            &frames[1].payload,
            ScriptFramePayload::ReplResponse(ReplSessionResponse {
                event: ReplSessionEvent::Failure { failure },
                ..
            }) if failure.code == "protocol_broker_response_unexpected"
        ));
        harness.send(&invoke_frame("legacy-frame", "legacy-invocation", "40 + 2"));
        harness.send(&invoke_frame("legacy-frame", "legacy-invocation", "40 + 2"));
        let frames = harness.wait_for_frames(4);
        for frame in &frames[2..4] {
            assert!(matches!(
                &frame.payload,
                ScriptFramePayload::ReplResponse(ReplSessionResponse {
                    event: ReplSessionEvent::Failure { failure },
                    ..
                }) if failure.code == "protocol_repl_session_active"
            ));
        }
        let mut mismatched = repl_frame(
            1,
            ReplSessionCommand::Query {
                query: ReplSessionQuery::State,
            },
        );
        let ScriptFramePayload::ReplRequest(request) = &mut mismatched.payload else {
            unreachable!("REPL request helper returned another payload")
        };
        request.session_id = "wrong-session".to_owned();
        harness.send(&mismatched);
        let frames = harness.wait_for_frames(5);
        assert!(matches!(
            &frames[4].payload,
            ScriptFramePayload::ReplResponse(ReplSessionResponse {
                session_id,
                event: ReplSessionEvent::Failure { failure },
                ..
            }) if session_id == "worker-session"
                && failure.code == "protocol_repl_session_mismatch"
        ));
        harness.send(&repl_frame(1, ReplSessionCommand::Close));
        harness.wait_for_frames(6);
        harness.finish();

        let harness = ReplHarness::new();
        harness.send(&ScriptFrame {
            frame_version: SCRIPT_FRAME_VERSION,
            frame_id: "unexpected-response".to_owned(),
            payload: ScriptFramePayload::ReplResponse(ReplSessionResponse {
                session_id: "worker-session".to_owned(),
                generation: 1,
                sequence: 0,
                event: ReplSessionEvent::Ready { worker_pid: 7 },
            }),
        });
        let frames = harness.wait_for_frames(1);
        assert_eq!(
            failure_code(frame_result(&frames[0])),
            "protocol_repl_unexpected_response"
        );
        harness.finish();
    }

    #[test]
    fn framed_worker_recovers_after_malformed_and_oversized_frames() {
        let mut input = Vec::new();
        input.extend(1_u32.to_be_bytes());
        input.push(b'{');
        let oversized = SCRIPT_FRAME_MAX_BYTES + 1;
        input.extend(oversized.to_be_bytes());
        input.resize(input.len() + oversized as usize, b'x');
        input.extend(encoded_frame(&invoke_frame(
            "recovery-frame",
            "recovery-invocation",
            "40 + 2",
        )));
        let mut output = Vec::new();
        process_framed_stream(Cursor::new(input), &mut output).expect("framed stream");

        let frames = decoded_frames(&output);
        assert_eq!(frames.len(), 3);
        assert_eq!(
            failure_code(frame_result(&frames[0])),
            "protocol_malformed_frame"
        );
        assert_eq!(
            failure_code(frame_result(&frames[1])),
            "protocol_frame_too_large"
        );
        assert_eq!(frames[2].frame_id, "recovery-frame");
        assert_eq!(frame_result(&frames[2]).value, Some(serde_json::json!(42)));
    }

    #[test]
    fn framed_worker_rejects_versions_duplicates_cancel_and_reserved_frames() {
        let mut unsupported = invoke_frame("unsupported", "never-run", "1");
        unsupported.frame_version = SCRIPT_FRAME_VERSION + 1;
        let first = invoke_frame("first", "same-invocation", "1");
        let duplicate_invocation = invoke_frame("second", "same-invocation", "2");
        let duplicate_frame = invoke_frame("first", "another-invocation", "3");
        let cancel = ScriptFrame {
            frame_version: SCRIPT_FRAME_VERSION,
            frame_id: "cancel".to_owned(),
            payload: ScriptFramePayload::Cancel {
                invocation_id: "same-invocation".to_owned(),
            },
        };
        let broker = ScriptFrame {
            frame_version: SCRIPT_FRAME_VERSION,
            frame_id: "broker".to_owned(),
            payload: ScriptFramePayload::BrokerRequest {
                invocation_id: "same-invocation".to_owned(),
                request_id: "request-one".to_owned(),
                request: ScriptBrokerRequest {
                    operation: "ui.snapshot".to_owned(),
                    arguments: serde_json::json!({}),
                },
            },
        };
        let mut input = Vec::new();
        for frame in [
            unsupported,
            first,
            duplicate_invocation,
            duplicate_frame,
            cancel,
            broker,
        ] {
            input.extend(encoded_frame(&frame));
        }
        let mut output = Vec::new();
        process_framed_stream(Cursor::new(input), &mut output).expect("framed stream");

        let frames = decoded_frames(&output);
        let codes: Vec<_> = frames
            .iter()
            .map(|frame| {
                frame_result(frame)
                    .failure
                    .as_ref()
                    .map(|failure| failure.code.as_str())
            })
            .collect();
        assert_eq!(
            codes,
            vec![
                Some("protocol_unsupported_frame_version"),
                None,
                Some("protocol_duplicate_invocation"),
                Some("protocol_duplicate_frame"),
                Some("protocol_cancel_too_late"),
                Some("protocol_broker_unavailable"),
            ]
        );
    }

    #[test]
    fn framed_worker_replaces_unencodable_large_result_with_typed_failure() {
        let mut result = execute(invocation(ScriptOperation::Eval, "42"));
        result.stdout = "\0".repeat(1024 * 1024);
        let frame = result_frame("large-result".to_owned(), result);
        let mut output = Vec::new();
        write_frame(&mut output, &frame).expect("bounded replacement frame");
        assert!(output.len() <= SCRIPT_FRAME_MAX_BYTES as usize + 4);
        let frames = decoded_frames(&output);
        assert_eq!(
            failure_code(frame_result(&frames[0])),
            "protocol_result_frame_too_large"
        );
    }
}
