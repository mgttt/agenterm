//! Independent C-toolchain proof for the fan cartridge authoring path.

use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;

use agenterm_tinyvm::{GameInput, GameLimits, GameRuntime, Indexed2dFrame, Limits, WasmError};

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
            let temp = tempfile::tempdir().expect("create C cartridge temp directory");
            let output = temp.path().join("fan-c-cartridge-0.1.0.wasm");
            let status = Command::new(crate_dir.join("build-fan-c-cartridge.sh"))
                .arg(&output)
                .env("TINYVM_BIN", env!("CARGO_BIN_EXE_tinyvm"))
                .status()
                .expect("run independent C cartridge builder");
            assert!(status.success(), "C cartridge build failed");
            let wasm = std::fs::read(output).expect("read C cartridge");
            assert!(wasm.len() < 2 * 1024, "C fixture grew unexpectedly");
            assert_eq!(&wasm[..8], b"\0asm\x01\0\0\0");
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
                max_memory_pages: 1,
                max_steps: 100_000,
                ..Limits::default()
            },
            GameLimits {
                max_render_bytes: 1024,
                max_audio_bytes: 0,
                max_state_bytes: 4,
            },
            7,
        ),
        "load C-authored cartridge",
    )
}

fn dot_x(frame: &Indexed2dFrame<'_>) -> usize {
    let row = 8 * 32;
    frame.pixels()[row..row + 32]
        .iter()
        .position(|&pixel| pixel == 2)
        .expect("visible C-authored dot")
}

fn metadata_dot_x(frame: &Indexed2dFrame<'_>) -> u32 {
    let bytes: [u8; 4] = frame.metadata().try_into().expect("C metadata is one u32");
    u32::from_le_bytes(bytes)
}

#[test]
fn ordinary_c_toolchain_emits_a_portable_standard_cartridge() {
    let wasm = build_cartridge();
    let mut game = runtime(&wasm);
    assert_eq!(game.manifest().game_id, "org.example.fan-c-cartridge");
    assert_eq!(game.manifest().game_version, "0.1.0");
    assert!(game.manifest().capabilities.is_empty());

    let initial = must_ok(game.tick(GameInput::default()), "tick C cartridge");
    let initial = must_ok(
        Indexed2dFrame::decode(&initial.render),
        "decode C-authored indexed frame",
    );
    assert_eq!((initial.width, initial.height), (32, 16));
    assert_eq!(initial.palette_count(), 3);
    assert_eq!(dot_x(&initial), 16);
    assert_eq!(initial.metadata_schema, Some(0x314e_4146));
    assert_eq!(metadata_dot_x(&initial), 16);

    let moved = must_ok(
        game.tick(GameInput {
            buttons: 1 << 1,
            clock_ms: 16,
        }),
        "move C-authored dot",
    );
    let moved = must_ok(Indexed2dFrame::decode(&moved.render), "decode moved frame");
    assert_eq!(dot_x(&moved), 17);
    assert_eq!(metadata_dot_x(&moved), 17);

    let snapshot = must_ok(game.suspend(), "suspend C cartridge");
    let mut restored = runtime(&wasm);
    must_ok(restored.resume(&snapshot), "resume C cartridge");
    let resumed = must_ok(
        restored.tick(GameInput {
            buttons: 0,
            clock_ms: 32,
        }),
        "tick restored C cartridge",
    );
    let resumed = must_ok(
        Indexed2dFrame::decode(&resumed.render),
        "decode restored C frame",
    );
    assert_eq!(dot_x(&resumed), 17);
    assert_eq!(metadata_dot_x(&resumed), 17);
}
