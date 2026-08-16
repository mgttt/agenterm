//! Live first-window chassis journey: checked image -> real server PTY/IPC -> L2 Host ABI.

#[allow(dead_code)]
#[path = "../src/frontend/chassis_image.rs"]
mod chassis_image;

use std::fs;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use agenterm_chassis::CELLS;
use agenterm_chassis::l2_dispatch::HostCallback;
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

struct LiveServer {
    root: PathBuf,
    address: String,
    child: Option<std::process::Child>,
    _tree: Option<agenterm_platform::process::ProcessTreeGuard>,
}

impl LiveServer {
    fn start() -> Self {
        let root = std::env::temp_dir().join(format!(
            "agenterm-chassis-live-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time")
                .as_millis()
        ));
        fs::create_dir_all(root.join("instances")).expect("isolation root");
        let address = TcpListener::bind("127.0.0.1:0")
            .expect("reserve loopback")
            .local_addr()
            .expect("loopback address")
            .to_string();
        let mut server = Self {
            root,
            address,
            child: None,
            _tree: None,
        };
        let child = server
            .command()
            .args(["server", "--address", &server.address])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("start real server");
        server._tree = Some(
            agenterm_platform::process::ProcessTreeGuard::attach(&child)
                .expect("contain server tree"),
        );
        server.child = Some(child);
        server.wait_for_snapshot();
        server
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_agenterm"));
        command
            .env("AGENTERM_IPC_ADDRESS", &self.address)
            .env("AGENTERM_WORKSPACE_PATH", self.root.join("workspace.json"))
            .env("AGENTERM_SETTINGS_PATH", self.root.join("settings.json"))
            .env("AGENTERM_INSTANCE_DIR", self.root.join("instances"));
        command
    }

    fn snapshot(&self) -> Result<Value, String> {
        let output = self
            .command()
            .args(["cli", "--address", &self.address, "ui-snapshot"])
            .output()
            .map_err(|error| error.to_string())?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).into_owned());
        }
        serde_json::from_slice(&output.stdout).map_err(|error| error.to_string())
    }

    fn wait_for_snapshot(&self) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while self.snapshot().is_err() {
            assert!(Instant::now() < deadline, "real IPC server did not start");
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

impl Drop for LiveServer {
    fn drop(&mut self) {
        let _ = self
            .command()
            .args(["cli", "--address", &self.address, "kill-server"])
            .output();
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        let _ = fs::remove_dir_all(&self.root);
    }
}

struct IpcHost<'a>(&'a LiveServer);

impl HostCallback for IpcHost<'_> {
    fn call(&mut self, capability: &str, parameters: &Value) -> Result<Value, String> {
        if capability != "tabs.active" || parameters != &json!({}) {
            return Err(format!("unexpected Host ABI call `{capability}`"));
        }
        let snapshot = self.0.snapshot()?;
        let active = snapshot["active_tab_id"]
            .as_str()
            .and_then(|id| id.strip_prefix('@'))
            .and_then(|id| id.parse::<i64>().ok())
            .ok_or_else(|| "real IPC snapshot has no active PTY".to_owned())?;
        if snapshot["tabs"].as_array().is_none_or(Vec::is_empty) {
            return Err("real host has no PTY tabs".to_owned());
        }
        Ok(Value::from(active))
    }
}

#[test]
fn checked_image_dispatches_l2_against_real_pty_and_ipc_host() {
    let image = ImageFixture::new();
    let checked = chassis_image::load_image(&image.installed).expect("checked chassis image");
    let server = LiveServer::start();

    let (active_tab, _) =
        chassis_image::eval_active_tab(&checked, IpcHost(&server)).expect("live L2 dispatch");
    assert!(active_tab > 0);
}

struct ImageFixture {
    _root: tempfile::TempDir,
    installed: PathBuf,
}

impl ImageFixture {
    fn new() -> Self {
        let root = tempfile::tempdir().expect("image root");
        let staged = root.path().join("staged");
        let installed = root.path().join("installed");
        for cell in CELLS {
            let directory = staged.join("l1").join(cell);
            fs::create_dir_all(&directory).expect("L1 cell");
            let loader = directory.join("loader");
            fs::write(&loader, executable_bytes(cell)).expect("loader");
            make_executable(&loader);
        }
        fs::create_dir_all(staged.join("l2/programs")).expect("L2 programs");
        fs::write(
            staged.join("l2/host-abi.json"),
            include_str!("../crates/agenterm-chassis/l2/host-abi.json"),
        )
        .expect("Host ABI");
        fs::write(
            staged.join("l2/programs/active-tab.json"),
            include_str!("../crates/agenterm-chassis/l2/programs/active-tab.json"),
        )
        .expect("L2 program");
        fs::create_dir_all(staged.join("l3")).expect("L3");
        fs::write(
            staged.join("l3/app.json"),
            r#"{"schema":1,"name":"live-first-window","capabilities":["tabs.active"]}"#,
        )
        .expect("L3 app");
        agenterm_chassis::compose(&staged, &installed).expect("compose image");

        let manifest = installed.join("manifest.json");
        let mut value: Value =
            serde_json::from_slice(&fs::read(&manifest).expect("manifest")).expect("JSON");
        value["l1_sha256"] = Value::Object(
            CELLS
                .iter()
                .map(|cell| {
                    let bytes = fs::read(installed.join("l1").join(cell).join("loader"))
                        .expect("loader bytes");
                    ((*cell).to_owned(), Value::String(sha256_hex(&bytes)))
                })
                .collect(),
        );
        fs::write(&manifest, serde_json::to_vec(&value).expect("JSON")).expect("manifest");
        Self {
            _root: root,
            installed,
        }
    }
}

fn executable_bytes(cell: &str) -> Vec<u8> {
    let mut bytes = match cell.split_once('-').map(|(os, _)| os) {
        Some("win") => b"MZ".to_vec(),
        Some("lnx") => b"\x7fELF".to_vec(),
        Some("osx") => vec![0xcf, 0xfa, 0xed, 0xfe],
        _ => unreachable!("canonical cell"),
    };
    bytes.extend_from_slice(format!("thin-loader-{cell}").as_bytes());
    bytes
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(unix)]
fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt as _;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).expect("executable");
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) {}
