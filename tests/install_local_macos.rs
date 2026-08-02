#![cfg(target_os = "macos")]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

fn write_executable(path: &Path, body: &str) {
    fs::write(path, body).expect("write fixture executable");
    let mut permissions = fs::metadata(path).expect("fixture metadata").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).expect("make fixture executable");
}

#[test]
fn local_build_installs_a_dock_safe_app_bundle() {
    let unique = format!(
        "agenterm-local-install-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos()
    );
    let root = std::env::temp_dir().join(unique);
    let binaries = root.join("build");
    let install = root.join("install");
    let bin = root.join("bin");
    let applications = root.join("applications");
    fs::create_dir_all(&binaries).expect("create fixture build");

    for name in [
        "agenterm",
        "agenterm-cli",
        "agenterm-mux",
        "agenterm-script",
        "agenterm-mcp",
    ] {
        let body = if name == "agenterm-cli" {
            format!("#!/bin/sh\necho 'agenterm-cli {CURRENT_VERSION}'\n")
        } else {
            "#!/bin/sh\nexit 0\n".to_owned()
        };
        write_executable(&binaries.join(name), &body);
    }

    let output = Command::new("bash")
        .arg("install.sh")
        .arg("--local-build")
        .arg(&binaries)
        .env("AGENTERM_INSTALL_DIR", &install)
        .env("AGENTERM_BIN_DIR", &bin)
        .env("AGENTERM_APPLICATIONS_DIR", &applications)
        .env("AGENTERM_NO_LAUNCH", "1")
        .output()
        .expect("run local installer");
    assert!(
        output.status.success(),
        "local installer failed:\nstdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(
        applications
            .join("AgenTerm.app/Contents/MacOS/AgenTerm")
            .exists(),
        "app executable is missing"
    );
    assert!(
        applications
            .join("AgenTerm.app/Contents/Info.plist")
            .is_file(),
        "app Info.plist is missing"
    );
    assert!(
        applications
            .join("AgenTerm.app/Contents/Resources/AgenTerm.icns")
            .is_file(),
        "app icon is missing"
    );
    let plist = fs::read_to_string(applications.join("AgenTerm.app/Contents/Info.plist"))
        .expect("read app Info.plist");
    assert!(
        plist.contains("<key>CFBundleIconFile</key>")
            && plist.contains("<string>AgenTerm.icns</string>"),
        "app icon is not declared"
    );
    assert!(bin.join("agenterm-cli").exists(), "CLI link is missing");
    assert!(install.join("current").exists(), "current link is missing");
    let architecture = match std::env::consts::ARCH {
        "aarch64" => "aarch64",
        "x86_64" => "x86_64",
        other => panic!("unexpected macOS architecture: {other}"),
    };
    assert!(
        install
            .join(format!(
                "releases/{CURRENT_VERSION}-local-macos-{architecture}/agenterm"
            ))
            .exists(),
        "versioned local payload is missing"
    );

    fs::remove_dir_all(&root).expect("remove fixture root");
}
