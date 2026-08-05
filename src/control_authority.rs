use std::collections::HashMap;

use crate::{
    commands::option_value,
    control_contract::{
        Admission, CompletionError, ControlError, ControlReceipt, ControlRequest, ErrorCategory,
        EventPosition as ControlEventPosition, ReceiptOutcome, ReplayWindow, ResolvedTarget,
        WaitCondition, WaitDescriptor,
    },
    control_dispatch::{ControlHost, resolve_target_position},
    event_journal::{EventJournal, EventKind},
    protocol::IpcResponse,
};

const CONTROL_REPLAY_CAPACITY: usize = 512;

struct PendingControlReceipt {
    request: ControlRequest,
    receipt: ControlReceipt,
}

pub(crate) enum ControlAdmission {
    Uncontrolled,
    Execute(ControlRequest),
    Respond(Box<IpcResponse>),
}

pub(crate) struct ControlAuthority {
    replay_window: ReplayWindow,
    pending_submissions: HashMap<u64, PendingControlReceipt>,
}

impl Default for ControlAuthority {
    fn default() -> Self {
        Self {
            replay_window: ReplayWindow::new(CONTROL_REPLAY_CAPACITY),
            pending_submissions: HashMap::new(),
        }
    }
}

impl ControlAuthority {
    pub(crate) fn admit(
        &mut self,
        control: Option<ControlRequest>,
        args: &[String],
        now_unix_ms: u64,
    ) -> ControlAdmission {
        let Some(control) = control else {
            return ControlAdmission::Uncontrolled;
        };
        if control.schema_version != crate::control_contract::CONTROL_CONTRACT_SCHEMA_VERSION {
            return ControlAdmission::Respond(Box::new(IpcResponse::typed_failure(
                "unsupported control contract schema version",
                "control_schema_unsupported",
                "unsupported",
                false,
            )));
        }
        let expected_identity = crate::client::control_request_identity(args);
        let expected_fingerprint = crate::client::control_payload_fingerprint(args);
        let identity_matches = expected_identity.as_ref().is_ok_and(|(operation, intent)| {
            operation == &control.operation_id && intent == &control.intent
        });
        let fingerprint_matches = expected_fingerprint
            .as_ref()
            .is_ok_and(|fingerprint| fingerprint == &control.payload_fingerprint);
        if !identity_matches || !fingerprint_matches {
            let error = ControlError::new(
                "control_request_mismatch",
                ErrorCategory::Validation,
                "control metadata does not match the command payload",
                false,
            );
            let receipt = ControlReceipt::rejected(&control, error.clone());
            return ControlAdmission::Respond(Box::new(
                IpcResponse::typed_failure(
                    error.message,
                    error.code,
                    error.category.as_str(),
                    error.retryable,
                )
                .with_receipt(receipt),
            ));
        }

        match self.replay_window.admit(&control, now_unix_ms) {
            Admission::Replay { receipt } | Admission::Reject { receipt } => {
                ControlAdmission::Respond(Box::new(response_from_receipt(receipt)))
            }
            Admission::Execute { .. } => ControlAdmission::Execute(control),
        }
    }

    pub(crate) fn complete(
        &mut self,
        control: ControlRequest,
        response: IpcResponse,
        resolved: ResolvedTarget,
        before_position: ControlEventPosition,
        after_position: ControlEventPosition,
        wait: Option<WaitDescriptor>,
    ) -> IpcResponse {
        let error = (!response.ok).then(|| {
            ControlError::new(
                if response.error_code.is_empty() {
                    "command_outcome_unknown"
                } else {
                    response.error_code.as_str()
                },
                crate::client::error_category_from_wire(&response.error_category),
                response.error.clone(),
                response.retryable,
            )
        });
        let mut outcome = if response.ok {
            ReceiptOutcome::Committed
        } else if matches!(
            error.as_ref().map(|error| error.category),
            Some(
                ErrorCategory::Validation
                    | ErrorCategory::Conflict
                    | ErrorCategory::NotFound
                    | ErrorCategory::Precondition
                    | ErrorCategory::Policy
                    | ErrorCategory::Unsupported
            )
        ) {
            ReceiptOutcome::NoOp
        } else {
            ReceiptOutcome::OutcomeUnknown
        };
        if wait.is_some() {
            outcome = ReceiptOutcome::Accepted;
        }
        let receipt = ControlReceipt {
            schema_version: crate::control_contract::CONTROL_CONTRACT_SCHEMA_VERSION,
            request_id: control.request_id.clone(),
            operation_id: control.operation_id.clone(),
            payload_fingerprint: control.payload_fingerprint.clone(),
            outcome,
            resolved: Some(resolved),
            before_position: Some(before_position),
            after_position: Some(after_position),
            result: response
                .ok
                .then(|| serde_json::json!({ "output": response.output })),
            error,
            wait,
        };
        let completion = if receipt.outcome == ReceiptOutcome::Accepted {
            self.replay_window
                .update_accepted(&control, receipt.clone())
        } else {
            self.replay_window.complete(&control, receipt.clone())
        };
        if let Err(error) = completion {
            return IpcResponse::typed_failure(
                format!("failed to finalize control receipt: {error}"),
                "control_receipt_internal",
                "internal",
                false,
            );
        }
        if receipt.outcome == ReceiptOutcome::Accepted {
            let Some(tab_id) = receipt.resolved.as_ref().and_then(|target| target.tab_id) else {
                return IpcResponse::typed_failure(
                    "accepted control request did not resolve a terminal",
                    "control_receipt_internal",
                    "internal",
                    false,
                );
            };
            self.pending_submissions.insert(
                tab_id,
                PendingControlReceipt {
                    request: control,
                    receipt: receipt.clone(),
                },
            );
        }
        response.with_receipt(receipt)
    }

    pub(crate) fn finish_submission(
        &mut self,
        journal: &mut EventJournal,
        tab_id: u64,
        enter_written: bool,
        terminal_finalized: bool,
    ) -> Result<bool, CompletionError> {
        let Some(pending) = self.pending_submissions.remove(&tab_id) else {
            return Ok(false);
        };
        let completion_event = journal.commit_correlated(
            EventKind::ComposerSubmissionFinished,
            Some(tab_id),
            Some(pending.request.request_id.as_str().to_owned()),
            Some(pending.request.operation_id.as_str().to_owned()),
            serde_json::json!({
                "enter_written": enter_written,
                "terminal_finalized": terminal_finalized,
            }),
        );
        let mut receipt = pending.receipt;
        receipt.after_position = Some(ControlEventPosition {
            epoch: completion_event.epoch,
            sequence: completion_event.sequence,
        });
        receipt.wait = None;
        if enter_written {
            receipt.outcome = ReceiptOutcome::Committed;
        } else {
            receipt.outcome = ReceiptOutcome::OutcomeUnknown;
            receipt.result = None;
            receipt.error = Some(ControlError::new(
                "terminal_submission_incomplete",
                ErrorCategory::Precondition,
                "composer text was written, but the terminal did not accept the final Enter",
                false,
            ));
        }
        self.replay_window.complete(&pending.request, receipt)?;
        Ok(true)
    }
}

pub(crate) fn control_event_position(host: &impl ControlHost) -> ControlEventPosition {
    let position = host.event_journal().position();
    ControlEventPosition {
        epoch: position.epoch,
        sequence: position.sequence,
    }
}

pub(crate) fn resolved_control_target(host: &impl ControlHost, args: &[String]) -> ResolvedTarget {
    let command = args.first().map(String::as_str).unwrap_or_default();
    let tab_scoped = option_value(args, "-t").is_some()
        || matches!(
            command,
            "capture-pane"
                | "display-message"
                | "dump-cells"
                | "focus"
                | "inspect"
                | "kill-window"
                | "rename-window"
                | "scroll-pane"
                | "send-composer"
                | "send-keys"
                | "send-mouse"
                | "set-composer"
                | "set-tab-note"
                | "show-composer"
                | "show-tab-note"
                | "paste-buffer"
                | "pasteb"
        );
    let tab_id = tab_scoped
        .then(|| resolve_target_position(host.tabs(), host.active_id(), option_value(args, "-t")))
        .flatten()
        .map(|position| host.tabs()[position].id);
    let position = host.event_journal().position();
    ResolvedTarget {
        server_pid: std::process::id(),
        server_address: crate::ipc_address(),
        server_epoch: position.epoch,
        session: host.session_name().to_owned(),
        tab_id,
    }
}

pub(crate) fn submission_wait(
    host: &impl ControlHost,
    control: &ControlRequest,
    response_ok: bool,
    resolved: &ResolvedTarget,
    after_position: &ControlEventPosition,
) -> Option<WaitDescriptor> {
    (response_ok && control.operation_id.as_str() == "command.send.composer")
        .then_some(resolved.tab_id)
        .flatten()
        .and_then(|tab_id| {
            let position = host.tabs().iter().position(|tab| tab.id == tab_id)?;
            host.tabs()[position]
                .submission
                .is_pending()
                .then(|| WaitDescriptor {
                    condition: WaitCondition::SubmissionComplete,
                    target: resolved.clone(),
                    minimum_position: after_position.clone(),
                    event_kind: Some("composer.submission-finished".to_owned()),
                    deadline_unix_ms: control.deadline_unix_ms,
                })
        })
}

fn response_from_receipt(receipt: ControlReceipt) -> IpcResponse {
    if let Some(error) = &receipt.error {
        return IpcResponse::typed_failure(
            error.message.clone(),
            error.code.clone(),
            error.category.as_str(),
            error.retryable,
        )
        .with_receipt(receipt);
    }
    let output = receipt
        .result
        .as_ref()
        .and_then(|result| result["output"].as_str())
        .unwrap_or_default()
        .to_owned();
    IpcResponse::success(output).with_receipt(receipt)
}
