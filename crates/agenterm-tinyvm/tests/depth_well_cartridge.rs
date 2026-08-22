//! Black-box proof that a Rust-authored standard cartridge runs unchanged.

use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;

use agenterm_tinyvm::{
    CartridgeOrigin, GameInput, GameLimits, GameRuntime, Grid3dFrame, Limits, ToneBatch, WasmError,
};

#[cfg(feature = "catalog-publisher")]
use agenterm_tinyvm::HostProfileV1;
#[cfg(feature = "cartridge-trust")]
use agenterm_tinyvm::{CartridgeCache, CartridgeTrustStore, CatalogEntry, cartridge_sha256};
#[cfg(feature = "replay")]
use agenterm_tinyvm::{ReplayRecorderV1, ReplayTraceV1};
#[cfg(feature = "cartridge-trust")]
use ring::signature::{Ed25519KeyPair, KeyPair};

fn must_ok<T>(result: Result<T, WasmError>, context: &str) -> T {
    match result {
        Ok(value) => value,
        Err(error) => panic!("{context}: {}", error.message()),
    }
}

fn build_cartridge() -> Vec<u8> {
    static CARTRIDGE: OnceLock<Vec<u8>> = OnceLock::new();
    CARTRIDGE
        .get_or_init(|| {
            let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
            let output =
                crate_dir.join("../../target/tinyvm-depth-well-test/depth-well-0.1.0.wasm");
            let status = Command::new(crate_dir.join("build-depth-well-cartridge.sh"))
                .arg(&output)
                .status()
                .expect("run Depth Well cartridge builder");
            assert!(status.success(), "Depth Well cartridge build failed");
            let wasm = std::fs::read(output).expect("read built Depth Well cartridge");
            assert!(
                !wasm
                    .windows(b"/Users/".len())
                    .any(|bytes| bytes == b"/Users/"),
                "published cartridge contains an absolute developer path"
            );
            wasm
        })
        .clone()
}

fn runtime(wasm: &[u8]) -> GameRuntime {
    must_ok(
        GameRuntime::from_bytes(
            wasm,
            Limits {
                max_table_elems: 64,
                max_memory_pages: 17,
                max_steps: 100_000,
                ..Limits::default()
            },
            GameLimits {
                max_render_bytes: 4 * 1024,
                max_audio_bytes: 64,
                max_state_bytes: 512,
            },
            0x5eed_1234,
        ),
        "load Depth Well cartridge",
    )
}

#[test]
fn standard_depth_well_plays_and_restores_deterministically() {
    let wasm = build_cartridge();
    assert!(wasm.len() < 16 * 1024, "cartridge grew unexpectedly");
    let mut first = runtime(&wasm);
    assert!(matches!(first.origin(), CartridgeOrigin::Bundled));
    assert_eq!(first.manifest().game_id, "com.partnernet.depth-well");
    assert_eq!(first.manifest().game_version, "0.1.0");
    assert!(first.manifest().capabilities.is_empty());

    let initial = must_ok(
        first.tick(GameInput {
            buttons: 0,
            clock_ms: 0,
        }),
        "initial Depth Well frame",
    );
    let initial_grid = must_ok(Grid3dFrame::decode(&initial.render), "decode initial frame");
    assert_eq!(
        (initial_grid.width, initial_grid.depth, initial_grid.height),
        (5, 5, 10)
    );
    assert_eq!(
        initial_grid.cell_count(),
        8,
        "active piece plus landing ghost"
    );
    assert!(initial.audio.is_empty());

    let moved = must_ok(
        first.tick(GameInput {
            buttons: 1,
            clock_ms: 1,
        }),
        "move active piece",
    );
    let moved_grid = must_ok(Grid3dFrame::decode(&moved.render), "decode moved frame");
    let initial_active_min_x = initial_grid
        .cells()
        .filter_map(Result::ok)
        .filter(|cell| cell.kind == 2)
        .map(|cell| cell.x)
        .min()
        .expect("initial active cells");
    let moved_active_min_x = moved_grid
        .cells()
        .filter_map(Result::ok)
        .filter(|cell| cell.kind == 2)
        .map(|cell| cell.x)
        .min()
        .expect("moved active cells");
    assert_eq!(moved_active_min_x + 1, initial_active_min_x);

    let released = must_ok(
        first.tick(GameInput {
            buttons: 0,
            clock_ms: 2,
        }),
        "release movement input",
    );
    let snapshot = must_ok(first.suspend(), "suspend Depth Well");

    let dropped = must_ok(
        first.tick(GameInput {
            buttons: 1 << 7,
            clock_ms: 3,
        }),
        "hard drop Depth Well piece",
    );
    let dropped_grid = must_ok(Grid3dFrame::decode(&dropped.render), "decode dropped frame");
    assert!(dropped_grid.score >= 10);
    assert!(
        dropped_grid.cell_count() >= 12,
        "settled, active and ghost cells"
    );
    let tones = must_ok(ToneBatch::decode(&dropped.audio), "decode lock sound");
    assert_eq!(tones.event_count(), 1);

    let mut restored = runtime(&wasm);
    must_ok(restored.resume(&snapshot), "resume Depth Well");
    let replay = must_ok(
        restored.tick(GameInput {
            buttons: 1 << 7,
            clock_ms: 3,
        }),
        "replay hard drop after resume",
    );
    assert_eq!(replay.render, dropped.render);
    assert_eq!(replay.audio, dropped.audio);

    // Seed a nearly complete bottom deck through the public portable state
    // envelope, leaving exactly the current landing cells empty. This reaches
    // the compaction/scoring path without a fragile long input choreography.
    let released_grid = must_ok(
        Grid3dFrame::decode(&released.render),
        "decode released frame",
    );
    let holes: Vec<_> = released_grid
        .cells()
        .filter_map(Result::ok)
        .filter(|cell| cell.kind == 3 && cell.z == 0)
        .map(|cell| (cell.x, cell.y))
        .collect();
    assert_eq!(holes.len(), 4);
    let mut clear_ready = snapshot.clone();
    let id_len = u16::from_le_bytes([clear_ready[12], clear_ready[13]]) as usize;
    let guest = 4 + 4 + 4 + 2 + id_len + 4 + 4;
    for y in 0..5usize {
        for x in 0..5usize {
            clear_ready[guest + 4 + y * 5 + x] = u8::from(!holes.contains(&(x as u8, y as u8)));
        }
    }
    let mut clearer = runtime(&wasm);
    must_ok(clearer.resume(&clear_ready), "resume near-complete deck");
    let cleared = must_ok(
        clearer.tick(GameInput {
            buttons: 1 << 7,
            clock_ms: 3,
        }),
        "clear a complete deck",
    );
    let cleared_grid = must_ok(Grid3dFrame::decode(&cleared.render), "decode cleared frame");
    assert_eq!(cleared_grid.cleared_decks, 1);
    let clear_tones = must_ok(ToneBatch::decode(&cleared.audio), "decode clear sound");
    let clear_tone = must_ok(
        clear_tones.events().next().expect("clear event"),
        "decode clear event",
    );
    assert_eq!(clear_tone.kind, 2);
}

#[cfg(feature = "replay")]
#[test]
fn depth_well_replay_is_portable_bounded_and_tamper_evident() {
    let wasm = build_cartridge();
    let mut recorded = runtime(&wasm);
    let mut recorder = must_ok(
        ReplayRecorderV1::start(&wasm, &mut recorded),
        "start Depth Well replay",
    );
    for input in [
        GameInput {
            buttons: 0,
            clock_ms: 0,
        },
        GameInput {
            buttons: 1,
            clock_ms: 16,
        },
        GameInput {
            buttons: 1 << 4,
            clock_ms: 32,
        },
        GameInput {
            buttons: 1 << 7,
            clock_ms: 48,
        },
    ] {
        must_ok(
            recorder.record_tick(&mut recorded, input),
            "record Depth Well tick",
        );
    }
    assert!(
        recorder
            .record_tick(
                &mut recorded,
                GameInput {
                    buttons: 0,
                    clock_ms: 47,
                },
            )
            .is_err()
    );
    let bytes = must_ok(recorder.finish(), "encode Depth Well replay");
    assert_eq!(bytes.len(), 749);
    assert_eq!(
        cartridge_sha256(&bytes),
        [
            0xc0, 0xc2, 0x69, 0x75, 0x00, 0x8b, 0xf2, 0x28, 0x8c, 0xce, 0x0a, 0x76, 0x0a, 0xa6,
            0x64, 0xc7, 0xef, 0x22, 0x3f, 0xc0, 0x41, 0xda, 0x06, 0x8a, 0xb8, 0x32, 0x47, 0x2b,
            0x0f, 0xaa, 0x4d, 0x64,
        ],
        "the checked-in input plan is the replay wire-format golden"
    );
    let trace = must_ok(ReplayTraceV1::decode(&bytes), "decode Depth Well replay");
    must_ok(
        trace.verify_cartridge(&wasm),
        "bind Depth Well replay cartridge",
    );
    assert_eq!(must_ok(trace.encode(), "re-encode replay"), bytes);
    let mut replayed = runtime(&wasm);
    let mut frames = 0;
    must_ok(
        trace.replay(&wasm, &mut replayed, |index, frame| {
            assert_eq!(index, frames);
            assert!(!frame.render.is_empty());
            frames += 1;
            Ok(())
        }),
        "replay Depth Well",
    );
    assert_eq!(frames, 4);

    let mut changed_wasm = wasm.clone();
    changed_wasm[0] ^= 0xff;
    assert!(trace.verify_cartridge(&changed_wasm).is_err());
    assert!(
        trace
            .replay(&changed_wasm, &mut runtime(&wasm), |_, _| Ok(()))
            .is_err(),
        "replay execution must enforce its own exact-cartridge binding"
    );
    let mut changed_trace = bytes;
    *changed_trace.last_mut().expect("replay byte") ^= 0xff;
    let changed = must_ok(
        ReplayTraceV1::decode(&changed_trace),
        "decode changed digest",
    );
    assert!(
        changed
            .replay(&wasm, &mut runtime(&wasm), |_, _| Ok(()))
            .is_err()
    );
    let mut same_manifest = wasm.clone();
    same_manifest.extend_from_slice(&[0, 1, 0]);
    let mut different_runtime = runtime(&same_manifest);
    assert!(
        ReplayRecorderV1::start(&wasm, &mut different_runtime).is_err(),
        "recording must not bind supplied bytes to a different loaded cartridge"
    );
}

#[cfg(feature = "replay")]
#[test]
fn replay_cli_records_checks_reproduces_and_never_overwrites() {
    let directory = tempfile::tempdir().expect("temporary replay fixture");
    let wasm_path = directory.path().join("depth-well.wasm");
    let first = directory.path().join("first.tareplay");
    let second = directory.path().join("second.tareplay");
    std::fs::write(&wasm_path, build_cartridge()).expect("write replay cartridge");
    let inputs = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/depth-well-replay-v1.inputs");

    for output in [&first, &second] {
        let result = Command::new(env!("CARGO_BIN_EXE_tinyvm"))
            .args(["replay", "record"])
            .arg(&wasm_path)
            .arg(&inputs)
            .arg(output)
            .output()
            .expect("record replay through CLI");
        assert!(
            result.status.success(),
            "record failed: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    let first_bytes = std::fs::read(&first).expect("read first replay");
    assert_eq!(
        first_bytes,
        std::fs::read(&second).expect("read reproduced replay")
    );
    assert_eq!(
        cartridge_sha256(&first_bytes),
        [
            0xc7, 0x73, 0x71, 0xd4, 0xde, 0xf9, 0xec, 0x49, 0x40, 0xb2, 0x48, 0x52, 0x73, 0x33,
            0x93, 0x16, 0xe1, 0xd1, 0x24, 0x2f, 0x05, 0x68, 0xa7, 0x55, 0x5f, 0x7a, 0xf1, 0x65,
            0x2e, 0x4e, 0x4c, 0x6f,
        ],
        "the CLI and checked-in input plan define a stable converter golden"
    );

    let checked = Command::new(env!("CARGO_BIN_EXE_tinyvm"))
        .args(["replay", "check"])
        .arg(&wasm_path)
        .arg(&first)
        .output()
        .expect("check replay through CLI");
    assert!(checked.status.success());
    assert!(String::from_utf8_lossy(&checked.stdout).contains("verified_frames=4"));

    let checked_json = Command::new(env!("CARGO_BIN_EXE_tinyvm"))
        .args(["replay", "check"])
        .arg(&wasm_path)
        .arg(&first)
        .arg("--json")
        .output()
        .expect("check replay JSON through CLI");
    assert!(checked_json.status.success());
    assert!(checked_json.stderr.is_empty());
    let wire: serde_json::Value =
        serde_json::from_slice(&checked_json.stdout).expect("decode replay conformance JSON");
    assert_eq!(wire["schema"], "tinyarcade-replay-conformance-report");
    assert_eq!(wire["schema_version"], 1);
    assert_eq!(wire["valid"], true);
    assert_eq!(wire["replay_valid"], true);
    assert_eq!(wire["cartridge_bound"], true);
    assert_eq!(wire["identity"]["game_id"], "com.partnernet.depth-well");
    assert_eq!(wire["cartridge"]["bytes"], 6_116);
    assert_eq!(wire["cartridge"]["sha256"].as_str().map(str::len), Some(64));
    assert_eq!(wire["trace"]["bytes"], first_bytes.len());
    assert_eq!(wire["trace"]["steps"], 4);
    assert_eq!(wire["evidence"]["verified_frames"], 4);
    assert_eq!(wire["evidence"]["first_clock_ms"], 0);
    assert_eq!(wire["evidence"]["final_clock_ms"], 48);
    assert!(
        wire["evidence"]["total_render_bytes"]
            .as_u64()
            .is_some_and(|n| n > 0)
    );
    assert_eq!(wire["error"], serde_json::Value::Null);
    assert_eq!(wire.as_object().expect("replay report object").len(), 11);
    assert_eq!(
        wire["identity"].as_object().expect("replay identity").len(),
        4
    );
    assert_eq!(
        wire["cartridge"]
            .as_object()
            .expect("cartridge artifact")
            .len(),
        2
    );
    assert_eq!(wire["trace"].as_object().expect("trace artifact").len(), 4);
    assert_eq!(wire["limits"].as_object().expect("replay limits").len(), 8);
    assert_eq!(
        wire["evidence"].as_object().expect("replay evidence").len(),
        5
    );

    let repeated_json = Command::new(env!("CARGO_BIN_EXE_tinyvm"))
        .args(["replay", "check"])
        .arg(&wasm_path)
        .arg(&first)
        .arg("--json")
        .output()
        .expect("repeat replay JSON check");
    assert!(repeated_json.status.success());
    assert_eq!(repeated_json.stdout, checked_json.stdout);
    assert!(repeated_json.stderr.is_empty());

    let missing_cartridge = Command::new(env!("CARGO_BIN_EXE_tinyvm"))
        .args(["replay", "check"])
        .arg(directory.path().join("missing.wasm"))
        .arg(&first)
        .arg("--json")
        .output()
        .expect("reject missing replay cartridge JSON");
    assert!(!missing_cartridge.status.success());
    assert!(missing_cartridge.stderr.is_empty());
    let missing_cartridge_wire: serde_json::Value =
        serde_json::from_slice(&missing_cartridge.stdout).expect("decode missing cartridge report");
    assert_eq!(missing_cartridge_wire["cartridge"], serde_json::Value::Null);
    assert_eq!(missing_cartridge_wire["trace"], serde_json::Value::Null);
    assert_eq!(missing_cartridge_wire["error"]["stage"], "cartridge_input");

    let missing_trace = Command::new(env!("CARGO_BIN_EXE_tinyvm"))
        .args(["replay", "check"])
        .arg(&wasm_path)
        .arg(directory.path().join("missing.tareplay"))
        .arg("--json")
        .output()
        .expect("reject missing replay trace JSON");
    assert!(!missing_trace.status.success());
    assert!(missing_trace.stderr.is_empty());
    let missing_trace_wire: serde_json::Value =
        serde_json::from_slice(&missing_trace.stdout).expect("decode missing trace report");
    assert_eq!(missing_trace_wire["cartridge"]["bytes"], 6_116);
    assert_eq!(missing_trace_wire["trace"], serde_json::Value::Null);
    assert_eq!(missing_trace_wire["error"]["stage"], "replay_input");

    let zero_path = directory.path().join("zero-frame.tareplay");
    let wasm_bytes = std::fs::read(&wasm_path).expect("read zero-frame replay cartridge");
    let mut zero_runtime = runtime(&wasm_bytes);
    let zero_recorder = must_ok(
        ReplayRecorderV1::start(&wasm_bytes, &mut zero_runtime),
        "start zero-frame replay",
    );
    std::fs::write(
        &zero_path,
        must_ok(zero_recorder.finish(), "finish zero-frame replay"),
    )
    .expect("write zero-frame replay");
    let zero = Command::new(env!("CARGO_BIN_EXE_tinyvm"))
        .args(["replay", "check"])
        .arg(&wasm_path)
        .arg(&zero_path)
        .arg("--json")
        .output()
        .expect("reject zero-frame replay JSON");
    assert!(!zero.status.success());
    assert!(zero.stderr.is_empty());
    let zero_wire: serde_json::Value =
        serde_json::from_slice(&zero.stdout).expect("decode zero-frame replay report");
    assert_eq!(zero_wire["replay_valid"], true);
    assert_eq!(zero_wire["cartridge_bound"], serde_json::Value::Null);
    assert_eq!(zero_wire["trace"]["steps"], 0);
    assert_eq!(zero_wire["error"]["stage"], "replay_coverage");

    let malformed_path = directory.path().join("malformed.tareplay");
    std::fs::write(&malformed_path, b"not a replay").expect("write malformed replay");
    let malformed = Command::new(env!("CARGO_BIN_EXE_tinyvm"))
        .args(["replay", "check"])
        .arg(&wasm_path)
        .arg(&malformed_path)
        .arg("--json")
        .output()
        .expect("reject malformed replay JSON");
    assert!(!malformed.status.success());
    assert!(malformed.stderr.is_empty());
    let malformed_wire: serde_json::Value =
        serde_json::from_slice(&malformed.stdout).expect("decode malformed replay report");
    assert_eq!(malformed_wire["valid"], false);
    assert_eq!(malformed_wire["replay_valid"], false);
    assert_eq!(malformed_wire["cartridge_bound"], serde_json::Value::Null);
    assert_eq!(malformed_wire["identity"], serde_json::Value::Null);
    assert_eq!(malformed_wire["trace"]["bytes"], 12);
    assert_eq!(malformed_wire["trace"]["steps"], serde_json::Value::Null);
    assert_eq!(malformed_wire["error"]["stage"], "replay_decode");

    let changed_wasm_path = directory.path().join("changed-depth-well.wasm");
    let mut changed_wasm = std::fs::read(&wasm_path).expect("read replay cartridge");
    changed_wasm.extend_from_slice(&[0, 1, 0]);
    std::fs::write(&changed_wasm_path, changed_wasm).expect("write changed cartridge");
    let mismatched = Command::new(env!("CARGO_BIN_EXE_tinyvm"))
        .args(["replay", "check"])
        .arg(&changed_wasm_path)
        .arg(&first)
        .arg("--json")
        .output()
        .expect("reject mismatched replay JSON");
    assert!(!mismatched.status.success());
    assert!(mismatched.stderr.is_empty());
    let mismatched_wire: serde_json::Value =
        serde_json::from_slice(&mismatched.stdout).expect("decode mismatched replay report");
    assert_eq!(mismatched_wire["replay_valid"], true);
    assert_eq!(mismatched_wire["cartridge_bound"], false);
    assert_eq!(
        mismatched_wire["identity"]["game_id"],
        "com.partnernet.depth-well"
    );
    assert_eq!(mismatched_wire["error"]["stage"], "cartridge_binding");

    let drifted_path = directory.path().join("drifted.tareplay");
    let mut drifted_bytes = first_bytes.clone();
    *drifted_bytes.last_mut().expect("replay digest byte") ^= 1;
    std::fs::write(&drifted_path, drifted_bytes).expect("write drifted replay");
    let drifted = Command::new(env!("CARGO_BIN_EXE_tinyvm"))
        .args(["replay", "check"])
        .arg(&wasm_path)
        .arg(&drifted_path)
        .arg("--json")
        .output()
        .expect("reject replay output drift JSON");
    assert!(!drifted.status.success());
    assert!(drifted.stderr.is_empty());
    let drifted_wire: serde_json::Value =
        serde_json::from_slice(&drifted.stdout).expect("decode replay drift report");
    assert_eq!(drifted_wire["replay_valid"], true);
    assert_eq!(drifted_wire["cartridge_bound"], true);
    assert_eq!(drifted_wire["evidence"], serde_json::Value::Null);
    assert_eq!(drifted_wire["error"]["stage"], "replay_execution");

    let overwrite = Command::new(env!("CARGO_BIN_EXE_tinyvm"))
        .args(["replay", "record"])
        .arg(&wasm_path)
        .arg(&inputs)
        .arg(&first)
        .output()
        .expect("reject replay overwrite");
    assert!(!overwrite.status.success());
    assert!(String::from_utf8_lossy(&overwrite.stderr).contains("already exists"));
}

#[cfg(feature = "catalog-publisher")]
#[test]
fn catalog_publisher_requires_exact_nonempty_representative_replay() {
    let directory = tempfile::tempdir().expect("temporary catalog replay fixture");
    let wasm = build_cartridge();
    let wasm_path = directory.path().join("depth-well.wasm");
    let replay_path = directory.path().join("depth-well.tareplay");
    let profile_path = directory.path().join("ios-build.tahost");
    let seed_path = directory.path().join("catalog.seed");
    let source_path = directory.path().join("source.json");
    std::fs::write(&wasm_path, &wasm).expect("write publisher cartridge");

    let mut replayed = runtime(&wasm);
    let mut recorder = must_ok(
        ReplayRecorderV1::start(&wasm, &mut replayed),
        "start publisher replay",
    );
    for input in [
        GameInput {
            buttons: 0,
            clock_ms: 0,
        },
        GameInput {
            buttons: 1,
            clock_ms: 16,
        },
        GameInput {
            buttons: 1 << 7,
            clock_ms: 32,
        },
    ] {
        must_ok(
            recorder.record_tick(&mut replayed, input),
            "record publisher replay",
        );
    }
    let replay = must_ok(recorder.finish(), "finish publisher replay");
    std::fs::write(&replay_path, &replay).expect("write publisher replay");

    let profile = must_ok(
        HostProfileV1::new(Limits::default(), GameLimits::default()),
        "create publisher host profile",
    );
    std::fs::write(
        &profile_path,
        must_ok(profile.encode(), "encode publisher host profile"),
    )
    .expect("write publisher host profile");
    std::fs::write(&seed_path, [0x5au8; 32]).expect("write publisher seed");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&seed_path, std::fs::Permissions::from_mode(0o600))
            .expect("restrict publisher seed");
    }
    std::fs::write(
        &source_path,
        serde_json::to_vec(&serde_json::json!({
            "schema_version": 2,
            "catalog_id": "tinyarcade.test",
            "signing_key_id": "test-2026",
            "host_profile": "ios-build.tahost",
            "games": [{
                "wasm": "depth-well.wasm",
                "replay": "depth-well.tareplay",
                "title": "Depth Well",
                "summary": "A representative replay gate."
            }]
        }))
        .expect("encode publisher source"),
    )
    .expect("write publisher source");

    let published = directory.path().join("published");
    let success = Command::new(env!("CARGO_BIN_EXE_tinyvm"))
        .args(["catalog", "build"])
        .arg(&source_path)
        .arg(&seed_path)
        .arg(&published)
        .output()
        .expect("publish catalog through CLI");
    assert!(
        success.status.success(),
        "publisher failed: {}",
        String::from_utf8_lossy(&success.stderr)
    );
    assert!(published.join("catalog-v1.json").is_file());
    assert!(
        !published.join("depth-well.tareplay").exists(),
        "review evidence must not become a runtime catalog object"
    );

    let mut drifted = replay;
    *drifted.last_mut().expect("replay digest byte") ^= 1;
    std::fs::write(&replay_path, drifted).expect("write drifted publisher replay");
    let rejected = directory.path().join("rejected");
    let failure = Command::new(env!("CARGO_BIN_EXE_tinyvm"))
        .args(["catalog", "build"])
        .arg(&source_path)
        .arg(&seed_path)
        .arg(&rejected)
        .output()
        .expect("reject catalog replay drift through CLI");
    assert!(!failure.status.success());
    assert!(
        String::from_utf8_lossy(&failure.stderr).contains("representative replay replay_execution")
    );
    assert!(
        !rejected.exists(),
        "failed replay gate must not publish a directory"
    );
    assert!(
        std::fs::read_dir(directory.path())
            .expect("enumerate publisher fixture")
            .filter_map(Result::ok)
            .filter_map(|entry| entry.file_name().into_string().ok())
            .all(|name| !name.starts_with(".rejected.tinyarcade-stage-")),
        "failed replay gate must remove its private staging directory"
    );
}

#[test]
fn converter_cli_accepts_the_real_depth_well_cartridge() {
    let wasm = build_cartridge();
    let directory = tempfile::tempdir().expect("temporary converter fixture");
    let path = directory.path().join("depth-well.wasm");
    std::fs::write(&path, wasm).expect("write converter fixture");
    let output = Command::new(env!("CARGO_BIN_EXE_tinyvm"))
        .args(["cartridge", "check"])
        .arg(&path)
        .output()
        .expect("run converter conformance command");
    assert!(
        output.status.success(),
        "converter failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("converter UTF-8 output");
    assert!(stdout.contains("game_id=com.partnernet.depth-well"));
    assert!(stdout.contains("render_stream=tinyarcade:grid3d/v1"));
    assert!(stdout.contains("OK: private-import converter conformance v1"));

    let json = Command::new(env!("CARGO_BIN_EXE_tinyvm"))
        .args(["cartridge", "check"])
        .arg(&path)
        .arg("--json")
        .output()
        .expect("run JSON converter conformance command");
    assert!(json.status.success());
    assert!(json.stderr.is_empty());
    let wire: serde_json::Value =
        serde_json::from_slice(&json.stdout).expect("decode dynamic conformance JSON");
    assert_eq!(wire["schema"], "tinyarcade-cartridge-conformance-report");
    assert_eq!(wire["schema_version"], 1);
    assert_eq!(wire["valid"], true);
    assert_eq!(wire["static_valid"], true);
    assert_eq!(wire["dynamic_valid"], true);
    assert_eq!(wire["deterministic"], true);
    assert_eq!(wire["cartridge"]["game_id"], "com.partnernet.depth-well");
    assert_eq!(wire["cartridge"]["wasm_bytes"], 6_116);
    assert_eq!(wire["evidence"]["render_stream"], "tinyarcade:grid3d/v1");
    assert_eq!(wire["evidence"]["initial_render_bytes"], 96);
    assert_eq!(
        wire["evidence"]["expected_render_bytes"],
        wire["evidence"]["replay_render_bytes"]
    );
    assert_eq!(
        wire["evidence"]["expected_audio_bytes"],
        wire["evidence"]["replay_audio_bytes"]
    );
    assert_eq!(
        wire["evidence"]["lifecycle_stats"]["initial_tick"]["render_bytes"],
        wire["evidence"]["initial_render_bytes"]
    );
    assert!(
        wire["evidence"]["lifecycle_stats"]["initial_tick"]["wasm_steps"]
            .as_u64()
            .is_some_and(|steps| steps > 0)
    );
    assert_eq!(wire["error"], serde_json::Value::Null);
    assert_eq!(wire.as_object().expect("dynamic report object").len(), 10);
    assert_eq!(
        wire["cartridge"]
            .as_object()
            .expect("dynamic cartridge object")
            .len(),
        8
    );
    assert_eq!(
        wire["limits"]
            .as_object()
            .expect("dynamic limits object")
            .len(),
        8
    );
    assert_eq!(
        wire["evidence"]
            .as_object()
            .expect("dynamic evidence object")
            .len(),
        11
    );
    assert_eq!(
        wire["evidence"]["lifecycle_stats"]
            .as_object()
            .expect("lifecycle stats object")
            .len(),
        7
    );
    assert_eq!(
        wire["evidence"]["lifecycle_stats"]["initial_tick"]
            .as_object()
            .expect("one lifecycle stats object")
            .len(),
        9
    );

    let repeated = Command::new(env!("CARGO_BIN_EXE_tinyvm"))
        .args(["cartridge", "check"])
        .arg(&path)
        .arg("--json")
        .output()
        .expect("repeat JSON converter conformance command");
    assert!(repeated.status.success());
    assert_eq!(repeated.stdout, json.stdout);
    assert!(repeated.stderr.is_empty());
}

#[cfg(feature = "cartridge-trust")]
#[test]
fn reviewed_depth_well_requires_exact_signed_bytes_and_honours_revocation() {
    let wasm = build_cartridge();
    let key_pair = Ed25519KeyPair::from_seed_unchecked(&[0x2a; 32]).expect("test signing key");
    let mut entry = CatalogEntry {
        game_id: "com.partnernet.depth-well".into(),
        game_version: "0.1.0".into(),
        abi_version: 1,
        state_version: 1,
        wasm_length: wasm.len() as u64,
        wasm_sha256: cartridge_sha256(&wasm),
        signing_key_id: "catalog-2026-a".into(),
        signature: [0; 64],
    };
    let signing_bytes = must_ok(entry.signing_bytes(), "encode signed catalog entry");
    entry
        .signature
        .copy_from_slice(key_pair.sign(&signing_bytes).as_ref());

    let mut trust = CartridgeTrustStore::new();
    must_ok(
        trust.add_key("catalog-2026-a", key_pair.public_key().as_ref()),
        "add catalog key",
    );
    let manifest = must_ok(trust.verify(&entry, &wasm), "verify reviewed cartridge");
    assert_eq!(manifest.game_id, entry.game_id);
    let reviewed = must_ok(
        GameRuntime::from_reviewed_bytes(
            &wasm,
            &entry,
            &trust,
            Limits {
                max_table_elems: 64,
                max_memory_pages: 17,
                max_steps: 100_000,
                ..Limits::default()
            },
            GameLimits::default(),
            7,
        ),
        "open reviewed runtime",
    );
    assert!(matches!(
        reviewed.origin(),
        CartridgeOrigin::OfficialReviewed
    ));

    let mut changed = wasm.clone();
    let last = changed.len() - 1;
    changed[last] ^= 1;
    assert!(trust.verify(&entry, &changed).is_err());

    trust.revoke_content(entry.wasm_sha256);
    assert!(trust.verify(&entry, &wasm).is_err());

    let mut rotated = CartridgeTrustStore::new();
    must_ok(
        rotated.add_key("catalog-2026-a", key_pair.public_key().as_ref()),
        "add key before revocation",
    );
    must_ok(rotated.revoke_key("catalog-2026-a"), "revoke catalog key");
    assert!(rotated.verify(&entry, &wasm).is_err());
}

#[cfg(feature = "cartridge-trust")]
#[test]
fn signed_cache_activation_and_rollback_reverify_current_trust() {
    let wasm = build_cartridge();
    let key_pair = Ed25519KeyPair::from_seed_unchecked(&[0x3b; 32]).expect("test signing key");
    let signed = |bytes: &[u8]| {
        let mut entry = CatalogEntry {
            game_id: "com.partnernet.depth-well".into(),
            game_version: "0.1.0".into(),
            abi_version: 1,
            state_version: 1,
            wasm_length: bytes.len() as u64,
            wasm_sha256: cartridge_sha256(bytes),
            signing_key_id: "catalog-cache-test".into(),
            signature: [0; 64],
        };
        let signing = must_ok(entry.signing_bytes(), "cache entry signing bytes");
        entry
            .signature
            .copy_from_slice(key_pair.sign(&signing).as_ref());
        entry
    };
    let v1 = signed(&wasm);
    // A standard unknown custom section makes a distinct valid generation
    // without changing the cartridge's declared semantic version.
    let mut wasm_v2 = wasm.clone();
    wasm_v2.extend_from_slice(&[0, 17, 16]);
    wasm_v2.extend_from_slice(b"cache-generation");
    let v2 = signed(&wasm_v2);

    let mut trust = CartridgeTrustStore::new();
    must_ok(
        trust.add_key("catalog-cache-test", key_pair.public_key().as_ref()),
        "add cache test key",
    );
    let directory = tempfile::tempdir().expect("temporary cartridge cache");
    let cache = must_ok(
        CartridgeCache::open(directory.path(), 16 * 1024),
        "open cache",
    );
    must_ok(cache.activate(&v1, &wasm, &trust), "activate v1");
    assert_eq!(
        must_ok(
            cache.load_active("com.partnernet.depth-well", &v1, &trust),
            "load v1",
        ),
        wasm
    );
    must_ok(cache.activate(&v2, &wasm_v2, &trust), "activate v2");
    assert_eq!(
        must_ok(
            cache.rollback("com.partnernet.depth-well", &v1, &trust),
            "rollback to v1",
        ),
        wasm
    );

    trust.revoke_content(v2.wasm_sha256);
    assert!(
        cache
            .rollback("com.partnernet.depth-well", &v2, &trust)
            .is_err(),
        "revoked previous generation must not reactivate"
    );
}
