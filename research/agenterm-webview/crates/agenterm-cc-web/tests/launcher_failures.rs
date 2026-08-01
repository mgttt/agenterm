use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new(case: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "agenterm-webview-{case}-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn executable_name(stem: &str) -> String {
    if cfg!(windows) {
        format!("{stem}.exe")
    } else {
        stem.to_owned()
    }
}

fn staged_launcher(directory: &Path) -> PathBuf {
    let destination = directory.join(executable_name("agenterm-cc-web"));
    fs::copy(env!("CARGO_BIN_EXE_agenterm-cc-web"), &destination).unwrap();
    destination
}

fn invoke(launcher: &Path) -> Output {
    Command::new(launcher).arg("--probe").output().unwrap()
}

fn receipt(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "receipt was not JSON: {error}; stdout={}; stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn assert_unavailable(output: &Output, code: &str, stage: &str) {
    assert_eq!(output.status.code(), Some(69));
    let receipt = receipt(output);
    assert_eq!(receipt["schema"], "agenterm.webview-host/1");
    assert_eq!(receipt["status"], "unavailable");
    assert_eq!(receipt["active_renderer"], "native");
    assert_eq!(receipt["failure"]["code"], code);
    assert_eq!(receipt["failure"]["stage"], stage);
    assert!(!receipt["failure"]["detail"].as_str().unwrap().is_empty());
}

#[test]
fn missing_direct_host_is_a_typed_fallback() {
    let directory = TestDirectory::new("missing-host");
    let launcher = staged_launcher(&directory.0);
    assert_unavailable(&invoke(&launcher), "host_executable_missing", "launcher");
}

#[test]
fn invalid_direct_host_is_a_typed_launch_failure() {
    let directory = TestDirectory::new("invalid-host");
    let launcher = staged_launcher(&directory.0);
    let host = directory
        .0
        .join(executable_name("agenterm-cc-web-direct-wry"));
    fs::write(&host, b"not a native executable").unwrap();

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mut permissions = fs::metadata(&host).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&host, permissions).unwrap();
    }

    assert_unavailable(&invoke(&launcher), "host_launch_failed", "launcher");
}
