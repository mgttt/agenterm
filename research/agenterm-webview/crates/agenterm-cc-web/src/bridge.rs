use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use std::sync::Mutex;

pub const BRIDGE_VERSION: u32 = 1;
pub const BRIDGE_ORIGIN: &str = "agenterm://localhost";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BridgeLimits {
    pub max_message_bytes: usize,
    pub max_concurrent_requests: usize,
    pub max_deadline_ahead_ms: u64,
    pub max_requests_per_document: usize,
}

impl Default for BridgeLimits {
    fn default() -> Self {
        Self {
            max_message_bytes: 64 * 1024,
            max_concurrent_requests: 8,
            max_deadline_ahead_ms: 30_000,
            max_requests_per_document: 4_096,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BridgeFrame<'a> {
    pub origin: &'a str,
    pub is_main_frame: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct BridgeRequest {
    pub version: u32,
    pub document_nonce: String,
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

#[derive(Debug, Default)]
struct BridgeState {
    in_flight: HashSet<String>,
    seen_request_ids: HashSet<String>,
}

/// Security state for exactly one loaded packaged document.
///
/// Navigation or reload must create a fresh session. A permit keeps a request
/// counted as in flight until the native operation and response are complete.
#[derive(Debug)]
pub struct BridgeSession {
    document_nonce: String,
    limits: BridgeLimits,
    state: Mutex<BridgeState>,
}

impl BridgeSession {
    pub fn new(limits: BridgeLimits) -> Result<Self, BridgeRejection> {
        let mut nonce = [0_u8; 32];
        getrandom::getrandom(&mut nonce).map_err(|error| {
            BridgeRejection::new(
                "nonce_generation_failed",
                format!("operating-system random source failed: {error}"),
            )
        })?;
        Ok(Self::with_document_nonce(limits, hex(&nonce)))
    }

    fn with_document_nonce(limits: BridgeLimits, document_nonce: String) -> Self {
        Self {
            document_nonce,
            limits,
            state: Mutex::new(BridgeState::default()),
        }
    }

    pub fn document_nonce(&self) -> &str {
        &self.document_nonce
    }

    pub fn begin<'a>(
        &'a self,
        frame: BridgeFrame<'_>,
        message: &[u8],
        now_ms: u64,
    ) -> Result<BridgePermit<'a>, BridgeRejection> {
        if frame.origin != BRIDGE_ORIGIN {
            return Err(BridgeRejection::new(
                "wrong_origin",
                "bridge accepts only the exact packaged origin",
            ));
        }
        if !frame.is_main_frame {
            return Err(BridgeRejection::new(
                "subframe",
                "bridge accepts messages only from the top-level document",
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
        if request.document_nonce != self.document_nonce {
            return Err(BridgeRejection::new(
                "stale_nonce",
                "message belongs to another document",
            ));
        }
        if !valid_request_id(&request.request_id) {
            return Err(BridgeRejection::new(
                "invalid_request_id",
                "request id must be 1..=128 portable identifier bytes",
            ));
        }
        if !matches!(
            request.method.as_str(),
            "host.ready" | "host.facts" | "fleet.snapshot"
        ) {
            return Err(BridgeRejection::new(
                "unknown_method",
                "method is not part of bridge v1",
            ));
        }
        if !request
            .params
            .as_object()
            .is_some_and(serde_json::Map::is_empty)
        {
            return Err(BridgeRejection::new(
                "invalid_params",
                "bridge v1 methods accept only an empty parameter object",
            ));
        }
        if request.deadline_ms <= now_ms
            || request.deadline_ms.saturating_sub(now_ms) > self.limits.max_deadline_ahead_ms
        {
            return Err(BridgeRejection::new(
                "invalid_deadline",
                "deadline is expired or exceeds the request horizon",
            ));
        }

        let mut state = self
            .state
            .lock()
            .map_err(|_| BridgeRejection::new("bridge_failed", "bridge state is poisoned"))?;
        if state.seen_request_ids.contains(&request.request_id) {
            return Err(BridgeRejection::new(
                "duplicate_request_id",
                "request id was already consumed by this document",
            ));
        }
        if state.seen_request_ids.len() >= self.limits.max_requests_per_document {
            return Err(BridgeRejection::new(
                "document_request_limit",
                "document exhausted its bounded request-id budget",
            ));
        }
        if state.in_flight.len() >= self.limits.max_concurrent_requests {
            return Err(BridgeRejection::new(
                "concurrency_limit",
                "too many bridge requests are in flight",
            ));
        }
        state.seen_request_ids.insert(request.request_id.clone());
        state.in_flight.insert(request.request_id.clone());
        drop(state);

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
        let mut state = self
            .session
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.in_flight.remove(&self.request.request_id);
    }
}

fn valid_request_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        write!(&mut output, "{byte:02x}").expect("writing to String cannot fail");
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_session(limits: BridgeLimits) -> BridgeSession {
        BridgeSession::with_document_nonce(limits, "a".repeat(64))
    }

    fn message(
        session: &BridgeSession,
        request_id: &str,
        method: &str,
        deadline_ms: u64,
    ) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "version": BRIDGE_VERSION,
            "document_nonce": session.document_nonce(),
            "request_id": request_id,
            "method": method,
            "params": {},
            "deadline_ms": deadline_ms,
        }))
        .unwrap()
    }

    fn main_frame() -> BridgeFrame<'static> {
        BridgeFrame {
            origin: BRIDGE_ORIGIN,
            is_main_frame: true,
        }
    }

    #[test]
    fn generated_document_nonces_are_strong_and_distinct() {
        let first = BridgeSession::new(BridgeLimits::default()).unwrap();
        let second = BridgeSession::new(BridgeLimits::default()).unwrap();
        assert_eq!(first.document_nonce().len(), 64);
        assert!(
            first
                .document_nonce()
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        );
        assert_ne!(first.document_nonce(), second.document_nonce());
    }

    #[test]
    fn default_limits_are_the_public_bridge_v1_contract() {
        let limits = BridgeLimits::default();
        assert_eq!(limits.max_message_bytes, 64 * 1024);
        assert_eq!(limits.max_concurrent_requests, 8);

        let session = test_session(limits);
        let permits = (0..8)
            .map(|index| {
                session
                    .begin(
                        main_frame(),
                        &message(
                            &session,
                            &format!("concurrent-{index}"),
                            "host.ready",
                            1_001,
                        ),
                        1_000,
                    )
                    .unwrap()
            })
            .collect::<Vec<_>>();
        assert_eq!(
            session
                .begin(
                    main_frame(),
                    &message(&session, "concurrent-8", "host.ready", 1_001),
                    1_000,
                )
                .unwrap_err()
                .code,
            "concurrency_limit"
        );
        drop(permits);
    }

    #[test]
    fn accepts_exactly_three_read_only_methods() {
        let session = test_session(BridgeLimits::default());
        for (index, method) in ["host.ready", "host.facts", "fleet.snapshot"]
            .into_iter()
            .enumerate()
        {
            let permit = session
                .begin(
                    main_frame(),
                    &message(&session, &format!("request-{index}"), method, 1_001),
                    1_000,
                )
                .unwrap();
            assert_eq!(permit.request().method, method);
        }
        for (index, method) in [
            "eval",
            "shell",
            "process.spawn",
            "network.fetch",
            "host.navigate",
            "download",
        ]
        .into_iter()
        .enumerate()
        {
            assert_eq!(
                session
                    .begin(
                        main_frame(),
                        &message(&session, &format!("denied-{index}"), method, 1_001),
                        1_000,
                    )
                    .unwrap_err()
                    .code,
                "unknown_method"
            );
        }
    }

    #[test]
    fn rejects_foreign_origin_subframe_and_stale_document() {
        let session = test_session(BridgeLimits::default());
        let request = message(&session, "binding-1", "host.ready", 1_001);
        for origin in [
            "https://example.invalid",
            "agenterm://localhost/",
            "agenterm://localhost.evil",
            "http://agenterm.localhost",
        ] {
            assert_eq!(
                session
                    .begin(
                        BridgeFrame {
                            origin,
                            is_main_frame: true,
                        },
                        &request,
                        1_000,
                    )
                    .unwrap_err()
                    .code,
                "wrong_origin"
            );
        }
        assert_eq!(
            session
                .begin(
                    BridgeFrame {
                        origin: BRIDGE_ORIGIN,
                        is_main_frame: false,
                    },
                    &request,
                    1_000,
                )
                .unwrap_err()
                .code,
            "subframe"
        );
        let other = BridgeSession::with_document_nonce(BridgeLimits::default(), "b".repeat(64));
        assert_eq!(
            other.begin(main_frame(), &request, 1_000).unwrap_err().code,
            "stale_nonce"
        );
    }

    #[test]
    fn rejects_malformed_oversized_and_expansive_requests() {
        let session = test_session(BridgeLimits {
            max_message_bytes: 512,
            ..BridgeLimits::default()
        });
        assert_eq!(
            session
                .begin(main_frame(), &vec![b' '; 513], 1_000)
                .unwrap_err()
                .code,
            "message_too_large"
        );
        assert_eq!(
            session
                .begin(main_frame(), b"not json", 1_000)
                .unwrap_err()
                .code,
            "invalid_request"
        );
        let unknown_field = serde_json::to_vec(&serde_json::json!({
            "version": BRIDGE_VERSION,
            "document_nonce": session.document_nonce(),
            "request_id": "unknown-field",
            "method": "host.facts",
            "params": {},
            "deadline_ms": 1_001,
            "command": "shell"
        }))
        .unwrap();
        assert_eq!(
            session
                .begin(main_frame(), &unknown_field, 1_000)
                .unwrap_err()
                .code,
            "invalid_request"
        );
        let mut nonempty_params: serde_json::Value =
            serde_json::from_slice(&message(&session, "params", "fleet.snapshot", 1_001)).unwrap();
        nonempty_params["params"] = serde_json::json!({"command": "shell"});
        assert_eq!(
            session
                .begin(
                    main_frame(),
                    &serde_json::to_vec(&nonempty_params).unwrap(),
                    1_000,
                )
                .unwrap_err()
                .code,
            "invalid_params"
        );

        let mut wrong_version: serde_json::Value =
            serde_json::from_slice(&message(&session, "version", "host.ready", 1_001)).unwrap();
        wrong_version["version"] = serde_json::json!(BRIDGE_VERSION + 1);
        assert_eq!(
            session
                .begin(
                    main_frame(),
                    &serde_json::to_vec(&wrong_version).unwrap(),
                    1_000,
                )
                .unwrap_err()
                .code,
            "version_mismatch"
        );
        for invalid_id in ["", "../escape", "contains space", "非ascii"] {
            assert_eq!(
                session
                    .begin(
                        main_frame(),
                        &message(&session, invalid_id, "host.ready", 1_001),
                        1_000,
                    )
                    .unwrap_err()
                    .code,
                "invalid_request_id"
            );
        }
        assert_eq!(
            session
                .begin(
                    main_frame(),
                    &message(&session, &"x".repeat(129), "host.ready", 1_001),
                    1_000,
                )
                .unwrap_err()
                .code,
            "invalid_request_id"
        );
    }

    #[test]
    fn enforces_deadline_concurrency_replay_and_document_budgets() {
        let session = test_session(BridgeLimits {
            max_concurrent_requests: 2,
            max_deadline_ahead_ms: 100,
            max_requests_per_document: 3,
            ..BridgeLimits::default()
        });
        for deadline in [999, 1_000, 1_101, u64::MAX] {
            assert_eq!(
                session
                    .begin(
                        main_frame(),
                        &message(
                            &session,
                            &format!("deadline-{deadline}"),
                            "host.ready",
                            deadline
                        ),
                        1_000,
                    )
                    .unwrap_err()
                    .code,
                "invalid_deadline"
            );
        }

        let first = session
            .begin(
                main_frame(),
                &message(&session, "one", "host.ready", 1_100),
                1_000,
            )
            .unwrap();
        let second = session
            .begin(
                main_frame(),
                &message(&session, "two", "host.facts", 1_100),
                1_000,
            )
            .unwrap();
        assert_eq!(
            session
                .begin(
                    main_frame(),
                    &message(&session, "three", "fleet.snapshot", 1_100),
                    1_000,
                )
                .unwrap_err()
                .code,
            "concurrency_limit"
        );
        drop(first);
        let third = session
            .begin(
                main_frame(),
                &message(&session, "three", "fleet.snapshot", 1_100),
                1_000,
            )
            .unwrap();
        drop((second, third));
        assert_eq!(
            session
                .begin(
                    main_frame(),
                    &message(&session, "one", "host.ready", 1_100),
                    1_000
                )
                .unwrap_err()
                .code,
            "duplicate_request_id"
        );
        assert_eq!(
            session
                .begin(
                    main_frame(),
                    &message(&session, "four", "host.ready", 1_100),
                    1_000
                )
                .unwrap_err()
                .code,
            "document_request_limit"
        );
    }
}
