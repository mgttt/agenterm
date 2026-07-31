use agenterm::webview_host::{
    APP_ORIGIN, BRIDGE_VERSION, BridgeFrame, BridgeLimits, BridgeSession, WebViewHostState,
    WebViewRuntimePresence, probe,
};
use std::process::Command;

fn request(method: &str, nonce: &str, deadline_ms: u64) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "version": BRIDGE_VERSION,
        "session_nonce": nonce,
        "request_id": "contract-1",
        "method": method,
        "params": {},
        "deadline_ms": deadline_ms,
    }))
    .expect("encode bridge request")
}

#[test]
fn passive_probe_reports_the_platform_backend_truthfully() {
    let facts = probe();
    assert_eq!(facts.active_renderer, "native");
    assert_eq!(facts.host_state, WebViewHostState::Unimplemented);
    assert_eq!(facts.host_reason, "system_webview_host_not_implemented");
    assert_eq!(facts.bridge_version, BRIDGE_VERSION);
    assert!(!facts.backend.is_empty());
    match facts.runtime_presence {
        WebViewRuntimePresence::Detected => {
            assert!(facts.source.is_some());
            assert!(facts.runtime_reason.is_none());
        }
        WebViewRuntimePresence::Missing | WebViewRuntimePresence::Failed => {
            assert!(facts.runtime_reason.is_some())
        }
    }

    #[cfg(windows)]
    assert_eq!(facts.backend, "webview2");
    #[cfg(target_os = "macos")]
    assert_eq!(facts.backend, "wkwebview");
    #[cfg(all(unix, not(target_os = "macos")))]
    assert_eq!(facts.backend, "webkitgtk");
}

#[test]
fn bridge_contract_has_no_generic_execution_escape_hatch() {
    let session = BridgeSession::new(BridgeLimits::default());
    let frame = BridgeFrame {
        origin: APP_ORIGIN,
        is_main_frame: true,
    };
    for method in ["host.ready", "host.facts", "fleet.snapshot"] {
        let permit = session
            .begin(frame, &request(method, session.nonce(), 10_100), 10_000)
            .expect("typed method accepted");
        assert_eq!(permit.request().method, method);
    }
    for method in ["eval", "shell", "runtime.eval", "host.navigate"] {
        let error = session
            .begin(frame, &request(method, session.nonce(), 10_100), 10_000)
            .expect_err("generic method rejected");
        assert_eq!(error.code, "unknown_method");
    }
}

#[test]
fn navigation_nonce_origin_and_frame_are_all_binding() {
    let session = BridgeSession::new(BridgeLimits::default());
    let old_session = BridgeSession::new(BridgeLimits::default());
    let message = request("host.ready", old_session.nonce(), 10_100);
    assert_eq!(
        session
            .begin(
                BridgeFrame {
                    origin: APP_ORIGIN,
                    is_main_frame: true,
                },
                &message,
                10_000,
            )
            .expect_err("old document rejected")
            .code,
        "stale_nonce"
    );
    assert_eq!(
        session
            .begin(
                BridgeFrame {
                    origin: "agenterm://control-center/",
                    is_main_frame: true,
                },
                &request("host.ready", session.nonce(), 10_100),
                10_000,
            )
            .expect_err("non-exact origin rejected")
            .code,
        "wrong_origin"
    );
    assert_eq!(
        session
            .begin(
                BridgeFrame {
                    origin: APP_ORIGIN,
                    is_main_frame: false,
                },
                &request("host.ready", session.nonce(), 10_100),
                10_000,
            )
            .expect_err("subframe rejected")
            .code,
        "subframe"
    );
}

#[test]
fn control_center_reports_webview_facts_without_opening_a_window() {
    let registry = std::env::temp_dir().join(format!(
        "agenterm-webview-facts-{}.json",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&registry);
    let output = Command::new(env!("CARGO_BIN_EXE_agenterm-cc"))
        .args(["capabilities", "--json"])
        .env("AGENTERM_CC_REGISTRY_PATH", &registry)
        .output()
        .expect("query Control Center capabilities");
    assert!(output.status.success());
    let document: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("capability JSON");
    assert_eq!(document["renderer"], "native");
    assert_eq!(document["webview_host"]["host_state"], "unimplemented");
    assert_eq!(document["webview_host"]["active_renderer"], "native");
    assert_ne!(
        document["webview_host"]["runtime_presence"], "available",
        "runtime discovery must not be reported as host availability"
    );
    assert_eq!(document["webview_host"]["bridge_version"], BRIDGE_VERSION);
    assert!(
        !registry.exists(),
        "capability query must not claim the UI registry"
    );
}
