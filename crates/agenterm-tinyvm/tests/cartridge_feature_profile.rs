//! Development gate for proposal priority derived from real cartridges.

#[cfg(target_family = "unix")]
#[test]
#[ignore = "run through smoke-cartridge-feature-profile.sh with production-built cartridges"]
fn real_cartridge_workload_prioritizes_standard_features() {
    use std::path::PathBuf;
    use std::process::Command;

    let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = crate_dir.join("../..");
    let status = Command::new(crate_dir.join("smoke-cartridge-feature-profile.sh"))
        .current_dir(&root)
        .env("CARGO", env!("CARGO"))
        .env(
            "CARGO_TARGET_DIR",
            root.join("target/tinyvm-cartridge-feature-profile"),
        )
        .status()
        .expect("run real-cartridge feature profile");
    assert!(status.success(), "real-cartridge feature profile failed");
}
