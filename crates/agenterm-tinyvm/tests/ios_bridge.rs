//! macOS-owned build/link gate for the delivered iOS bridge artifacts.

#[cfg(target_os = "macos")]
#[test]
fn ios_xcframework_swift_link() {
    use std::path::PathBuf;
    use std::process::Command;

    let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = crate_dir.join("../..");
    let status = Command::new(crate_dir.join("smoke-ios-bridge.sh"))
        .current_dir(&root)
        .env("CARGO", env!("CARGO"))
        .env("CARGO_TARGET_DIR", root.join("target/tinyarcade-ios-test"))
        .status()
        .expect("run iOS bridge smoke gate");
    assert!(status.success(), "iOS XCFramework/Swift smoke failed");
}
