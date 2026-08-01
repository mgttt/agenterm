//! Renderer-neutral system WebView runtime-presence and local bridge contract.
//!
//! Probing is deliberately passive: it reads filesystem facts only. It never
//! downloads a runtime, starts a process, opens a socket, or creates a window.
//! Runtime presence is not host availability: v0.1.11 has no WebView host and
//! keeps the native renderer active on every platform.

use std::{
    sync::atomic::{AtomicU64, AtomicUsize, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const APP_ORIGIN: &str = "agenterm://control-center";
pub const BRIDGE_VERSION: u32 = 1;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WebViewRuntimePresence {
    Detected,
    Missing,
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WebViewHostState {
    Unimplemented,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct WebViewHostFacts {
    pub runtime_presence: WebViewRuntimePresence,
    pub host_state: WebViewHostState,
    pub backend: &'static str,
    pub version: Option<String>,
    pub source: Option<String>,
    pub runtime_reason: Option<String>,
    pub host_reason: &'static str,
    pub active_renderer: &'static str,
    pub bridge_version: u32,
}

/// Inspect the platform WebView without initializing it.
pub fn probe() -> WebViewHostFacts {
    let platform = crate::platform::webview::probe_system_webview();
    WebViewHostFacts {
        runtime_presence: match platform.presence {
            crate::platform::webview::RuntimePresence::Detected => WebViewRuntimePresence::Detected,
            crate::platform::webview::RuntimePresence::Missing => WebViewRuntimePresence::Missing,
            crate::platform::webview::RuntimePresence::Failed => WebViewRuntimePresence::Failed,
            _ => WebViewRuntimePresence::Failed,
        },
        host_state: WebViewHostState::Unimplemented,
        backend: platform.backend,
        version: platform.version,
        source: platform.source,
        runtime_reason: platform.reason,
        host_reason: "system_webview_host_not_implemented",
        active_renderer: "native",
        bridge_version: BRIDGE_VERSION,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BridgeLimits {
    pub max_message_bytes: usize,
    pub max_concurrent_requests: usize,
    pub max_deadline_ahead_ms: u64,
}

impl Default for BridgeLimits {
    fn default() -> Self {
        Self {
            max_message_bytes: 64 * 1024,
            max_concurrent_requests: 8,
            max_deadline_ahead_ms: 30_000,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct BridgeFrame<'a> {
    pub origin: &'a str,
    pub is_main_frame: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct BridgeRequest {
    pub version: u32,
    pub session_nonce: String,
    pub request_id: String,
    pub method: String,
    pub params: Value,
    pub deadline_ms: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct BridgeRejection {
    pub code: &'static str,
    pub reason: String,
}

impl BridgeRejection {
    fn new(code: &'static str, reason: impl Into<String>) -> Self {
        Self {
            code,
            reason: reason.into(),
        }
    }
}

/// Per-document bridge state. A new navigation must receive a new session.
#[derive(Debug)]
pub struct BridgeSession {
    nonce: String,
    limits: BridgeLimits,
    in_flight: AtomicUsize,
}

impl BridgeSession {
    pub fn new(limits: BridgeLimits) -> Self {
        static NEXT_NONCE: AtomicU64 = AtomicU64::new(1);
        let sequence = NEXT_NONCE.fetch_add(1, Ordering::Relaxed);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        Self {
            nonce: format!("{:x}-{:x}-{sequence:x}", std::process::id(), now),
            limits,
            in_flight: AtomicUsize::new(0),
        }
    }

    pub fn nonce(&self) -> &str {
        &self.nonce
    }

    pub fn begin<'a>(
        &'a self,
        frame: BridgeFrame<'_>,
        message: &[u8],
        now_ms: u64,
    ) -> Result<BridgePermit<'a>, BridgeRejection> {
        if frame.origin != APP_ORIGIN {
            return Err(BridgeRejection::new(
                "wrong_origin",
                "bridge accepts only the exact packaged app origin",
            ));
        }
        if !frame.is_main_frame {
            return Err(BridgeRejection::new(
                "subframe",
                "bridge accepts messages only from the main frame",
            ));
        }
        if message.len() > self.limits.max_message_bytes {
            return Err(BridgeRejection::new(
                "message_too_large",
                "bridge message exceeds the byte limit",
            ));
        }
        let request: BridgeRequest = serde_json::from_slice(message)
            .map_err(|error| BridgeRejection::new("invalid_request", error.to_string()))?;
        if request.version != BRIDGE_VERSION {
            return Err(BridgeRejection::new(
                "version_mismatch",
                "unsupported bridge version",
            ));
        }
        if request.session_nonce != self.nonce {
            return Err(BridgeRejection::new(
                "stale_nonce",
                "message belongs to another document session",
            ));
        }
        if request.request_id.is_empty() || request.request_id.len() > 128 {
            return Err(BridgeRejection::new(
                "invalid_request_id",
                "request id must be non-empty and bounded",
            ));
        }
        if !matches!(
            request.method.as_str(),
            "host.ready" | "host.facts" | "fleet.snapshot"
        ) {
            return Err(BridgeRejection::new(
                "unknown_method",
                "method is not part of the typed host bridge",
            ));
        }
        if !request
            .params
            .as_object()
            .is_some_and(serde_json::Map::is_empty)
        {
            return Err(BridgeRejection::new(
                "invalid_params",
                "this bridge version requires an empty parameter object",
            ));
        }
        if request.deadline_ms < now_ms
            || request.deadline_ms.saturating_sub(now_ms) > self.limits.max_deadline_ahead_ms
        {
            return Err(BridgeRejection::new(
                "invalid_deadline",
                "deadline is expired or exceeds the bounded request horizon",
            ));
        }
        let previous = self.in_flight.fetch_add(1, Ordering::AcqRel);
        if previous >= self.limits.max_concurrent_requests {
            self.in_flight.fetch_sub(1, Ordering::AcqRel);
            return Err(BridgeRejection::new(
                "concurrency_limit",
                "too many host bridge requests are in flight",
            ));
        }
        Ok(BridgePermit {
            session: self,
            request,
        })
    }
}

#[derive(Debug)]
pub struct BridgePermit<'a> {
    session: &'a BridgeSession,
    request: BridgeRequest,
}

impl BridgePermit<'_> {
    pub fn request(&self) -> &BridgeRequest {
        &self.request
    }
}

impl Drop for BridgePermit<'_> {
    fn drop(&mut self) {
        self.session.in_flight.fetch_sub(1, Ordering::AcqRel);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(session: &BridgeSession, method: &str, deadline_ms: u64) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "version": BRIDGE_VERSION,
            "session_nonce": session.nonce(),
            "request_id": "request-1",
            "method": method,
            "params": {},
            "deadline_ms": deadline_ms,
        }))
        .unwrap()
    }

    #[test]
    fn accepts_only_the_typed_main_frame_contract() {
        let session = BridgeSession::new(BridgeLimits::default());
        let permit = session
            .begin(
                BridgeFrame {
                    origin: APP_ORIGIN,
                    is_main_frame: true,
                },
                &message(&session, "host.facts", 1_100),
                1_000,
            )
            .unwrap();
        assert_eq!(permit.request().method, "host.facts");
    }

    #[test]
    fn rejects_wrong_origin_subframe_unknown_method_and_stale_nonce() {
        let session = BridgeSession::new(BridgeLimits::default());
        let valid_frame = BridgeFrame {
            origin: APP_ORIGIN,
            is_main_frame: true,
        };
        assert_eq!(
            session
                .begin(
                    BridgeFrame {
                        origin: "https://example.invalid",
                        is_main_frame: true,
                    },
                    &message(&session, "host.ready", 1_100),
                    1_000,
                )
                .unwrap_err()
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
                    &message(&session, "host.ready", 1_100),
                    1_000,
                )
                .unwrap_err()
                .code,
            "subframe"
        );
        assert_eq!(
            session
                .begin(
                    valid_frame,
                    &message(&session, "runtime.eval", 1_100),
                    1_000
                )
                .unwrap_err()
                .code,
            "unknown_method"
        );
        let other = BridgeSession::new(BridgeLimits::default());
        assert_eq!(
            session
                .begin(valid_frame, &message(&other, "host.ready", 1_100), 1_000)
                .unwrap_err()
                .code,
            "stale_nonce"
        );
    }

    #[test]
    fn typed_methods_reject_unrecognized_parameters() {
        let session = BridgeSession::new(BridgeLimits::default());
        let encoded = serde_json::to_vec(&serde_json::json!({
            "version": BRIDGE_VERSION,
            "session_nonce": session.nonce(),
            "request_id": "request-1",
            "method": "fleet.snapshot",
            "params": {"command": "shell"},
            "deadline_ms": 1_100,
        }))
        .unwrap();
        assert_eq!(
            session
                .begin(
                    BridgeFrame {
                        origin: APP_ORIGIN,
                        is_main_frame: true,
                    },
                    &encoded,
                    1_000,
                )
                .unwrap_err()
                .code,
            "invalid_params"
        );
    }

    #[test]
    fn enforces_message_deadline_and_concurrency_limits() {
        let session = BridgeSession::new(BridgeLimits {
            max_message_bytes: 512,
            max_concurrent_requests: 1,
            max_deadline_ahead_ms: 100,
        });
        let frame = BridgeFrame {
            origin: APP_ORIGIN,
            is_main_frame: true,
        };
        let first = session
            .begin(frame, &message(&session, "host.ready", 1_100), 1_000)
            .unwrap();
        assert_eq!(
            session
                .begin(frame, &message(&session, "host.facts", 1_100), 1_000)
                .unwrap_err()
                .code,
            "concurrency_limit"
        );
        drop(first);
        assert_eq!(
            session
                .begin(frame, &message(&session, "host.ready", 999), 1_000)
                .unwrap_err()
                .code,
            "invalid_deadline"
        );
        assert_eq!(
            session
                .begin(frame, &vec![b' '; 513], 1_000)
                .unwrap_err()
                .code,
            "message_too_large"
        );
    }
}
