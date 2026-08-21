//! Explicit cross-repository gate for the shipping iOS application consumer.

#[cfg(target_os = "macos")]
#[test]
#[ignore = "cross-repository real-App gate; run smoke-nostalgia-consumer.sh"]
fn current_main_runtime_runs_in_real_nostalgia_app_target() {
    use std::path::PathBuf;
    use std::process::Command;

    let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = crate_dir.join("../..");
    let status = Command::new(crate_dir.join("smoke-nostalgia-consumer.sh"))
        .current_dir(&root)
        .env("CARGO", env!("CARGO"))
        .status()
        .expect("run Nostalgia Arcade current-main consumer gate");
    assert!(status.success(), "real iOS App consumer gate failed");
}
