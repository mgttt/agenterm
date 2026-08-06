use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

mod bridge;

pub use bridge::{
    BRIDGE_ORIGIN, BRIDGE_VERSION, BridgeFrame, BridgeLimits, BridgePermit, BridgeRejection,
    BridgeRequest, BridgeSession,
};

pub const CONTRACT_VERSION: &str = "agenterm.webview-host/1";
pub const ASSET_VERSION: &str = "cc-shell-placeholder/2";
pub const LOCAL_URL: &str = "agenterm://localhost/index.html";
pub const MAX_ASSET_BYTES: usize = 64 * 1024;

pub const CONTENT_SECURITY_POLICY: &str = "default-src 'none'; script-src 'self'; style-src 'self'; connect-src 'none'; img-src 'none'; font-src 'none'; media-src 'none'; object-src 'none'; frame-src 'none'; child-src 'none'; worker-src 'none'; manifest-src 'none'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PackagedAsset {
    pub path: &'static str,
    pub media_type: &'static str,
    pub bytes: &'static [u8],
}

pub const ASSETS: &[PackagedAsset] = &[
    PackagedAsset {
        path: "/index.html",
        media_type: "text/html; charset=utf-8",
        bytes: include_bytes!("../../../assets/index.html"),
    },
    PackagedAsset {
        path: "/app.css",
        media_type: "text/css; charset=utf-8",
        bytes: include_bytes!("../../../assets/app.css"),
    },
    PackagedAsset {
        path: "/app.js",
        media_type: "text/javascript; charset=utf-8",
        bytes: include_bytes!("../../../assets/app.js"),
    },
];

#[derive(Debug, Serialize)]
pub struct AssetIdentity {
    pub path: &'static str,
    pub media_type: &'static str,
    pub bytes: usize,
    pub sha256: String,
}

#[derive(Debug, Serialize)]
pub struct AssetManifest {
    pub schema: &'static str,
    pub version: &'static str,
    pub local_url: &'static str,
    pub assets: Vec<AssetIdentity>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HostFailureCode {
    CurrentExecutableFailed,
    HostExecutableMissing,
    HostLaunchFailed,
    SystemRuntimeUnavailable,
    NativeWindowUnavailable,
    WebviewCreationFailed,
    HostProcessFailed,
    HostReceiptInvalid,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HostFailureStage {
    Launcher,
    RuntimeProbe,
    NativeWindow,
    WebviewCreation,
    HostProcess,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct HostFailure {
    pub code: HostFailureCode,
    pub stage: HostFailureStage,
    pub detail: String,
}

impl HostFailure {
    pub fn new(code: HostFailureCode, stage: HostFailureStage, detail: impl Into<String>) -> Self {
        Self {
            code,
            stage,
            detail: detail.into(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct LauncherReceipt {
    pub schema: &'static str,
    pub implementation: &'static str,
    pub status: &'static str,
    pub reason: String,
    pub failure: HostFailure,
    pub requested_implementation: &'static str,
    pub host_path: PathBuf,
    pub host_exit_code: Option<i32>,
    pub host_receipt: Option<serde_json::Value>,
    pub active_renderer: &'static str,
}

pub fn host_failure_from_receipt(
    receipt: Option<&serde_json::Value>,
    detail: impl Into<String>,
) -> HostFailure {
    let detail = detail.into();
    let Some(receipt) = receipt else {
        return HostFailure::new(
            HostFailureCode::HostProcessFailed,
            HostFailureStage::HostProcess,
            detail,
        );
    };
    if receipt.get("schema").and_then(serde_json::Value::as_str) != Some(CONTRACT_VERSION)
        || receipt.get("status").and_then(serde_json::Value::as_str) != Some("unavailable")
    {
        return HostFailure::new(
            HostFailureCode::HostReceiptInvalid,
            HostFailureStage::HostProcess,
            detail,
        );
    }
    let Some(failure) = receipt.get("failure") else {
        return HostFailure::new(
            HostFailureCode::HostReceiptInvalid,
            HostFailureStage::HostProcess,
            detail,
        );
    };
    serde_json::from_value(failure.clone()).unwrap_or_else(|_| {
        HostFailure::new(
            HostFailureCode::HostReceiptInvalid,
            HostFailureStage::HostProcess,
            detail,
        )
    })
}

pub fn asset_manifest() -> AssetManifest {
    AssetManifest {
        schema: CONTRACT_VERSION,
        version: ASSET_VERSION,
        local_url: LOCAL_URL,
        assets: ASSETS
            .iter()
            .map(|asset| AssetIdentity {
                path: asset.path,
                media_type: asset.media_type,
                bytes: asset.bytes.len(),
                sha256: sha256_hex(asset.bytes),
            })
            .collect(),
    }
}

pub fn asset_for_path(path: &str) -> Option<PackagedAsset> {
    let path = if path == "/" { "/index.html" } else { path };
    ASSETS.iter().copied().find(|asset| asset.path == path)
}

pub fn canonical_local_path(url: &str) -> Option<&str> {
    // WRY maps custom protocols to a synthetic HTTP origin on Windows/Android.
    // Both accepted spellings represent the same single packaged origin.
    let path = url
        .strip_prefix("agenterm://localhost")
        .or_else(|| url.strip_prefix("http://agenterm.localhost"))?;
    if path.contains(['?', '#', '\\']) || path.contains("..") {
        return None;
    }
    let path = if path.is_empty() { "/" } else { path };
    asset_for_path(path).map(|asset| asset.path)
}

pub fn is_allowed_navigation(url: &str) -> bool {
    canonical_local_path(url).is_some()
}

pub fn direct_host_path(current_exe: &Path) -> PathBuf {
    host_path(current_exe, "agenterm-cc-web-direct-wry")
}

pub fn tauri_host_path(current_exe: &Path) -> PathBuf {
    host_path(current_exe, "agenterm-cc-web-tauri")
}

fn host_path(current_exe: &Path, stem: &str) -> PathBuf {
    let suffix = current_exe.extension().and_then(|value| value.to_str());
    let name = if suffix.is_some_and(|value| value.eq_ignore_ascii_case("exe")) {
        format!("{stem}.exe")
    } else {
        stem.to_string()
    };
    current_exe.with_file_name(name)
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut result = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        write!(&mut result, "{byte:02x}").expect("writing to String cannot fail");
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packaged_assets_are_bounded_unique_and_identified() {
        assert!(!ASSETS.is_empty());
        for (index, asset) in ASSETS.iter().enumerate() {
            assert!(!asset.bytes.is_empty(), "{} is empty", asset.path);
            assert!(
                asset.bytes.len() <= MAX_ASSET_BYTES,
                "{} is too large",
                asset.path
            );
            assert!(
                !ASSETS[..index].iter().any(|other| other.path == asset.path),
                "duplicate asset path {}",
                asset.path
            );
            assert_eq!(sha256_hex(asset.bytes).len(), 64);
        }
    }

    #[test]
    fn navigation_is_closed_to_exact_packaged_origin_and_routes() {
        for allowed in [
            "agenterm://localhost/",
            "agenterm://localhost/index.html",
            "http://agenterm.localhost/app.css",
            "http://agenterm.localhost/app.js",
        ] {
            assert!(is_allowed_navigation(allowed), "should allow {allowed}");
        }
        for denied in [
            "https://example.com/",
            "agenterm://evil/index.html",
            "agenterm://localhost/missing",
            "agenterm://localhost/../index.html",
            "agenterm://localhost/index.html?remote=true",
            "http://agenterm.localhost.evil/app.js",
        ] {
            assert!(!is_allowed_navigation(denied), "should deny {denied}");
        }
    }

    #[test]
    fn csp_denies_network_and_embedding() {
        for directive in [
            "default-src 'none'",
            "connect-src 'none'",
            "object-src 'none'",
            "frame-src 'none'",
            "form-action 'none'",
        ] {
            assert!(CONTENT_SECURITY_POLICY.contains(directive));
        }
    }

    #[test]
    fn packaged_script_has_no_bridge_or_network_escape_hatch() {
        let script = std::str::from_utf8(asset_for_path("/app.js").unwrap().bytes).unwrap();
        for forbidden in [
            "window.ipc",
            "fetch(",
            "XMLHttpRequest",
            "WebSocket",
            "eval(",
            "localStorage",
            "sessionStorage",
            "window.open",
            "location =",
        ] {
            assert!(!script.contains(forbidden), "script contains {forbidden}");
        }
    }

    #[test]
    fn fallback_hosts_are_explicit_sibling_processes() {
        let launcher = Path::new("/tmp/agenterm-cc-web");
        assert_eq!(
            direct_host_path(launcher),
            Path::new("/tmp/agenterm-cc-web-direct-wry")
        );
        assert_eq!(
            tauri_host_path(launcher),
            Path::new("/tmp/agenterm-cc-web-tauri")
        );
    }

    #[test]
    fn typed_host_failure_is_inherited_only_from_a_valid_receipt() {
        let receipt = serde_json::json!({
            "schema": CONTRACT_VERSION,
            "status": "unavailable",
            "failure": {
                "code": "system_runtime_unavailable",
                "stage": "runtime_probe",
                "detail": "runtime absent"
            }
        });
        assert_eq!(
            host_failure_from_receipt(Some(&receipt), "fallback"),
            HostFailure::new(
                HostFailureCode::SystemRuntimeUnavailable,
                HostFailureStage::RuntimeProbe,
                "runtime absent"
            )
        );

        let absent = host_failure_from_receipt(None, "host exited");
        assert_eq!(absent.code, HostFailureCode::HostProcessFailed);
        assert_eq!(absent.stage, HostFailureStage::HostProcess);

        let untyped = serde_json::json!({
            "schema": CONTRACT_VERSION,
            "status": "unavailable"
        });
        let invalid = host_failure_from_receipt(Some(&untyped), "untyped receipt");
        assert_eq!(invalid.code, HostFailureCode::HostReceiptInvalid);

        let unknown_code = serde_json::json!({
            "schema": CONTRACT_VERSION,
            "status": "unavailable",
            "failure": { "code": "invented", "stage": "runtime_probe", "detail": "x" }
        });
        let invalid = host_failure_from_receipt(Some(&unknown_code), "unknown code");
        assert_eq!(invalid.code, HostFailureCode::HostReceiptInvalid);
        assert_eq!(invalid.detail, "unknown code");
    }
}
