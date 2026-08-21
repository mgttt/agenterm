//! Minimal VM evaluator and TinyArcade cartridge conformance front door.
//!
//! Assembles one-instruction-per-line text and runs it on a fresh [`Vm`],
//! printing the resulting stack top. Assembly comes from the arguments after
//! `eval` (joined with newlines) or, if none are given, from stdin.
//!
//! This is intentionally not a REPL framework — the persistent-image REPL is
//! the library's `Vm::eval` loop; this binary is a thin one-shot front door.

use std::io::{Read, Write};
use std::path::Path;
use std::process::ExitCode;

#[cfg(feature = "catalog-publisher")]
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
#[cfg(feature = "catalog-publisher")]
use ring::signature::{Ed25519KeyPair, KeyPair};
#[cfg(feature = "catalog-publisher")]
use serde::{Deserialize, Serialize};
#[cfg(feature = "catalog-publisher")]
use std::collections::{BTreeMap, HashSet};

use agenterm_tinyvm::{
    CartridgeDescriptor, CartridgeManifest, GameInput, GameLimits, GameRuntime, HostProfileV1,
    Limits, MAX_HOST_PROFILE_BYTES, RenderFrame, ToneBatch, Vm, WasmError, WasmModule,
};
#[cfg(feature = "catalog-publisher")]
use agenterm_tinyvm::{CartridgeTrustStore, CatalogEntry, cartridge_sha256};
#[cfg(feature = "replay")]
use agenterm_tinyvm::{MAX_REPLAY_BYTES, ReplayRecorderV1, ReplayTraceV1};

const MEM_CELLS: usize = 4_096;
const MAX_CARTRIDGE_BYTES: u64 = 2 * 1024 * 1024;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("eval") => {
            let rest: Vec<String> = args.collect();
            let src = if rest.is_empty() {
                let mut buf = String::new();
                if let Err(e) = std::io::stdin().read_to_string(&mut buf) {
                    eprintln!("tinyvm: reading stdin: {e}");
                    return ExitCode::FAILURE;
                }
                buf
            } else {
                rest.join("\n")
            };
            run_eval(&src)
        }
        Some("cartridge") => match args.next().as_deref() {
            Some("inspect") => match (args.next(), args.next()) {
                (Some(path), None) => run_cartridge(&path, false),
                _ => usage(),
            },
            Some("check") => match (args.next(), args.next()) {
                (Some(path), None) => run_cartridge(&path, true),
                _ => usage(),
            },
            Some("check-profile") => match (args.next(), args.next(), args.next()) {
                (Some(path), Some(profile), None) => run_cartridge_profile(&path, &profile),
                _ => usage(),
            },
            Some("attach-manifest") => {
                let input = args.next();
                let output = args.next();
                let game_id = args.next();
                let game_version = args.next();
                let abi_version = args.next();
                let state_version = args.next();
                match (
                    input,
                    output,
                    game_id,
                    game_version,
                    abi_version,
                    state_version,
                ) {
                    (
                        Some(input),
                        Some(output),
                        Some(game_id),
                        Some(game_version),
                        Some(abi_version),
                        Some(state_version),
                    ) if args.next().is_none() => run_attach_manifest(
                        &input,
                        &output,
                        game_id,
                        game_version,
                        &abi_version,
                        &state_version,
                    ),
                    _ => usage(),
                }
            }
            _ => usage(),
        },
        Some("host-profile") => match args.next().as_deref() {
            Some("default") => match (args.next(), args.next()) {
                (Some(path), None) => run_host_profile_default(&path),
                _ => usage(),
            },
            Some("inspect") => match (args.next(), args.next()) {
                (Some(path), None) => run_host_profile_inspect(&path),
                _ => usage(),
            },
            _ => usage(),
        },
        #[cfg(feature = "replay")]
        Some("replay") => {
            let operation = args.next();
            let first = args.next();
            let second = args.next();
            let third = args.next();
            let extra = args.next();
            match (operation.as_deref(), first, second, third, extra) {
                (Some("record"), Some(wasm), Some(inputs), Some(output), None) => {
                    run_replay_record(&wasm, &inputs, &output)
                }
                (Some("check"), Some(wasm), Some(trace), None, None) => {
                    run_replay_check(&wasm, &trace)
                }
                _ => usage(),
            }
        }
        #[cfg(feature = "catalog-publisher")]
        Some("catalog") => match (
            args.next().as_deref(),
            args.next(),
            args.next(),
            args.next(),
        ) {
            (Some("build"), Some(source), Some(seed), Some(output)) => {
                run_catalog_build(&source, &seed, &output)
            }
            _ => usage(),
        },
        Some(other) => {
            eprintln!("tinyvm: unknown command `{other}`");
            usage()
        }
        None => usage(),
    }
}

fn usage() -> ExitCode {
    eprintln!("usage:");
    eprintln!("  tinyvm eval [asm...]");
    eprintln!("  tinyvm cartridge inspect FILE.wasm");
    eprintln!("  tinyvm cartridge check FILE.wasm");
    eprintln!("  tinyvm cartridge check-profile FILE.wasm HOST.tahost");
    eprintln!(
        "  tinyvm cartridge attach-manifest INPUT.wasm OUTPUT.wasm GAME_ID GAME_VERSION ABI_VERSION STATE_VERSION"
    );
    eprintln!("  tinyvm host-profile default OUTPUT.tahost");
    eprintln!("  tinyvm host-profile inspect FILE.tahost");
    #[cfg(feature = "catalog-publisher")]
    eprintln!("  tinyvm catalog build SOURCE.json ED25519-SEED OUTPUT-DIRECTORY");
    #[cfg(feature = "replay")]
    {
        eprintln!("  tinyvm replay record FILE.wasm INPUTS.txt OUTPUT.tareplay");
        eprintln!("  tinyvm replay check FILE.wasm TRACE.tareplay");
    }
    ExitCode::FAILURE
}

#[cfg(feature = "replay")]
fn run_replay_record(wasm_path: &str, inputs_path: &str, output_path: &str) -> ExitCode {
    let result: Result<(String, String, usize, usize), String> = (|| {
        let wasm = read_bounded_regular(Path::new(wasm_path), MAX_CARTRIDGE_BYTES, "cartridge")?;
        let input_bytes =
            read_bounded_regular(Path::new(inputs_path), 1024 * 1024, "replay input plan")?;
        let inputs = parse_replay_inputs(&input_bytes)?;
        let mut runtime = replay_runtime(&wasm)?;
        let mut recorder = ReplayRecorderV1::start(&wasm, &mut runtime)
            .map_err(|error| error.message().to_string())?;
        for input in inputs {
            recorder
                .record_tick(&mut runtime, input)
                .map_err(|error| error.message().to_string())?;
        }
        let trace = recorder
            .finish()
            .map_err(|error| error.message().to_string())?;
        publish_new_file(Path::new(output_path), &trace, "replay output")?;
        let decoded = ReplayTraceV1::decode(&trace).map_err(|error| error.message().to_string())?;
        Ok((
            decoded.game_id,
            decoded.game_version,
            decoded.steps.len(),
            trace.len(),
        ))
    })();
    match result {
        Ok((game_id, version, steps, bytes)) => {
            println!("game_id={game_id}");
            println!("game_version={version}");
            println!("steps={steps}");
            println!("replay_bytes={bytes}");
            println!("OK: deterministic TinyArcade replay v1");
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("tinyvm: {message}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(feature = "replay")]
fn run_replay_check(wasm_path: &str, trace_path: &str) -> ExitCode {
    let result: Result<(String, String, usize), String> = (|| {
        let wasm = read_bounded_regular(Path::new(wasm_path), MAX_CARTRIDGE_BYTES, "cartridge")?;
        let bytes = read_bounded_regular(
            Path::new(trace_path),
            MAX_REPLAY_BYTES as u64,
            "replay trace",
        )?;
        let trace = ReplayTraceV1::decode(&bytes).map_err(|error| error.message().to_string())?;
        trace
            .verify_cartridge(&wasm)
            .map_err(|error| error.message().to_string())?;
        let mut runtime = replay_runtime(&wasm)?;
        let mut frames = 0usize;
        trace
            .replay(&wasm, &mut runtime, |_, _| {
                frames += 1;
                Ok(())
            })
            .map_err(|error| error.message().to_string())?;
        Ok((trace.game_id, trace.game_version, frames))
    })();
    match result {
        Ok((game_id, version, frames)) => {
            println!("game_id={game_id}");
            println!("game_version={version}");
            println!("verified_frames={frames}");
            println!("OK: replay matches exact cartridge outputs");
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("tinyvm: {message}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(feature = "replay")]
fn replay_runtime(wasm: &[u8]) -> Result<GameRuntime, String> {
    GameRuntime::from_private_bytes(
        wasm,
        Limits {
            max_table_elems: 1_024,
            max_memory_pages: 64,
            max_steps: 1_000_000,
            ..Limits::default()
        },
        GameLimits::default(),
        0x5441_5231,
    )
    .map_err(|error| error.message().to_string())
}

#[cfg(feature = "replay")]
fn parse_replay_inputs(bytes: &[u8]) -> Result<Vec<GameInput>, String> {
    let source = core::str::from_utf8(bytes).map_err(|_| "replay input plan is not UTF-8")?;
    let mut inputs = Vec::new();
    for (index, raw) in source.lines().enumerate() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let mut fields = line.split_ascii_whitespace();
        let clock = fields
            .next()
            .ok_or_else(|| format!("input line {} is empty", index + 1))?;
        let buttons = fields
            .next()
            .ok_or_else(|| format!("input line {} needs clock and buttons", index + 1))?;
        if fields.next().is_some() {
            return Err(format!("input line {} has extra fields", index + 1));
        }
        if inputs.len() >= agenterm_tinyvm::replay::MAX_REPLAY_STEPS {
            return Err("replay input plan exceeds step limit".into());
        }
        inputs
            .try_reserve(1)
            .map_err(|_| "replay input allocation".to_string())?;
        inputs.push(GameInput {
            clock_ms: parse_u32(clock)
                .ok_or_else(|| format!("invalid clock on line {}", index + 1))?,
            buttons: parse_u32(buttons)
                .ok_or_else(|| format!("invalid buttons on line {}", index + 1))?,
        });
    }
    if inputs.is_empty() {
        return Err("replay input plan has no steps".into());
    }
    Ok(inputs)
}

#[cfg(feature = "replay")]
fn parse_u32(value: &str) -> Option<u32> {
    value.strip_prefix("0x").map_or_else(
        || value.parse().ok(),
        |hex| u32::from_str_radix(hex, 16).ok(),
    )
}

fn publish_new_file(path: &Path, bytes: &[u8], label: &str) -> Result<(), String> {
    if path.exists() {
        return Err(format!("{label} already exists"));
    }
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let leaf = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| "replay output needs a UTF-8 leaf name".to_string())?;
    let stage = parent.join(format!(".{leaf}.stage-{}", std::process::id()));
    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&stage)
            .map_err(|_| format!("cannot create {label} staging file"))?;
        file.write_all(bytes)
            .map_err(|_| format!("cannot write {label} staging file"))?;
        file.sync_all()
            .map_err(|_| format!("cannot flush {label} staging file"))?;
        std::fs::hard_link(&stage, path).map_err(|_| format!("cannot promote {label}"))?;
        let _ = std::fs::remove_file(&stage);
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&stage);
    }
    result
}

#[cfg(feature = "catalog-publisher")]
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CatalogSource {
    schema_version: u32,
    catalog_id: String,
    signing_key_id: String,
    host_profile: String,
    games: Vec<SourceGame>,
}

#[cfg(feature = "catalog-publisher")]
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceGame {
    wasm: String,
    title: String,
    summary: String,
    #[serde(default)]
    localizations: BTreeMap<String, Localization>,
}

#[cfg(feature = "catalog-publisher")]
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Localization {
    title: String,
    summary: String,
}

#[cfg(feature = "catalog-publisher")]
#[derive(Serialize)]
struct PublishedCatalog {
    schema_version: u32,
    catalog_id: String,
    host_profile: PublishedHostProfile,
    games: Vec<PublishedGame>,
}

#[cfg(feature = "catalog-publisher")]
#[derive(Serialize)]
struct PublishedHostProfile {
    file: String,
    length: u64,
    sha256: String,
}

#[cfg(feature = "catalog-publisher")]
#[derive(Serialize)]
struct PublishedGame {
    game_id: String,
    game_version: String,
    title: String,
    summary: String,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    localizations: BTreeMap<String, Localization>,
    cartridge: String,
    abi_version: u32,
    state_version: u32,
    wasm_length: u64,
    wasm_sha256: String,
    signing_key_id: String,
    signature: String,
}

#[cfg(feature = "catalog-publisher")]
fn run_catalog_build(source_path: &str, seed_path: &str, output_path: &str) -> ExitCode {
    match build_catalog(
        Path::new(source_path),
        Path::new(seed_path),
        Path::new(output_path),
    ) {
        Ok(count) => {
            println!("OK: staged {count} signed cartridge(s) and catalog-v1.json");
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("tinyvm: {message}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(feature = "catalog-publisher")]
fn build_catalog(source_path: &Path, seed_path: &Path, output: &Path) -> Result<usize, String> {
    if output.exists() {
        return Err("output directory already exists".into());
    }
    let source_bytes = read_bounded_regular(source_path, 1024 * 1024, "catalog source")?;
    let source: CatalogSource =
        serde_json::from_slice(&source_bytes).map_err(|_| "invalid catalog source JSON")?;
    if source.schema_version != 1
        || !valid_identifier(&source.catalog_id, 128)
        || !valid_identifier(&source.signing_key_id, 64)
        || source.games.is_empty()
        || source.games.len() > 256
    {
        return Err("invalid catalog source metadata".into());
    }
    let seed = read_signing_seed(seed_path)?;
    let key_pair =
        Ed25519KeyPair::from_seed_unchecked(&seed).map_err(|_| "invalid Ed25519 signing seed")?;
    let source_dir = source_path.parent().unwrap_or_else(|| Path::new("."));
    let host_profile_bytes = read_bounded_regular(
        &source_dir.join(&source.host_profile),
        MAX_HOST_PROFILE_BYTES as u64,
        "host profile",
    )?;
    let host_profile =
        HostProfileV1::decode(&host_profile_bytes).map_err(|error| error.message().to_string())?;
    let host_profile_hash = cartridge_sha256(&host_profile_bytes);
    let parent = output.parent().unwrap_or_else(|| Path::new("."));
    let leaf = output
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| "output directory needs a UTF-8 leaf name".to_string())?;
    let stage = parent.join(format!(".{leaf}.tinyarcade-stage-{}", std::process::id()));
    if stage.exists() {
        return Err("staging directory already exists".into());
    }
    std::fs::create_dir(&stage).map_err(|_| "cannot create staging directory")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if std::fs::set_permissions(&stage, std::fs::Permissions::from_mode(0o700)).is_err() {
            let _ = std::fs::remove_dir(&stage);
            return Err("cannot restrict staging directory".into());
        }
    }

    let result = (|| {
        let mut games = Vec::with_capacity(source.games.len());
        let mut seen = HashSet::new();
        for game in source.games {
            validate_display_metadata(&game)?;
            let wasm_path = source_dir.join(&game.wasm);
            let wasm = read_bounded_regular(&wasm_path, MAX_CARTRIDGE_BYTES, "cartridge")?;
            host_profile
                .inspect_cartridge(&wasm)
                .map_err(|error| error.message().to_string())?;
            let manifest = validate_publishable_cartridge(&wasm)?;
            if !valid_identifier(&manifest.game_id, 128) || !valid_version(&manifest.game_version) {
                return Err("cartridge identity is incompatible with catalog v1".into());
            }
            if !seen.insert(manifest.game_id.clone()) {
                return Err("duplicate game_id in catalog source".into());
            }
            let cartridge = format!("{}-{}.wasm", manifest.game_id, manifest.game_version);
            if cartridge.len() > 160 {
                return Err("published cartridge filename is too long".into());
            }
            let hash = cartridge_sha256(&wasm);
            let mut entry = CatalogEntry {
                game_id: manifest.game_id.clone(),
                game_version: manifest.game_version.clone(),
                abi_version: manifest.abi_version,
                state_version: manifest.state_version,
                wasm_length: wasm.len() as u64,
                wasm_sha256: hash,
                signing_key_id: source.signing_key_id.clone(),
                signature: [0; 64],
            };
            let message = entry.signing_bytes().map_err(|error| error.message())?;
            entry
                .signature
                .copy_from_slice(key_pair.sign(&message).as_ref());
            let mut trust = CartridgeTrustStore::new();
            trust
                .add_key(&source.signing_key_id, key_pair.public_key().as_ref())
                .map_err(|error| error.message())?;
            trust
                .verify(&entry, &wasm)
                .map_err(|error| error.message())?;
            std::fs::write(stage.join(&cartridge), &wasm)
                .map_err(|_| "cannot write staged cartridge")?;
            games.push(PublishedGame {
                game_id: manifest.game_id,
                game_version: manifest.game_version,
                title: game.title,
                summary: game.summary,
                localizations: game.localizations,
                cartridge,
                abi_version: manifest.abi_version,
                state_version: manifest.state_version,
                wasm_length: wasm.len() as u64,
                wasm_sha256: lower_hex(&hash),
                signing_key_id: source.signing_key_id.clone(),
                signature: BASE64.encode(entry.signature),
            });
        }
        games.sort_by(|left, right| left.game_id.cmp(&right.game_id));
        std::fs::write(stage.join("host-profile-v1.tahost"), &host_profile_bytes)
            .map_err(|_| "cannot write staged host profile")?;
        let published = PublishedCatalog {
            schema_version: 1,
            catalog_id: source.catalog_id,
            host_profile: PublishedHostProfile {
                file: "host-profile-v1.tahost".into(),
                length: host_profile_bytes.len() as u64,
                sha256: lower_hex(&host_profile_hash),
            },
            games,
        };
        let mut json =
            serde_json::to_vec_pretty(&published).map_err(|_| "cannot encode catalog JSON")?;
        json.push(b'\n');
        if json.len() > 1024 * 1024 {
            return Err("published catalog exceeds 1 MiB".into());
        }
        std::fs::write(stage.join("catalog-v1.json"), json)
            .map_err(|_| "cannot write staged catalog")?;
        std::fs::rename(&stage, output).map_err(|_| "cannot promote staging directory")?;
        Ok(published.games.len())
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&stage);
    }
    result
}

fn read_bounded_regular(path: &Path, maximum: u64, label: &str) -> Result<Vec<u8>, String> {
    let metadata = std::fs::symlink_metadata(path).map_err(|_| format!("cannot stat {label}"))?;
    if !metadata.file_type().is_file() || metadata.len() == 0 || metadata.len() > maximum {
        return Err(format!("{label} is not a bounded non-empty regular file"));
    }
    let file = std::fs::File::open(path).map_err(|_| format!("cannot open {label}"))?;
    let opened = file
        .metadata()
        .map_err(|_| format!("cannot inspect opened {label}"))?;
    if !opened.file_type().is_file() || opened.len() == 0 || opened.len() > maximum {
        return Err(format!("{label} changed outside its accepted bounds"));
    }
    let capacity = usize::try_from(maximum)
        .ok()
        .and_then(|value| value.checked_add(1))
        .ok_or_else(|| format!("{label} limit is unsupported on this host"))?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(capacity)
        .map_err(|_| format!("cannot allocate bounded {label}"))?;
    file.take(maximum + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| format!("cannot read {label}"))?;
    if bytes.is_empty() || bytes.len() > maximum as usize {
        return Err(format!("{label} changed outside its accepted bounds"));
    }
    Ok(bytes)
}

#[cfg(feature = "catalog-publisher")]
fn read_signing_seed(path: &Path) -> Result<[u8; 32], String> {
    let bytes = read_bounded_regular(path, 32, "Ed25519 signing seed")?;
    if bytes.len() != 32 {
        return Err("Ed25519 signing seed must contain exactly 32 raw bytes".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(path)
            .map_err(|_| "cannot inspect signing seed permissions")?
            .permissions()
            .mode();
        if mode & 0o077 != 0 {
            return Err("signing seed must not be accessible by group or others".into());
        }
    }
    let mut seed = [0; 32];
    seed.copy_from_slice(&bytes);
    Ok(seed)
}

#[cfg(feature = "catalog-publisher")]
fn validate_publishable_cartridge(wasm: &[u8]) -> Result<CartridgeManifest, String> {
    let manifest = CartridgeManifest::from_wasm(wasm).map_err(|error| error.message())?;
    WasmModule::from_bytes_with(
        wasm,
        Limits {
            max_table_elems: 1_024,
            max_memory_pages: 64,
            max_steps: 1_000_000,
            ..Limits::default()
        },
    )
    .map_err(|error| error.message())?;
    let vm_limits = Limits {
        max_table_elems: 1_024,
        max_memory_pages: 64,
        max_steps: 1_000_000,
        ..Limits::default()
    };
    let game_limits = GameLimits {
        max_render_bytes: 64 * 1024,
        max_audio_bytes: 16 * 1024,
        max_state_bytes: 256 * 1024,
    };
    let mut first = GameRuntime::from_private_bytes(wasm, vm_limits, game_limits, 0x5441_4331)
        .map_err(|error| error.message())?;
    let frame = first
        .tick(GameInput {
            buttons: 0,
            clock_ms: 0,
        })
        .map_err(|error| error.message())?;
    validate_media(&frame.render, &frame.audio).map_err(|error| error.message())?;
    let snapshot = first.suspend().map_err(|error| error.message())?;
    let expected = first
        .tick(GameInput {
            buttons: 0,
            clock_ms: 16,
        })
        .map_err(|error| error.message())?;
    let mut restored = GameRuntime::from_private_bytes(wasm, vm_limits, game_limits, 0x5441_4331)
        .map_err(|error| error.message())?;
    restored
        .resume(&snapshot)
        .map_err(|error| error.message())?;
    let replay = restored
        .tick(GameInput {
            buttons: 0,
            clock_ms: 16,
        })
        .map_err(|error| error.message())?;
    validate_media(&replay.render, &replay.audio).map_err(|error| error.message())?;
    if expected.render != replay.render || expected.audio != replay.audio {
        return Err("suspend/resume replay is not byte-deterministic".into());
    }
    Ok(manifest)
}

#[cfg(feature = "catalog-publisher")]
fn validate_display_metadata(game: &SourceGame) -> Result<(), String> {
    if !valid_text(&game.title, 256)
        || !valid_text(&game.summary, 1024)
        || game.localizations.len() > 16
        || game.localizations.iter().any(|(tag, value)| {
            !valid_language_tag(tag)
                || !valid_text(&value.title, 256)
                || !valid_text(&value.summary, 1024)
        })
    {
        return Err("invalid game display metadata".into());
    }
    let mut folded = HashSet::new();
    if game
        .localizations
        .keys()
        .any(|tag| !folded.insert(tag.to_ascii_lowercase()))
    {
        return Err("duplicate case-insensitive localization tag".into());
    }
    Ok(())
}

#[cfg(feature = "catalog-publisher")]
fn valid_identifier(value: &str, maximum: usize) -> bool {
    !value.is_empty()
        && value.len() <= maximum
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
        })
}

#[cfg(feature = "catalog-publisher")]
fn valid_text(value: &str, maximum: usize) -> bool {
    !value.trim().is_empty() && value.len() <= maximum
}

#[cfg(feature = "catalog-publisher")]
fn valid_version(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'+' | b'-'))
}

#[cfg(feature = "catalog-publisher")]
fn valid_language_tag(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 35
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

#[cfg(feature = "catalog-publisher")]
fn lower_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut result = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        result.push(char::from(HEX[usize::from(byte >> 4)]));
        result.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    result
}

fn read_cartridge(path: &str) -> Result<Vec<u8>, &'static str> {
    let metadata = std::fs::symlink_metadata(path).map_err(|_| "cannot stat cartridge")?;
    if !metadata.file_type().is_file() || metadata.len() == 0 {
        return Err("cartridge is not a non-empty regular file");
    }
    if metadata.len() > MAX_CARTRIDGE_BYTES {
        return Err("cartridge exceeds 2 MiB converter limit");
    }
    std::fs::read(path).map_err(|_| "cannot read cartridge")
}

fn run_attach_manifest(
    input_path: &str,
    output_path: &str,
    game_id: String,
    game_version: String,
    abi_version: &str,
    state_version: &str,
) -> ExitCode {
    let result: Result<(CartridgeManifest, usize), String> = (|| {
        let wasm = read_cartridge(input_path).map_err(str::to_string)?;
        let limits = Limits {
            max_table_elems: 1_024,
            max_memory_pages: 64,
            max_steps: 1_000_000,
            ..Limits::default()
        };
        let module = WasmModule::from_bytes_with(&wasm, limits)
            .map_err(|error| error.message().to_string())?;
        let mut capabilities = Vec::new();
        for import in module.imports() {
            if import.module != "tinyarcade:core/v1" && !capabilities.contains(&import.module) {
                if capabilities.len() == 64 {
                    return Err("more than 64 native capability namespaces".into());
                }
                capabilities.push(import.module.clone());
            }
        }
        capabilities.sort();
        let manifest = CartridgeManifest {
            game_id,
            game_version,
            abi_version: abi_version
                .parse()
                .map_err(|_| "ABI version must be a decimal u32")?,
            state_version: state_version
                .parse()
                .map_err(|_| "state version must be a decimal u32")?,
            capabilities,
        };
        let cartridge = manifest
            .append_to_wasm(&wasm)
            .map_err(|error| error.message().to_string())?;
        if cartridge.len() > MAX_CARTRIDGE_BYTES as usize {
            return Err("manifested cartridge exceeds 2 MiB converter limit".into());
        }
        let descriptor = CartridgeDescriptor::inspect(&cartridge, limits)
            .map_err(|error| error.message().to_string())?;
        publish_new_file(
            Path::new(output_path),
            &cartridge,
            "manifested cartridge output",
        )?;
        Ok((descriptor.manifest, cartridge.len()))
    })();
    match result {
        Ok((manifest, bytes)) => {
            println!("game_id={}", manifest.game_id);
            println!("game_version={}", manifest.game_version);
            println!("abi_version={}", manifest.abi_version);
            println!("state_version={}", manifest.state_version);
            println!("wasm_bytes={bytes}");
            if manifest.capabilities.is_empty() {
                println!("native_capabilities=(none)");
            } else {
                println!("native_capabilities={}", manifest.capabilities.join(","));
            }
            println!("OK: attached canonical manifest to standard WASM module");
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("tinyvm: {message}");
            ExitCode::FAILURE
        }
    }
}

fn read_host_profile(path: &str) -> Result<Vec<u8>, String> {
    read_bounded_regular(
        Path::new(path),
        MAX_HOST_PROFILE_BYTES as u64,
        "host profile",
    )
}

fn print_host_profile(profile: &HostProfileV1, bytes: usize) {
    let vm = profile.vm_limits();
    let game = profile.game_limits();
    println!("schema=tinyarcade-host-profile-v1");
    println!("profile_bytes={bytes}");
    println!("game_abi_version=1");
    println!("max_cartridge_bytes={MAX_CARTRIDGE_BYTES}");
    println!("max_table_elems={}", vm.max_table_elems);
    println!("max_memory_pages={}", vm.max_memory_pages);
    println!("max_steps={}", vm.max_steps);
    println!("max_call_depth={}", vm.max_call_depth);
    println!("max_activation_slots={}", vm.max_activation_slots);
    println!("max_render_bytes={}", game.max_render_bytes);
    println!("max_audio_bytes={}", game.max_audio_bytes);
    println!("max_state_bytes={}", game.max_state_bytes);
    println!("media=tinyarcade:grid3d/v1,tinyarcade:indexed2d/v1,tinyarcade:tones/v1");
    println!("native_functions={}", profile.native_functions().len());
    for function in profile.native_functions() {
        println!(
            "native={}.{} params={} results={} max_calls={}",
            function.module,
            function.field,
            function.n_params,
            function.n_results,
            function.max_calls_per_lifecycle
        );
    }
}

fn run_host_profile_default(path: &str) -> ExitCode {
    let result: Result<(HostProfileV1, Vec<u8>), String> = (|| {
        let profile = HostProfileV1::new(Limits::default(), GameLimits::default())
            .map_err(|error| error.message().to_string())?;
        let bytes = profile
            .encode()
            .map_err(|error| error.message().to_string())?;
        publish_new_file(Path::new(path), &bytes, "host profile output")?;
        Ok((profile, bytes))
    })();
    match result {
        Ok((profile, bytes)) => {
            print_host_profile(&profile, bytes.len());
            println!("OK: wrote canonical core-only TinyArcade host profile");
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("tinyvm: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run_host_profile_inspect(path: &str) -> ExitCode {
    let result: Result<(HostProfileV1, usize), String> = (|| {
        let bytes = read_host_profile(path)?;
        let profile = HostProfileV1::decode(&bytes).map_err(|error| error.message().to_string())?;
        Ok((profile, bytes.len()))
    })();
    match result {
        Ok((profile, bytes)) => {
            print_host_profile(&profile, bytes);
            println!("OK: canonical TinyArcade host profile");
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("tinyvm: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run_cartridge_profile(cartridge_path: &str, profile_path: &str) -> ExitCode {
    let result: Result<(CartridgeDescriptor, usize), String> = (|| {
        let wasm = read_cartridge(cartridge_path).map_err(str::to_string)?;
        let profile_bytes = read_host_profile(profile_path)?;
        let profile =
            HostProfileV1::decode(&profile_bytes).map_err(|error| error.message().to_string())?;
        let descriptor = profile
            .inspect_cartridge(&wasm)
            .map_err(|error| error.message().to_string())?;
        Ok((descriptor, profile_bytes.len()))
    })();
    match result {
        Ok((descriptor, profile_bytes)) => {
            println!("game_id={}", descriptor.manifest.game_id);
            println!("game_version={}", descriptor.manifest.game_version);
            println!("profile_bytes={profile_bytes}");
            println!("function_imports={}", descriptor.imports.len());
            println!("OK: cartridge is statically compatible with exact host profile");
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("tinyvm: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run_cartridge(path: &str, execute: bool) -> ExitCode {
    let bytes = match read_cartridge(path) {
        Ok(bytes) => bytes,
        Err(message) => {
            eprintln!("tinyvm: {message}");
            return ExitCode::FAILURE;
        }
    };
    let descriptor = match CartridgeDescriptor::inspect(
        &bytes,
        Limits {
            max_table_elems: 1_024,
            max_memory_pages: 64,
            max_steps: 1_000_000,
            ..Limits::default()
        },
    ) {
        Ok(descriptor) => descriptor,
        Err(error) => return cartridge_error(error),
    };
    let manifest = &descriptor.manifest;
    println!("game_id={}", manifest.game_id);
    println!("game_version={}", manifest.game_version);
    println!("abi_version={}", manifest.abi_version);
    println!("state_version={}", manifest.state_version);
    println!("wasm_bytes={}", bytes.len());
    if !manifest.capabilities.is_empty() {
        println!("native_capabilities={}", manifest.capabilities.join(","));
    } else {
        println!("native_capabilities=(none)");
    }
    println!("function_imports={}", descriptor.imports.len());
    for import in &descriptor.imports {
        let class = if import.module == "tinyarcade:core/v1" {
            "core"
        } else {
            "native"
        };
        println!(
            "import={}.{} class={class} params={} results={} i32_only={}",
            import.module, import.field, import.n_params, import.n_results, import.i32_only
        );
    }
    if !execute {
        println!("OK: canonical TinyArcade manifest and parseable WASM module");
        return ExitCode::SUCCESS;
    }

    let vm_limits = Limits {
        max_table_elems: 1_024,
        max_memory_pages: 64,
        max_steps: 1_000_000,
        ..Limits::default()
    };
    let game_limits = GameLimits {
        max_render_bytes: 64 * 1024,
        max_audio_bytes: 16 * 1024,
        max_state_bytes: 256 * 1024,
    };
    let mut first =
        match GameRuntime::from_private_bytes(&bytes, vm_limits, game_limits, 0x5441_4331) {
            Ok(runtime) => runtime,
            Err(error) => return cartridge_error(error),
        };
    let initial = match first.tick(GameInput {
        buttons: 0,
        clock_ms: 0,
    }) {
        Ok(frame) => frame,
        Err(error) => return cartridge_error(error),
    };
    let initial_render_stream = match validate_media(&initial.render, &initial.audio) {
        Ok(stream) => stream,
        Err(error) => return cartridge_error(error),
    };
    let snapshot = match first.suspend() {
        Ok(snapshot) => snapshot,
        Err(error) => return cartridge_error(error),
    };
    let expected = match first.tick(GameInput {
        buttons: 0,
        clock_ms: 16,
    }) {
        Ok(frame) => frame,
        Err(error) => return cartridge_error(error),
    };
    let mut restored =
        match GameRuntime::from_private_bytes(&bytes, vm_limits, game_limits, 0x5441_4331) {
            Ok(runtime) => runtime,
            Err(error) => return cartridge_error(error),
        };
    if let Err(error) = restored.resume(&snapshot) {
        return cartridge_error(error);
    }
    let replay = match restored.tick(GameInput {
        buttons: 0,
        clock_ms: 16,
    }) {
        Ok(frame) => frame,
        Err(error) => return cartridge_error(error),
    };
    if let Err(error) = validate_media(&replay.render, &replay.audio) {
        return cartridge_error(error);
    }
    if expected.render != replay.render || expected.audio != replay.audio {
        eprintln!("tinyvm: suspend/resume replay is not byte-deterministic");
        return ExitCode::FAILURE;
    }
    println!("render_stream={initial_render_stream}");
    println!("initial_render_bytes={}", initial.render.len());
    println!("initial_audio_bytes={}", initial.audio.len());
    println!("snapshot_bytes={}", snapshot.len());
    println!("OK: private-import converter conformance v1");
    ExitCode::SUCCESS
}

fn validate_media(render: &[u8], audio: &[u8]) -> Result<&'static str, WasmError> {
    let stream = match RenderFrame::decode(render)? {
        RenderFrame::Grid3d(_) => "tinyarcade:grid3d/v1",
        RenderFrame::Indexed2d(_) => "tinyarcade:indexed2d/v1",
    };
    if !audio.is_empty() {
        ToneBatch::decode(audio)?;
    }
    Ok(stream)
}

fn cartridge_error(error: WasmError) -> ExitCode {
    eprintln!("tinyvm: {}", error.message());
    ExitCode::FAILURE
}

fn run_eval(src: &str) -> ExitCode {
    let mut vm = Vm::new(MEM_CELLS);
    match vm.eval(src) {
        Ok(Some(top)) => {
            println!("{top}");
            ExitCode::SUCCESS
        }
        Ok(None) => {
            println!("(empty)");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("tinyvm: {}", e.message());
            ExitCode::FAILURE
        }
    }
}

#[cfg(all(test, feature = "catalog-publisher"))]
mod publisher_tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn publisher_is_reproducible_atomic_and_does_not_emit_the_seed() {
        let temp = tempfile::tempdir().expect("temporary publisher directory");
        let wasm = temp.path().join("game.wasm");
        let status = Command::new(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("build-paddle-guard-cartridge.sh"),
        )
        .arg(&wasm)
        .status()
        .expect("run cartridge builder");
        assert!(status.success());

        let seed = temp.path().join("catalog.seed");
        let secret = [0x5au8; 32];
        std::fs::write(&seed, secret).expect("write seed");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&seed, std::fs::Permissions::from_mode(0o600))
                .expect("restrict seed");
        }
        let source = temp.path().join("source.json");
        let profile_path = temp.path().join("ios-build.tahost");
        let profile = HostProfileV1::new(Limits::default(), GameLimits::default())
            .unwrap_or_else(|error| panic!("default host profile: {}", error.message()))
            .encode()
            .unwrap_or_else(|error| panic!("encode host profile: {}", error.message()));
        std::fs::write(&profile_path, &profile).expect("write host profile");
        std::fs::write(
            &source,
            r#"{
              "schema_version": 1,
              "catalog_id": "tinyarcade.test",
              "signing_key_id": "test-2026",
              "host_profile": "ios-build.tahost",
              "games": [{
                "wasm": "game.wasm",
                "title": "Paddle Guard",
                "summary": "A bounded test cartridge.",
                "localizations": {"zh-Hans": {"title": "挡板守卫", "summary": "有边界的测试卡带。"}}
              }]
            }"#
            .as_bytes(),
        )
        .expect("write source");

        let first = temp.path().join("publish-one");
        let second = temp.path().join("publish-two");
        assert_eq!(build_catalog(&source, &seed, &first), Ok(1));
        assert_eq!(build_catalog(&source, &seed, &second), Ok(1));
        let first_json = std::fs::read(first.join("catalog-v1.json")).expect("read catalog");
        let second_json = std::fs::read(second.join("catalog-v1.json")).expect("read catalog");
        assert_eq!(first_json, second_json);
        assert!(
            !first_json
                .windows(secret.len())
                .any(|bytes| bytes == secret)
        );
        let wire: serde_json::Value = serde_json::from_slice(&first_json).expect("decode catalog");
        assert_eq!(wire["host_profile"]["file"], "host-profile-v1.tahost");
        assert_eq!(wire["host_profile"]["length"], profile.len());
        assert_eq!(
            wire["host_profile"]["sha256"]
                .as_str()
                .expect("profile hash"),
            lower_hex(&cartridge_sha256(&profile))
        );
        assert_eq!(
            std::fs::read(first.join("host-profile-v1.tahost")).expect("read staged profile"),
            profile
        );
        let game = &wire["games"][0];
        assert_eq!(game["game_id"], "com.partnernet.paddle-guard");
        assert_eq!(game["game_version"], "0.1.0");
        assert_eq!(game["cartridge"], "com.partnernet.paddle-guard-0.1.0.wasm");
        assert_eq!(game["wasm_sha256"].as_str().expect("hash").len(), 64);
        assert_eq!(
            BASE64
                .decode(game["signature"].as_str().expect("signature"))
                .expect("base64 signature")
                .len(),
            64
        );
        assert_eq!(
            std::fs::read(first.join("com.partnernet.paddle-guard-0.1.0.wasm"))
                .expect("read staged wasm"),
            std::fs::read(&wasm).expect("read source wasm")
        );

        let tight_profile = HostProfileV1::new(
            Limits {
                max_memory_pages: 1,
                ..Limits::default()
            },
            GameLimits::default(),
        )
        .unwrap_or_else(|error| panic!("tight host profile: {}", error.message()))
        .encode()
        .unwrap_or_else(|error| panic!("encode tight profile: {}", error.message()));
        std::fs::write(&profile_path, tight_profile).expect("replace with incompatible profile");
        let incompatible = temp.path().join("incompatible-publish");
        assert!(build_catalog(&source, &seed, &incompatible).is_err());
        assert!(!incompatible.exists());

        let failed = temp.path().join("failed-publish");
        std::fs::write(&source, b"{}").expect("replace with invalid source");
        assert!(build_catalog(&source, &seed, &failed).is_err());
        assert!(
            !failed.exists(),
            "failed publication must not become visible"
        );
    }
}
