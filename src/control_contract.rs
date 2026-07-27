use std::collections::VecDeque;
use std::fmt;

use serde::{Deserialize, Serialize};

pub(crate) const CONTROL_CONTRACT_SCHEMA_VERSION: u32 = 1;

fn contract_schema_version() -> u32 {
    CONTROL_CONTRACT_SCHEMA_VERSION
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct RequestId(String);

impl RequestId {
    pub(crate) fn new(value: impl Into<String>) -> Result<Self, IdentifierError> {
        let value = value.into();
        validate_identifier("request_id", &value)?;
        Ok(Self(value))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct OperationId(String);

impl OperationId {
    pub(crate) fn new(value: impl Into<String>) -> Result<Self, IdentifierError> {
        let value = value.into();
        validate_identifier("operation_id", &value)?;
        Ok(Self(value))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct IdentifierError {
    field: &'static str,
    reason: &'static str,
}

impl fmt::Display for IdentifierError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} {}", self.field, self.reason)
    }
}

impl std::error::Error for IdentifierError {}

fn validate_identifier(field: &'static str, value: &str) -> Result<(), IdentifierError> {
    if value.is_empty() {
        return Err(IdentifierError {
            field,
            reason: "cannot be empty",
        });
    }
    if value.len() > 128 {
        return Err(IdentifierError {
            field,
            reason: "cannot exceed 128 bytes",
        });
    }
    if !value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b':'))
    {
        return Err(IdentifierError {
            field,
            reason: "contains unsupported characters",
        });
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct PayloadFingerprint(String);

impl PayloadFingerprint {
    /// Provides a deterministic, dependency-free fingerprint for local replay
    /// detection. It is not a cryptographic integrity digest; callers crossing
    /// a trust boundary should supply a validated cryptographic fingerprint.
    pub(crate) fn from_bytes(payload: &[u8]) -> Self {
        const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
        const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

        let mut hash = FNV_OFFSET_BASIS;
        for byte in payload {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(FNV_PRIME);
        }
        Self(format!("fnv1a64:{hash:016x}"))
    }

    #[cfg(test)]
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RequestIntent {
    Query,
    Mutation,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ControlRequest {
    #[serde(default = "contract_schema_version")]
    pub(crate) schema_version: u32,
    pub(crate) request_id: RequestId,
    pub(crate) operation_id: OperationId,
    pub(crate) payload_fingerprint: PayloadFingerprint,
    pub(crate) intent: RequestIntent,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) deadline_unix_ms: Option<u64>,
}

impl ControlRequest {
    pub(crate) fn new(
        request_id: RequestId,
        operation_id: OperationId,
        payload_fingerprint: PayloadFingerprint,
        intent: RequestIntent,
        deadline_unix_ms: Option<u64>,
    ) -> Self {
        Self {
            schema_version: CONTROL_CONTRACT_SCHEMA_VERSION,
            request_id,
            operation_id,
            payload_fingerprint,
            intent,
            deadline_unix_ms,
        }
    }

    pub(crate) fn mutation_is_expired(&self, now_unix_ms: u64) -> bool {
        self.intent == RequestIntent::Mutation
            && self
                .deadline_unix_ms
                .is_some_and(|deadline| now_unix_ms >= deadline)
    }

    fn dedupe_key(&self) -> DedupeKey {
        DedupeKey {
            operation_id: self.operation_id.clone(),
            payload_fingerprint: self.payload_fingerprint.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct EventPosition {
    pub(crate) epoch: String,
    pub(crate) sequence: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ResolvedTarget {
    pub(crate) server_pid: u32,
    pub(crate) server_address: String,
    pub(crate) server_epoch: String,
    pub(crate) session: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) tab_id: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ReceiptOutcome {
    Committed,
    Accepted,
    NoOp,
    OutcomeUnknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ErrorCategory {
    Validation,
    Conflict,
    NotFound,
    Precondition,
    Availability,
    Timeout,
    Policy,
    Unsupported,
    Internal,
}

impl ErrorCategory {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Validation => "validation",
            Self::Conflict => "conflict",
            Self::NotFound => "not_found",
            Self::Precondition => "precondition",
            Self::Availability => "availability",
            Self::Timeout => "timeout",
            Self::Policy => "policy",
            Self::Unsupported => "unsupported",
            Self::Internal => "internal",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct ControlError {
    pub(crate) code: String,
    pub(crate) category: ErrorCategory,
    pub(crate) message: String,
    #[serde(default)]
    pub(crate) retryable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) details: Option<serde_json::Value>,
}

impl ControlError {
    pub(crate) fn new(
        code: impl Into<String>,
        category: ErrorCategory,
        message: impl Into<String>,
        retryable: bool,
    ) -> Self {
        Self {
            code: code.into(),
            category,
            message: message.into(),
            retryable,
            details: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_details(mut self, details: serde_json::Value) -> Self {
        self.details = Some(details);
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WaitCondition {
    Event,
    PaneContains,
    ProcessExited,
    SubmissionComplete,
    TargetClosed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct WaitDescriptor {
    pub(crate) condition: WaitCondition,
    pub(crate) target: ResolvedTarget,
    pub(crate) minimum_position: EventPosition,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) event_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) deadline_unix_ms: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct ControlReceipt {
    #[serde(default = "contract_schema_version")]
    pub(crate) schema_version: u32,
    pub(crate) request_id: RequestId,
    pub(crate) operation_id: OperationId,
    pub(crate) payload_fingerprint: PayloadFingerprint,
    pub(crate) outcome: ReceiptOutcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) resolved: Option<ResolvedTarget>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) before_position: Option<EventPosition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) after_position: Option<EventPosition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) result: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) error: Option<ControlError>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) wait: Option<WaitDescriptor>,
}

impl ControlReceipt {
    pub(crate) fn accepted(request: &ControlRequest) -> Self {
        Self {
            schema_version: CONTROL_CONTRACT_SCHEMA_VERSION,
            request_id: request.request_id.clone(),
            operation_id: request.operation_id.clone(),
            payload_fingerprint: request.payload_fingerprint.clone(),
            outcome: ReceiptOutcome::Accepted,
            resolved: None,
            before_position: None,
            after_position: None,
            result: None,
            error: None,
            wait: None,
        }
    }

    pub(crate) fn rejected(request: &ControlRequest, error: ControlError) -> Self {
        Self {
            error: Some(error),
            outcome: ReceiptOutcome::NoOp,
            ..Self::accepted(request)
        }
    }

    #[cfg(test)]
    pub(crate) fn outcome_unknown(request: &ControlRequest, error: ControlError) -> Self {
        Self {
            error: Some(error),
            outcome: ReceiptOutcome::OutcomeUnknown,
            ..Self::accepted(request)
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DedupeKey {
    operation_id: OperationId,
    payload_fingerprint: PayloadFingerprint,
}

#[derive(Clone, Debug)]
struct ReplayEntry {
    request_id: RequestId,
    key: DedupeKey,
    receipt: ControlReceipt,
    finalized: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Admission {
    Execute { accepted: ControlReceipt },
    Replay { receipt: ControlReceipt },
    Reject { receipt: ControlReceipt },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum CompletionError {
    UnknownRequest,
    RequestMismatch,
    AlreadyFinalized,
    NonFinalOutcome,
    ExpectedAcceptedOutcome,
}

impl fmt::Display for CompletionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::UnknownRequest => "request is not present in the replay window",
            Self::RequestMismatch => "receipt does not match the admitted request",
            Self::AlreadyFinalized => "request already has a final receipt",
            Self::NonFinalOutcome => "completion receipt cannot remain accepted",
            Self::ExpectedAcceptedOutcome => "in-flight receipt must remain accepted",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for CompletionError {}

#[derive(Debug)]
pub(crate) struct ReplayWindow {
    capacity: usize,
    entries: VecDeque<ReplayEntry>,
}

impl ReplayWindow {
    pub(crate) fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            entries: VecDeque::with_capacity(capacity.max(1)),
        }
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    pub(crate) fn admit(&mut self, request: &ControlRequest, now_unix_ms: u64) -> Admission {
        if let Some(entry) = self
            .entries
            .iter()
            .find(|entry| entry.request_id == request.request_id)
        {
            if entry.key == request.dedupe_key() {
                return Admission::Replay {
                    receipt: entry.receipt.clone(),
                };
            }
            return Admission::Reject {
                receipt: ControlReceipt::rejected(
                    request,
                    ControlError::new(
                        "request_id_conflict",
                        ErrorCategory::Conflict,
                        "request_id was already used for a different request",
                        false,
                    ),
                ),
            };
        }

        if request.mutation_is_expired(now_unix_ms) {
            return Admission::Reject {
                receipt: ControlReceipt::rejected(
                    request,
                    ControlError::new(
                        "request_deadline_expired",
                        ErrorCategory::Timeout,
                        "mutation deadline expired before execution",
                        false,
                    ),
                ),
            };
        }

        if self.entries.len() == self.capacity {
            if let Some(position) = self.entries.iter().position(|entry| entry.finalized) {
                self.entries.remove(position);
            } else {
                return Admission::Reject {
                    receipt: ControlReceipt::rejected(
                        request,
                        ControlError::new(
                            "replay_window_saturated",
                            ErrorCategory::Availability,
                            "replay window is full of requests still being processed",
                            true,
                        ),
                    ),
                };
            }
        }

        let accepted = ControlReceipt::accepted(request);
        self.entries.push_back(ReplayEntry {
            request_id: request.request_id.clone(),
            key: request.dedupe_key(),
            receipt: accepted.clone(),
            finalized: false,
        });
        Admission::Execute { accepted }
    }

    pub(crate) fn complete(
        &mut self,
        request: &ControlRequest,
        receipt: ControlReceipt,
    ) -> Result<(), CompletionError> {
        let Some(entry) = self
            .entries
            .iter_mut()
            .find(|entry| entry.request_id == request.request_id)
        else {
            return Err(CompletionError::UnknownRequest);
        };
        if entry.key != request.dedupe_key()
            || receipt.request_id != request.request_id
            || receipt.operation_id != request.operation_id
            || receipt.payload_fingerprint != request.payload_fingerprint
        {
            return Err(CompletionError::RequestMismatch);
        }
        if entry.finalized {
            return Err(CompletionError::AlreadyFinalized);
        }
        if receipt.outcome == ReceiptOutcome::Accepted {
            return Err(CompletionError::NonFinalOutcome);
        }
        entry.receipt = receipt;
        entry.finalized = true;
        Ok(())
    }

    pub(crate) fn update_accepted(
        &mut self,
        request: &ControlRequest,
        receipt: ControlReceipt,
    ) -> Result<(), CompletionError> {
        let Some(entry) = self
            .entries
            .iter_mut()
            .find(|entry| entry.request_id == request.request_id)
        else {
            return Err(CompletionError::UnknownRequest);
        };
        if entry.key != request.dedupe_key()
            || receipt.request_id != request.request_id
            || receipt.operation_id != request.operation_id
            || receipt.payload_fingerprint != request.payload_fingerprint
        {
            return Err(CompletionError::RequestMismatch);
        }
        if entry.finalized {
            return Err(CompletionError::AlreadyFinalized);
        }
        if receipt.outcome != ReceiptOutcome::Accepted {
            return Err(CompletionError::ExpectedAcceptedOutcome);
        }
        entry.receipt = receipt;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(id: &str, payload: &[u8], intent: RequestIntent) -> ControlRequest {
        ControlRequest::new(
            RequestId::new(id).unwrap(),
            OperationId::new("terminal.send").unwrap(),
            PayloadFingerprint::from_bytes(payload),
            intent,
            Some(2_000),
        )
    }

    fn committed(request: &ControlRequest, sequence: u64) -> ControlReceipt {
        ControlReceipt {
            schema_version: CONTROL_CONTRACT_SCHEMA_VERSION,
            request_id: request.request_id.clone(),
            operation_id: request.operation_id.clone(),
            payload_fingerprint: request.payload_fingerprint.clone(),
            outcome: ReceiptOutcome::Committed,
            resolved: Some(ResolvedTarget {
                server_pid: 42,
                server_address: "127.0.0.1:48913".to_owned(),
                server_epoch: "epoch-a".to_owned(),
                session: "agenterm".to_owned(),
                tab_id: Some(7),
            }),
            before_position: Some(EventPosition {
                epoch: "epoch-a".to_owned(),
                sequence: sequence - 1,
            }),
            after_position: Some(EventPosition {
                epoch: "epoch-a".to_owned(),
                sequence,
            }),
            result: Some(serde_json::json!({"written": true})),
            error: None,
            wait: None,
        }
    }

    #[test]
    fn identifiers_are_bounded_and_wire_safe() {
        assert!(RequestId::new("request-1:a").is_ok());
        assert!(RequestId::new("").is_err());
        assert!(RequestId::new("contains space").is_err());
        assert!(OperationId::new("x".repeat(129)).is_err());
    }

    #[test]
    fn payload_fingerprint_is_deterministic_and_payload_sensitive() {
        let first = PayloadFingerprint::from_bytes(b"send:hello");
        assert_eq!(first, PayloadFingerprint::from_bytes(b"send:hello"));
        assert_ne!(first, PayloadFingerprint::from_bytes(b"send:world"));
        assert!(first.as_str().starts_with("fnv1a64:"));
    }

    #[test]
    fn same_id_and_fingerprint_replays_without_second_execution() {
        let request = request("request-1", b"hello", RequestIntent::Mutation);
        let mut window = ReplayWindow::new(4);
        assert!(matches!(
            window.admit(&request, 1_000),
            Admission::Execute { .. }
        ));

        let receipt = committed(&request, 9);
        window.complete(&request, receipt.clone()).unwrap();
        assert_eq!(window.admit(&request, 1_100), Admission::Replay { receipt });
    }

    #[test]
    fn retry_deadline_does_not_change_an_existing_request_identity() {
        let request = request("request-1", b"hello", RequestIntent::Mutation);
        let mut retry = request.clone();
        retry.deadline_unix_ms = Some(9_000);
        let mut window = ReplayWindow::new(4);
        assert!(matches!(
            window.admit(&request, 1_000),
            Admission::Execute { .. }
        ));
        let receipt = committed(&request, 9);
        window.complete(&request, receipt.clone()).unwrap();

        assert_eq!(window.admit(&retry, 8_000), Admission::Replay { receipt });
    }

    #[test]
    fn same_id_with_different_payload_is_a_conflict() {
        let first = request("request-1", b"hello", RequestIntent::Mutation);
        let second = request("request-1", b"world", RequestIntent::Mutation);
        let mut window = ReplayWindow::new(4);
        assert!(matches!(
            window.admit(&first, 1_000),
            Admission::Execute { .. }
        ));

        let Admission::Reject { receipt } = window.admit(&second, 1_000) else {
            panic!("different payload was not rejected");
        };
        assert_eq!(receipt.outcome, ReceiptOutcome::NoOp);
        assert_eq!(receipt.error.unwrap().code, "request_id_conflict");
    }

    #[test]
    fn expired_mutation_is_rejected_without_reserving_the_id() {
        let expired = request("request-1", b"hello", RequestIntent::Mutation);
        let mut window = ReplayWindow::new(4);
        let Admission::Reject { receipt } = window.admit(&expired, 2_000) else {
            panic!("expired mutation was not rejected");
        };
        assert_eq!(receipt.outcome, ReceiptOutcome::NoOp);
        assert_eq!(receipt.error.unwrap().code, "request_deadline_expired");
        assert_eq!(window.len(), 0);
    }

    #[test]
    fn query_deadline_does_not_trigger_mutation_guard() {
        let query = request("request-1", b"snapshot", RequestIntent::Query);
        let mut window = ReplayWindow::new(4);
        assert!(matches!(
            window.admit(&query, 2_000),
            Admission::Execute { .. }
        ));
    }

    #[test]
    fn bounded_window_evicts_only_finalized_entries() {
        let first = request("request-1", b"one", RequestIntent::Mutation);
        let second = request("request-2", b"two", RequestIntent::Mutation);
        let mut window = ReplayWindow::new(1);
        assert!(matches!(
            window.admit(&first, 1_000),
            Admission::Execute { .. }
        ));

        let Admission::Reject { receipt } = window.admit(&second, 1_000) else {
            panic!("in-flight entry was evicted");
        };
        assert_eq!(receipt.error.unwrap().code, "replay_window_saturated");

        window.complete(&first, committed(&first, 2)).unwrap();
        assert!(matches!(
            window.admit(&second, 1_000),
            Admission::Execute { .. }
        ));
        assert_eq!(window.len(), 1);
    }

    #[test]
    fn completion_must_be_final_and_match_the_admitted_request() {
        let first = request("request-1", b"one", RequestIntent::Mutation);
        let other = request("request-2", b"two", RequestIntent::Mutation);
        let mut window = ReplayWindow::new(2);
        window.admit(&first, 1_000);

        assert_eq!(
            window.complete(&first, ControlReceipt::accepted(&first)),
            Err(CompletionError::NonFinalOutcome)
        );
        assert_eq!(
            window.complete(&first, committed(&other, 2)),
            Err(CompletionError::RequestMismatch)
        );
        window.complete(&first, committed(&first, 2)).unwrap();
        assert_eq!(
            window.complete(&first, committed(&first, 3)),
            Err(CompletionError::AlreadyFinalized)
        );
    }

    #[test]
    fn accepted_receipt_can_be_enriched_before_final_completion() {
        let request = request("request-1", b"one", RequestIntent::Mutation);
        let mut window = ReplayWindow::new(2);
        window.admit(&request, 1_000);

        let mut accepted = ControlReceipt::accepted(&request);
        accepted.wait = Some(WaitDescriptor {
            condition: WaitCondition::SubmissionComplete,
            target: ResolvedTarget {
                server_pid: 42,
                server_address: "127.0.0.1:48913".to_owned(),
                server_epoch: "epoch-a".to_owned(),
                session: "agenterm".to_owned(),
                tab_id: Some(7),
            },
            minimum_position: EventPosition {
                epoch: "epoch-a".to_owned(),
                sequence: 8,
            },
            event_kind: Some("composer.submission-finished".to_owned()),
            deadline_unix_ms: Some(2_000),
        });
        window.update_accepted(&request, accepted.clone()).unwrap();
        assert_eq!(
            window.admit(&request, 1_100),
            Admission::Replay { receipt: accepted }
        );

        let final_receipt = committed(&request, 9);
        window.complete(&request, final_receipt.clone()).unwrap();
        assert_eq!(
            window.admit(&request, 1_200),
            Admission::Replay {
                receipt: final_receipt
            }
        );
    }

    #[test]
    fn receipt_roundtrip_preserves_resolution_positions_error_and_wait() {
        let request = request("request-1", b"hello", RequestIntent::Mutation);
        let target = ResolvedTarget {
            server_pid: 42,
            server_address: "127.0.0.1:48913".to_owned(),
            server_epoch: "epoch-a".to_owned(),
            session: "agenterm".to_owned(),
            tab_id: Some(7),
        };
        let mut receipt = ControlReceipt::outcome_unknown(
            &request,
            ControlError::new(
                "command_outcome_unknown",
                ErrorCategory::Timeout,
                "response deadline elapsed after dispatch",
                false,
            )
            .with_details(serde_json::json!({"phase": "dispatched"})),
        );
        receipt.resolved = Some(target.clone());
        receipt.before_position = Some(EventPosition {
            epoch: "epoch-a".to_owned(),
            sequence: 8,
        });
        receipt.wait = Some(WaitDescriptor {
            condition: WaitCondition::SubmissionComplete,
            target,
            minimum_position: EventPosition {
                epoch: "epoch-a".to_owned(),
                sequence: 8,
            },
            event_kind: Some("terminal.submission-complete".to_owned()),
            deadline_unix_ms: Some(2_000),
        });

        let encoded = serde_json::to_string(&receipt).unwrap();
        assert!(encoded.contains(r#""outcome":"outcome_unknown""#));
        let decoded: ControlReceipt = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, receipt);
    }

    #[test]
    fn missing_schema_version_defaults_to_current_contract() {
        let request = request("request-1", b"hello", RequestIntent::Mutation);
        let mut value = serde_json::to_value(request).unwrap();
        value.as_object_mut().unwrap().remove("schema_version");
        let decoded: ControlRequest = serde_json::from_value(value).unwrap();
        assert_eq!(decoded.schema_version, CONTROL_CONTRACT_SCHEMA_VERSION);
    }

    #[test]
    fn every_receipt_outcome_has_a_stable_wire_name() {
        let names = [
            (ReceiptOutcome::Committed, r#""committed""#),
            (ReceiptOutcome::Accepted, r#""accepted""#),
            (ReceiptOutcome::NoOp, r#""no_op""#),
            (ReceiptOutcome::OutcomeUnknown, r#""outcome_unknown""#),
        ];
        for (outcome, expected) in names {
            assert_eq!(serde_json::to_string(&outcome).unwrap(), expected);
        }
    }
}
