//! Black-box proof that the first migrated Swift game is a standard cartridge.

use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;

use agenterm_tinyvm::{
    GameFrame, GameInput, GameLimits, GameRuntime, Indexed2dFrame, Limits, ToneBatch, WasmError,
};
#[cfg(feature = "replay")]
use agenterm_tinyvm::{ReplayRecorderV1, ReplayTraceV1};

const LEFT: [u32; 3] = [1 << 0, 1 << 2, 1 << 4];
const RIGHT: [u32; 3] = [1 << 1, 1 << 3, 1 << 5];

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
                crate_dir.join("../../target/tinyvm-signal-lock-test/signal-lock-0.1.0.wasm");
            let status = Command::new(crate_dir.join("build-signal-lock-cartridge.sh"))
                .arg(&output)
                .status()
                .expect("run Signal Lock cartridge builder");
            assert!(status.success(), "Signal Lock cartridge build failed");
            let wasm = std::fs::read(output).expect("read Signal Lock cartridge");
            assert!(wasm.len() < 16 * 1024, "cartridge grew unexpectedly");
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
        GameRuntime::from_private_bytes(
            wasm,
            Limits {
                max_table_elems: 64,
                max_memory_pages: 17,
                max_steps: 500_000,
                ..Limits::default()
            },
            GameLimits {
                max_render_bytes: 20 * 1024,
                max_audio_bytes: 64,
                max_state_bytes: 128,
            },
            0x51_6e_a1,
        ),
        "load Signal Lock cartridge",
    )
}

fn tick(runtime: &mut GameRuntime, buttons: u32, clock_ms: u32) -> GameFrame {
    must_ok(
        runtime.tick(GameInput { buttons, clock_ms }),
        "tick Signal Lock",
    )
}

fn snapshot(runtime: &mut GameRuntime) -> Vec<u8> {
    must_ok(runtime.suspend(), "suspend Signal Lock")
}

fn guest(snapshot: &[u8]) -> &[u8] {
    &snapshot[snapshot.len() - 64..]
}

fn u16_at(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

#[test]
fn standard_signal_lock_rotates_channels_and_renders_a_readable_radar() {
    let wasm = build_cartridge();
    let mut game = runtime(&wasm);
    assert_eq!(game.manifest().game_id, "com.partnernet.signal-lock");
    assert_eq!(game.manifest().game_version, "0.1.0");
    assert!(game.manifest().capabilities.is_empty());

    let initial = tick(&mut game, 0, 0);
    let frame = must_ok(
        Indexed2dFrame::decode(&initial.render),
        "decode radar frame",
    );
    assert_eq!((frame.width, frame.height), (160, 120));
    assert_eq!(frame.palette_count(), 8);
    assert!(frame.pixels().contains(&2), "current bearings are visible");
    assert!(frame.pixels().contains(&3), "route targets are visible");
    assert!(frame.pixels().contains(&7), "forecast path is visible");

    let before = snapshot(&mut game);
    let old_outer = guest(&before)[8];
    let moved = tick(&mut game, RIGHT[0], 0);
    let tone = must_ok(ToneBatch::decode(&moved.audio), "decode movement tone");
    assert_eq!(
        must_ok(
            tone.events().next().expect("movement event"),
            "movement event"
        )
        .kind,
        1
    );
    let after = snapshot(&mut game);
    assert_eq!(guest(&after)[8], (old_outer + 1) % 8);
    assert_eq!(u16_at(guest(&after), 16), 1);
}

#[test]
fn signal_lock_shortest_route_scores_and_advances_the_forecast() {
    let wasm = build_cartridge();
    let mut game = runtime(&wasm);
    tick(&mut game, 0, 0);
    let opening = snapshot(&mut game);
    let state = guest(&opening);
    let old_route = [state[11], state[12], state[13]];
    let mut final_frame = None;

    for ring in 0..3 {
        let state = snapshot(&mut game);
        let state = guest(&state);
        let current = state[8 + ring];
        let target = state[11 + ring];
        let clockwise = (target + 8 - current) % 8;
        let counterclockwise = (current + 8 - target) % 8;
        let (button, distance) = if clockwise <= counterclockwise {
            (RIGHT[ring], clockwise)
        } else {
            (LEFT[ring], counterclockwise)
        };
        for _ in 0..distance {
            final_frame = Some(tick(&mut game, button, 0));
            tick(&mut game, 0, 0);
        }
    }

    let completed = snapshot(&mut game);
    let state = guest(&completed);
    assert_eq!(u16_at(state, 18), 2);
    assert_eq!(u32_at(state, 20), 399);
    assert_eq!(state[6], 1, "perfect route starts the chain");
    assert_eq!(&state[8..11], &old_route);
    assert_eq!(&state[11..13], &old_route[1..3]);
    let final_frame = final_frame.expect("at least one turn");
    let tones = must_ok(
        ToneBatch::decode(&final_frame.audio),
        "decode route-lock tone",
    );
    assert_eq!(
        must_ok(
            tones.events().next().expect("success event"),
            "success event"
        )
        .kind,
        2
    );
}

#[test]
fn signal_lock_three_expired_sweeps_end_the_run() {
    let wasm = build_cartridge();
    let mut game = runtime(&wasm);
    tick(&mut game, 0, 0);
    let mut clock = 0;
    let mut last = None;
    for _ in 0..(48 * 3) {
        clock += 250;
        last = Some(tick(&mut game, 0, clock));
    }
    let ended = snapshot(&mut game);
    let state = guest(&ended);
    assert_eq!(state[5], 0);
    assert_eq!(state[7], 1);
    assert_eq!(state[14], 0);
    let last = last.expect("last sweep");
    let tones = must_ok(ToneBatch::decode(&last.audio), "decode failure tone");
    assert_eq!(
        must_ok(
            tones.events().next().expect("failure event"),
            "failure event"
        )
        .kind,
        3
    );
}

#[test]
fn signal_lock_suspend_resume_replays_exact_frame_and_audio() {
    let wasm = build_cartridge();
    let mut first = runtime(&wasm);
    tick(&mut first, RIGHT[0], 0);
    tick(&mut first, 0, 0);
    tick(&mut first, LEFT[1], 16);
    tick(&mut first, 0, 16);
    let saved = snapshot(&mut first);
    assert_eq!(saved.len(), 112);
    let expected = tick(&mut first, RIGHT[2], 266);

    let mut restored = runtime(&wasm);
    must_ok(restored.resume(&saved), "resume Signal Lock");
    let replay = tick(&mut restored, RIGHT[2], 266);
    assert_eq!(replay.render, expected.render);
    assert_eq!(replay.audio, expected.audio);
}

#[cfg(feature = "replay")]
#[test]
fn signal_lock_replay_is_portable_across_tinyvm_and_browser_oracles() {
    let wasm = build_cartridge();
    let mut recorded = runtime(&wasm);
    let mut recorder = must_ok(
        ReplayRecorderV1::start(&wasm, &mut recorded),
        "start Signal Lock replay",
    );
    for (buttons, clock_ms) in [
        (RIGHT[0], 0),
        (0, 0),
        (LEFT[1], 16),
        (0, 32),
        (RIGHT[2], 32),
        (0, 282),
    ] {
        must_ok(
            recorder.record_tick(&mut recorded, GameInput { buttons, clock_ms }),
            "record Signal Lock tick",
        );
    }
    let bytes = must_ok(recorder.finish(), "encode Signal Lock replay");
    let trace = must_ok(ReplayTraceV1::decode(&bytes), "decode Signal Lock replay");
    assert_eq!(trace.steps.len(), 6);
    assert!(trace.steps.iter().any(|step| step.audio_length > 0));
    let mut replayed = runtime(&wasm);
    must_ok(
        trace.replay(&wasm, &mut replayed, |_, frame| {
            Indexed2dFrame::decode(&frame.render).map(|_| ())
        }),
        "replay Signal Lock",
    );
}
