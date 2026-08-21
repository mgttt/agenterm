//! macOS-owned differential gate against WebKit's JavaScriptCore WASM engine.
//!
//! This is a development oracle, not an iOS product-runtime dependency.

#[cfg(target_os = "macos")]
#[test]
fn webkit_matches_tinyvm_replay() {
    use std::path::PathBuf;
    use std::process::Command;

    let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = crate_dir.join("../..");
    let status = Command::new(crate_dir.join("smoke-webkit-differential.sh"))
        .current_dir(&root)
        .env("CARGO", env!("CARGO"))
        .env(
            "CARGO_TARGET_DIR",
            root.join("target/tinyarcade-webkit-test"),
        )
        .status()
        .expect("run development-only WebKit differential gate");
    assert!(
        status.success(),
        "JavaScriptCore/tinyvm differential gate failed"
    );
}
