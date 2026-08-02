use std::collections::{HashMap, VecDeque};

use serde::{Deserialize, Serialize};

use crate::protocol::IpcResponse;

pub(crate) const UI_CLIENT_COMMAND_SCHEMA_VERSION: u32 = 1;
pub(crate) const UI_CLIENT_COMMAND_QUEUE_LIMIT: usize = 64;
pub(crate) const UI_CLIENT_COMMAND_MAX_ARGUMENTS: usize = 64;
pub(crate) const UI_CLIENT_COMMAND_MAX_BYTES: usize = 256 * 1024;
pub(crate) const UI_CLIENT_COMMAND_RESPONSE_MAX_BYTES: usize = 1024 * 1024;
pub(crate) const UI_CLIENT_COMMAND_FOCUS: &str = "__focus";
pub(crate) const UI_CLIENT_COMMAND_SHOW_NO_ACTIVATE: &str = "__show-no-activate";

pub(crate) fn is_ui_client_handoff_command(args: &[String]) -> bool {
    matches!(
        args.first().map(String::as_str),
        Some(UI_CLIENT_COMMAND_FOCUS) | Some(UI_CLIENT_COMMAND_SHOW_NO_ACTIVATE)
    )
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct UiClientCommand {
    pub(crate) schema_version: u32,
    pub(crate) command_id: String,
    pub(crate) args: Vec<String>,
}

pub(crate) struct UiClientCommandQueue {
    identity: String,
    next_id: u64,
    pending: VecDeque<UiClientCommand>,
    in_flight: HashMap<String, UiClientCommand>,
    preapplied: HashMap<String, String>,
    completed: VecDeque<(String, String)>,
    completion_inputs: HashMap<String, String>,
}

impl UiClientCommandQueue {
    pub(crate) fn new(identity: String) -> Self {
        Self {
            identity,
            next_id: 1,
            pending: VecDeque::new(),
            in_flight: HashMap::new(),
            preapplied: HashMap::new(),
            completed: VecDeque::new(),
            completion_inputs: HashMap::new(),
        }
    }

    pub(crate) fn enqueue(&mut self, args: Vec<String>) -> Result<String, String> {
        validate_args(&args)?;
        if self.pending.len() + self.in_flight.len() >= UI_CLIENT_COMMAND_QUEUE_LIMIT {
            return Err("UI client command queue is full".to_owned());
        }
        let command_id = format!("{}-{}", self.identity, self.next_id);
        self.next_id = self.next_id.saturating_add(1);
        self.pending.push_back(UiClientCommand {
            schema_version: UI_CLIENT_COMMAND_SCHEMA_VERSION,
            command_id: command_id.clone(),
            args,
        });
        Ok(command_id)
    }

    pub(crate) fn poll(&mut self) -> Option<UiClientCommand> {
        let command = self.pending.pop_front()?;
        self.in_flight
            .insert(command.command_id.clone(), command.clone());
        Some(command)
    }

    pub(crate) fn in_flight(&self, command_id: &str) -> Option<&UiClientCommand> {
        self.in_flight.get(command_id)
    }

    pub(crate) fn record_preapplied(
        &mut self,
        command_id: &str,
        response: &IpcResponse,
    ) -> Result<(), String> {
        if !self
            .pending
            .iter()
            .any(|command| command.command_id == command_id)
        {
            return Err("UI client command is not pending".to_owned());
        }
        let encoded = serde_json::to_string(response)
            .map_err(|error| format!("UI client preapply response is invalid: {error}"))?;
        if encoded.len() > UI_CLIENT_COMMAND_RESPONSE_MAX_BYTES {
            return Err("UI client preapply response exceeds its byte budget".to_owned());
        }
        self.preapplied.insert(command_id.to_owned(), encoded);
        Ok(())
    }

    pub(crate) fn discard_pending(&mut self, command_id: &str) -> bool {
        let Some(position) = self
            .pending
            .iter()
            .position(|command| command.command_id == command_id)
        else {
            return false;
        };
        self.pending.remove(position);
        self.preapplied.remove(command_id);
        true
    }

    pub(crate) fn preapplied(&self, command_id: &str) -> Result<Option<IpcResponse>, String> {
        self.preapplied
            .get(command_id)
            .map(|encoded| {
                serde_json::from_str(encoded)
                    .map_err(|error| format!("UI client preapply response is invalid: {error}"))
            })
            .transpose()
    }

    pub(crate) fn complete(
        &mut self,
        command_id: &str,
        response_json: String,
    ) -> Result<IpcResponse, String> {
        if response_json.is_empty() || response_json.len() > UI_CLIENT_COMMAND_RESPONSE_MAX_BYTES {
            return Err(format!(
                "UI client command response must contain 1..={UI_CLIENT_COMMAND_RESPONSE_MAX_BYTES} bytes"
            ));
        }
        let response = serde_json::from_str::<IpcResponse>(&response_json)
            .map_err(|error| format!("UI client command response is invalid: {error}"))?;
        if let Some(previous_input) = self.completion_inputs.get(command_id) {
            if previous_input != &response_json {
                return Err("UI client command completion replay changed its response".to_owned());
            }
            let completed = self
                .completed
                .iter()
                .find(|(candidate, _)| candidate == command_id)
                .ok_or_else(|| "UI client command completion replay expired".to_owned())?;
            return serde_json::from_str::<IpcResponse>(&completed.1)
                .map_err(|error| format!("UI client command response is invalid: {error}"));
        }
        if self.in_flight.remove(command_id).is_none() {
            return Err("UI client command is not in flight".to_owned());
        }
        self.preapplied.remove(command_id);
        self.completion_inputs
            .insert(command_id.to_owned(), response_json.clone());
        self.completed
            .push_back((command_id.to_owned(), response_json));
        while self.completed.len() > UI_CLIENT_COMMAND_QUEUE_LIMIT {
            if let Some((expired_id, _)) = self.completed.pop_front() {
                self.completion_inputs.remove(&expired_id);
            }
        }
        Ok(response)
    }

    pub(crate) fn result(&self, command_id: &str) -> UiClientCommandResult<'_> {
        if let Some((_, response_json)) = self
            .completed
            .iter()
            .find(|(candidate, _)| candidate == command_id)
        {
            UiClientCommandResult::Complete(response_json)
        } else if self.in_flight.contains_key(command_id) {
            UiClientCommandResult::InFlight
        } else if self
            .pending
            .iter()
            .any(|command| command.command_id == command_id)
        {
            UiClientCommandResult::Pending
        } else {
            UiClientCommandResult::Unknown
        }
    }

    pub(crate) fn replace_completed(
        &mut self,
        command_id: &str,
        response: &IpcResponse,
    ) -> Result<(), String> {
        let encoded = serde_json::to_string(response)
            .map_err(|error| format!("UI client command response is invalid: {error}"))?;
        if encoded.len() > UI_CLIENT_COMMAND_RESPONSE_MAX_BYTES {
            return Err("UI client command response exceeds its byte budget".to_owned());
        }
        let Some((_, response_json)) = self
            .completed
            .iter_mut()
            .find(|(candidate, _)| candidate == command_id)
        else {
            return Err("UI client command completion is unavailable".to_owned());
        };
        *response_json = encoded;
        Ok(())
    }

    pub(crate) fn clear_active(&mut self) {
        self.pending.clear();
        self.in_flight.clear();
        self.preapplied.clear();
    }
}

pub(crate) enum UiClientCommandResult<'a> {
    Pending,
    InFlight,
    Complete(&'a str),
    Unknown,
}

fn validate_args(args: &[String]) -> Result<(), String> {
    if args.is_empty() || args.len() > UI_CLIENT_COMMAND_MAX_ARGUMENTS {
        return Err(format!(
            "UI client command requires 1..={UI_CLIENT_COMMAND_MAX_ARGUMENTS} arguments"
        ));
    }
    let total = args
        .iter()
        .try_fold(0_usize, |total, value| total.checked_add(value.len()))
        .ok_or_else(|| "UI client command byte count overflowed".to_owned())?;
    if total > UI_CLIENT_COMMAND_MAX_BYTES || args.iter().any(|value| value.contains('\0')) {
        return Err(format!(
            "UI client command exceeds its {UI_CLIENT_COMMAND_MAX_BYTES}-byte budget or contains NUL"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_has_bounded_pending_in_flight_and_completed_states() {
        let mut queue = UiClientCommandQueue::new("epoch".to_owned());
        let id = queue
            .enqueue(vec!["ui-action".to_owned(), "toggle-tabs".to_owned()])
            .unwrap();
        assert!(matches!(queue.result(&id), UiClientCommandResult::Pending));
        let command = queue.poll().unwrap();
        assert_eq!(command.command_id, id);
        assert!(matches!(queue.result(&id), UiClientCommandResult::InFlight));
        queue
            .record_preapplied(&id, &IpcResponse::success("preapplied"))
            .unwrap_err();
        let response_json = serde_json::to_string(&IpcResponse::success("{}")).unwrap();
        let response = queue.complete(&id, response_json.clone()).unwrap();
        assert!(response.ok);
        assert!(queue.complete(&id, response_json).unwrap().ok);
        assert!(
            queue
                .complete(
                    &id,
                    serde_json::to_string(&IpcResponse::success("changed")).unwrap()
                )
                .is_err()
        );
        assert!(matches!(
            queue.result(&id),
            UiClientCommandResult::Complete(_)
        ));
    }

    #[test]
    fn queue_retains_bounded_preapply_until_completion() {
        let mut queue = UiClientCommandQueue::new("epoch".to_owned());
        let id = queue
            .enqueue(vec!["focus".to_owned(), "tabs".to_owned()])
            .unwrap();
        queue
            .record_preapplied(&id, &IpcResponse::success("preapplied"))
            .unwrap();
        assert_eq!(queue.preapplied(&id).unwrap().unwrap().output, "preapplied");
        queue.poll().unwrap();
        let response_json = serde_json::to_string(&IpcResponse::success("{}")).unwrap();
        queue.complete(&id, response_json).unwrap();
        assert!(queue.preapplied(&id).unwrap().is_none());
    }

    #[test]
    fn queue_rejects_oversize_and_unknown_completion() {
        let mut queue = UiClientCommandQueue::new("epoch".to_owned());
        assert!(
            queue
                .enqueue(vec!["x".repeat(UI_CLIENT_COMMAND_MAX_BYTES + 1)])
                .is_err()
        );
        assert!(
            queue
                .complete(
                    "missing",
                    serde_json::to_string(&IpcResponse::success("")).unwrap()
                )
                .is_err()
        );
    }

    #[test]
    fn handoff_commands_are_normatively_identified_by_shared_helper() {
        let focus = vec![UI_CLIENT_COMMAND_FOCUS.to_owned()];
        let show = vec![UI_CLIENT_COMMAND_SHOW_NO_ACTIVATE.to_owned()];
        let other = vec!["focus".to_owned()];
        assert!(is_ui_client_handoff_command(&focus));
        assert!(is_ui_client_handoff_command(&show));
        assert!(!is_ui_client_handoff_command(&other));
    }
}
