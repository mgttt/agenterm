//! Black-box proof that a Rust-authored standard cartridge runs unchanged.

use std::path::PathBuf;
use std::process::Command;

use agenterm_tinyvm::{
    GameInput, GameLimits, GameRuntime, Grid3dFrame, Limits, ToneBatch, WasmError,
};

fn must_ok<T>(result: Result<T, WasmError>, context: &str) -> T {
    match result {
        Ok(value) => value,
        Err(error) => panic!("{context}: {}", error.message()),
    }
}

fn build_cartridge() -> Vec<u8> {
    let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let output = crate_dir.join("../../target/tinyvm-depth-well-test/depth-well-0.1.0.wasm");
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
}

fn runtime(wasm: &[u8]) -> GameRuntime {
    must_ok(
        GameRuntime::from_bytes(
            wasm,
            Limits {
                max_table_elems: 64,
                max_memory_pages: 17,
                max_steps: 100_000,
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
