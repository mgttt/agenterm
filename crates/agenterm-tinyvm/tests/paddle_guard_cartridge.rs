//! Black-box proof that a second, indexed-2D Rust cartridge is playable.

use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;

use agenterm_tinyvm::{
    GameFrame, GameInput, GameLimits, GameRuntime, Indexed2dFrame, Limits, ToneBatch, WasmError,
};
#[cfg(feature = "replay")]
use agenterm_tinyvm::{ReplayRecorderV1, ReplayTraceV1, cartridge_sha256};

const LEFT: u32 = 1 << 0;
const RIGHT: u32 = 1 << 1;
const PRIMARY: u32 = 1 << 4;

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
                crate_dir.join("../../target/tinyvm-paddle-guard-test/paddle-guard-0.1.0.wasm");
            let status = Command::new(crate_dir.join("build-paddle-guard-cartridge.sh"))
                .arg(&output)
                .status()
                .expect("run Paddle Guard cartridge builder");
            assert!(status.success(), "Paddle Guard cartridge build failed");
            let wasm = std::fs::read(output).expect("read Paddle Guard cartridge");
            assert!(wasm.len() < 16 * 1024, "cartridge grew unexpectedly");
            let checkout_path = env!("CARGO_MANIFEST_DIR").as_bytes();
            assert!(
                !wasm
                    .windows(checkout_path.len())
                    .any(|bytes| bytes == checkout_path),
                "published cartridge contains an absolute developer path"
            );
            let reproduction = crate_dir
                .join("../../target/tinyvm-paddle-guard-test/paddle-guard-reproduction.wasm");
            let second = Command::new(crate_dir.join("build-paddle-guard-cartridge.sh"))
                .arg(&reproduction)
                .status()
                .expect("repeat Paddle Guard cartridge builder");
            assert!(second.success(), "reproduction build failed");
            assert_eq!(
                wasm,
                std::fs::read(reproduction).expect("read reproduction cartridge"),
                "two independent cartridge builds must be byte-identical"
            );
            wasm
        })
        .clone()
}

fn runtime(wasm: &[u8]) -> GameRuntime {
    must_ok(
        GameRuntime::from_private_bytes(
            wasm,
            Limits {
                max_table_elems: 64,
                max_memory_pages: 17,
                max_steps: 500_000,
            },
            GameLimits {
                max_render_bytes: 20 * 1024,
                max_audio_bytes: 64,
                max_state_bytes: 128,
            },
            0x5041_4447,
        ),
        "load Paddle Guard cartridge",
    )
}

fn tick(runtime: &mut GameRuntime, buttons: u32, clock_ms: u32) -> GameFrame {
    must_ok(
        runtime.tick(GameInput { buttons, clock_ms }),
        "tick Paddle Guard",
    )
}

fn horizontal_span(frame: &Indexed2dFrame<'_>, color: u8, y: usize) -> Option<(usize, usize)> {
    let row = &frame.pixels()[y * usize::from(frame.width)..(y + 1) * usize::from(frame.width)];
    let first = row.iter().position(|pixel| *pixel == color)?;
    let last = row.iter().rposition(|pixel| *pixel == color)?;
    Some((first, last))
}

fn first_pixel(frame: &Indexed2dFrame<'_>, color: u8) -> Option<(usize, usize)> {
    let width = usize::from(frame.width);
    frame
        .pixels()
        .iter()
        .position(|pixel| *pixel == color)
        .map(|index| (index % width, index / width))
}

#[test]
fn standard_paddle_guard_launches_moves_and_emits_indexed_frames() {
    let wasm = build_cartridge();
    let mut game = runtime(&wasm);
    assert_eq!(game.manifest().game_id, "com.partnernet.paddle-guard");
    assert_eq!(game.manifest().game_version, "0.1.0");
    assert!(game.manifest().capabilities.is_empty());

    let initial = tick(&mut game, 0, 0);
    let initial_frame = must_ok(
        Indexed2dFrame::decode(&initial.render),
        "decode initial indexed frame",
    );
    assert_eq!((initial_frame.width, initial_frame.height), (160, 120));
    assert_eq!(initial_frame.palette_count(), 8);
    assert_eq!(initial_frame.pixels().len(), 19_200);
    assert_eq!(horizontal_span(&initial_frame, 2, 108), Some((68, 91)));
    assert!(initial_frame.pixels().contains(&3));
    assert!(initial_frame.pixels().iter().any(|pixel| *pixel >= 4));

    let moved = tick(&mut game, RIGHT, 50);
    let moved_frame = must_ok(
        Indexed2dFrame::decode(&moved.render),
        "decode moved indexed frame",
    );
    assert_eq!(horizontal_span(&moved_frame, 2, 108), Some((73, 96)));

    let launched = tick(&mut game, PRIMARY, 51);
    let tones = must_ok(ToneBatch::decode(&launched.audio), "decode launch tone");
    let launch_tone = must_ok(tones.events().next().expect("launch tone"), "launch tone");
    assert_eq!(launch_tone.kind, 1, "launch is generic impact feedback");
    let launch_frame = must_ok(
        Indexed2dFrame::decode(&launched.render),
        "decode launch frame",
    );
    let launch_ball = horizontal_span(&launch_frame, 3, 104).expect("docked launch ball");

    let _released = tick(&mut game, 0, 51);
    let advanced = tick(&mut game, 0, 551);
    let advanced_frame = must_ok(
        Indexed2dFrame::decode(&advanced.render),
        "decode advanced frame",
    );
    assert_ne!(
        horizontal_span(&advanced_frame, 3, 104),
        Some(launch_ball),
        "the launched spark must leave its dock"
    );
}

#[test]
fn paddle_guard_suspend_resume_replays_exact_frame_and_audio() {
    let wasm = build_cartridge();
    let mut first = runtime(&wasm);
    tick(&mut first, PRIMARY, 0);
    tick(&mut first, 0, 0);
    for frame in 1..=90u32 {
        let buttons = if frame % 40 < 20 { LEFT } else { RIGHT };
        tick(&mut first, buttons, frame * 16);
    }
    let snapshot = must_ok(first.suspend(), "suspend Paddle Guard");
    assert_eq!(snapshot.len(), 113);

    let clock_ms = 91 * 16;
    let expected = tick(&mut first, RIGHT, clock_ms);
    let mut restored = runtime(&wasm);
    must_ok(restored.resume(&snapshot), "resume Paddle Guard");
    let replay = tick(&mut restored, RIGHT, clock_ms);
    assert_eq!(expected.render, replay.render);
    assert_eq!(expected.audio, replay.audio);
}

#[cfg(feature = "replay")]
#[test]
fn paddle_guard_replay_covers_indexed_frames_and_tones() {
    let wasm = build_cartridge();
    let mut recorded = runtime(&wasm);
    let mut recorder = must_ok(
        ReplayRecorderV1::start(&wasm, &mut recorded),
        "start Paddle Guard replay",
    );
    let mut saw_audio = false;
    for (buttons, clock_ms) in [(PRIMARY, 0), (0, 0), (LEFT, 16), (RIGHT, 32)] {
        let frame = must_ok(
            recorder.record_tick(&mut recorded, GameInput { buttons, clock_ms }),
            "record Paddle Guard tick",
        );
        saw_audio |= !frame.audio.is_empty();
    }
    assert!(saw_audio, "the replay must cover a real tone output");
    let bytes = must_ok(recorder.finish(), "encode Paddle Guard replay");
    assert_eq!(bytes.len(), 529);
    assert_eq!(
        cartridge_sha256(&bytes),
        [
            0xaa, 0xd1, 0x26, 0x7f, 0x6a, 0x7d, 0xbc, 0x59, 0x11, 0x64, 0x7c, 0xa7, 0x71, 0xe1,
            0x4c, 0x19, 0x0f, 0x60, 0xa9, 0xa7, 0x47, 0x12, 0xda, 0xbb, 0x3a, 0x81, 0x8e, 0x6b,
            0xd3, 0x05, 0xe8, 0x87,
        ],
        "the checked-in input plan is the indexed-frame replay golden"
    );
    let trace = must_ok(ReplayTraceV1::decode(&bytes), "decode Paddle Guard replay");
    assert_eq!(trace.steps.len(), 4);
    assert!(trace.steps.iter().any(|step| step.audio_length > 0));
    must_ok(
        trace.verify_cartridge(&wasm),
        "bind Paddle Guard replay cartridge",
    );
    let mut replayed = runtime(&wasm);
    must_ok(
        trace.replay(&wasm, &mut replayed, |_, frame| {
            let decoded = Indexed2dFrame::decode(&frame.render)?;
            assert_eq!((decoded.width, decoded.height), (160, 120));
            Ok(())
        }),
        "replay Paddle Guard",
    );
}

#[test]
fn paddle_guard_field_clear_rebuilds_within_the_production_step_budget() {
    let wasm = build_cartridge();
    let mut source = runtime(&wasm);
    tick(&mut source, 0, 0);
    let mut snapshot = must_ok(source.suspend(), "suspend clear fixture");
    let guest = snapshot.len() - 64;
    snapshot[guest + 5] = 1; // playing
    snapshot[guest + 6] = 3;
    snapshot[guest + 7] = 1;
    snapshot[guest + 12..guest + 16].copy_from_slice(&(10 * 256i32).to_le_bytes());
    snapshot[guest + 16..guest + 20].copy_from_slice(&(24 * 256i32).to_le_bytes());
    snapshot[guest + 20..guest + 24].copy_from_slice(&0i32.to_le_bytes());
    snapshot[guest + 24..guest + 28].copy_from_slice(&(-65 * 256i32).to_le_bytes());
    snapshot[guest + 28..guest + 40].fill(0);
    snapshot[guest + 40..guest + 48].copy_from_slice(&1u64.to_le_bytes());
    snapshot[guest + 48] = 1;

    let mut game = runtime(&wasm);
    must_ok(game.resume(&snapshot), "resume one-panel fixture");
    let cleared = tick(&mut game, 0, 50);
    let tones = must_ok(ToneBatch::decode(&cleared.audio), "decode field-clear tone");
    let tone = must_ok(
        tones.events().next().expect("field-clear tone"),
        "clear tone",
    );
    assert_eq!(tone.kind, 2, "field clear is generic success feedback");
    let next_snapshot = must_ok(game.suspend(), "suspend cleared field");
    let next_guest = next_snapshot.len() - 64;
    assert_eq!(next_snapshot[next_guest + 7], 2, "level must advance");
    assert_eq!(
        u64::from_le_bytes(
            next_snapshot[next_guest + 40..next_guest + 48]
                .try_into()
                .expect("brick bits"),
        ),
        (1u64 << 40) - 1,
        "the next panel field must be complete"
    );
}

#[test]
fn paddle_guard_primary_restarts_a_game_over_state() {
    let wasm = build_cartridge();
    let mut source = runtime(&wasm);
    tick(&mut source, 0, 0);
    let mut snapshot = must_ok(source.suspend(), "suspend game-over fixture");
    let guest = snapshot.len() - 64;
    snapshot[guest + 5] = 2;
    snapshot[guest + 6] = 0;
    snapshot[guest + 7] = 5;
    snapshot[guest + 36..guest + 40].copy_from_slice(&12_345u32.to_le_bytes());

    let mut game = runtime(&wasm);
    must_ok(game.resume(&snapshot), "resume game-over fixture");
    let restarted = tick(&mut game, PRIMARY, 1);
    let tones = must_ok(ToneBatch::decode(&restarted.audio), "decode restart tone");
    assert_eq!(
        must_ok(tones.events().next().expect("restart tone"), "restart tone").kind,
        2
    );
    let reset = must_ok(game.suspend(), "suspend restarted game");
    let reset_guest = reset.len() - 64;
    assert_eq!(reset[reset_guest + 5], 0);
    assert_eq!(reset[reset_guest + 6], 3);
    assert_eq!(reset[reset_guest + 7], 1);
    assert_eq!(
        u32::from_le_bytes(
            reset[reset_guest + 36..reset_guest + 40]
                .try_into()
                .expect("score"),
        ),
        0
    );
}

#[test]
fn paddle_guard_produces_collision_and_failure_feedback_without_host_gameplay() {
    let wasm = build_cartridge();
    let mut guarded = runtime(&wasm);
    tick(&mut guarded, PRIMARY, 0);
    let mut output = tick(&mut guarded, 0, 0);
    let mut last_ball_y = 104usize;
    let mut saw_rebound = false;
    for frame in 1..=1_000u32 {
        let decoded = must_ok(
            Indexed2dFrame::decode(&output.render),
            "decode guarded frame",
        );
        let (ball_x, _) = first_pixel(&decoded, 3).expect("guarded ball pixel");
        let (paddle_left, paddle_right) =
            horizontal_span(&decoded, 2, 108).expect("guarded paddle");
        let buttons = if ball_x < paddle_left + 6 {
            LEFT
        } else if ball_x > paddle_right - 6 {
            RIGHT
        } else {
            0
        };
        output = tick(&mut guarded, buttons, frame * 16);
        let next = must_ok(
            Indexed2dFrame::decode(&output.render),
            "decode next guarded frame",
        );
        let (_, ball_y) = first_pixel(&next, 3).expect("next guarded ball pixel");
        if last_ball_y >= 100 && ball_y < last_ball_y {
            saw_rebound = true;
            break;
        }
        last_ball_y = ball_y;
    }
    assert!(
        saw_rebound,
        "tracking the spark must produce a shield rebound"
    );

    let mut game = runtime(&wasm);
    tick(&mut game, PRIMARY, 0);
    tick(&mut game, 0, 0);
    let mut saw_impact = false;
    let mut saw_failure = false;
    for frame in 1..=2_000u32 {
        let output = tick(&mut game, LEFT, frame * 16);
        if !output.audio.is_empty() {
            let tones = must_ok(ToneBatch::decode(&output.audio), "decode gameplay tone");
            for event in tones.events() {
                match must_ok(event, "gameplay tone").kind {
                    1 => saw_impact = true,
                    3 => saw_failure = true,
                    _ => {}
                }
            }
        }
        if saw_impact && saw_failure {
            break;
        }
    }
    assert!(
        saw_impact,
        "ball must collide with a wall, panel, or shield"
    );
    assert!(
        saw_failure,
        "an unattended ball must eventually cost a life"
    );
}

#[test]
fn converter_cli_accepts_the_real_paddle_guard_cartridge() {
    let wasm = build_cartridge();
    let directory = tempfile::tempdir().expect("temporary converter fixture");
    let path = directory.path().join("paddle-guard.wasm");
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
    assert!(stdout.contains("game_id=com.partnernet.paddle-guard"));
    assert!(stdout.contains("render_stream=tinyarcade:indexed2d/v1"));
    assert!(stdout.contains("initial_render_bytes=19248"));
    assert!(stdout.contains("OK: private-import converter conformance v1"));
}
