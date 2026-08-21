//! macOS-owned build/link/run gate for the optional iOS WASI host artifact.

#[cfg(target_os = "macos")]
#[test]
fn ios_wasi_host_simulator_container() {
    use std::path::PathBuf;
    use std::process::Command;

    let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = crate_dir.join("../..");
    let status = Command::new(crate_dir.join("smoke-ios-wasi-host.sh"))
        .current_dir(&root)
        .env("CARGO", env!("CARGO"))
        .env(
            "CARGO_TARGET_DIR",
            root.join("target/tinyvm-ios-wasi-host-test"),
        )
        .status()
        .expect("run optional iOS WASI host smoke gate");
    assert!(status.success(), "iOS WASI host smoke failed");
}
