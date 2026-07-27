use serde::Serialize;

use crate::ui_geometry::{TABS_MAX_WIDTH, TABS_MIN_WIDTH};

pub const OPERATION_CATALOG_SCHEMA_VERSION: u32 = 1;

pub const UI_TABS_SHOW: &str = "ui.tabs.show";
pub const UI_TABS_HIDE: &str = "ui.tabs.hide";
pub const UI_TABS_TOGGLE: &str = "ui.tabs.toggle";
pub const UI_TABS_SET_WIDTH: &str = "ui.tabs.set-width";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationClass {
    Observe,
    Control,
    Destructive,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct OperationParameterSpec {
    pub name: &'static str,
    pub value_type: &'static str,
    pub required: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub minimum: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maximum: Option<i64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct OperationSpec {
    pub id: &'static str,
    pub class: OperationClass,
    pub command: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<&'static str>,
    pub aliases: &'static [&'static str],
    pub parameters: &'static [OperationParameterSpec],
    pub result_type: &'static str,
    pub errors: &'static [&'static str],
    pub events: &'static [&'static str],
    pub destructive: bool,
    pub available: bool,
    pub since: &'static str,
}

const NO_PARAMETERS: &[OperationParameterSpec] = &[];
const EVENT_POSITION_PARAMETERS: &[OperationParameterSpec] = &[
    OperationParameterSpec {
        name: "epoch",
        value_type: "string",
        required: true,
        minimum: None,
        maximum: None,
    },
    OperationParameterSpec {
        name: "after",
        value_type: "uint64",
        required: true,
        minimum: Some(0),
        maximum: None,
    },
];
const EVENT_READ_PARAMETERS: &[OperationParameterSpec] = &[
    EVENT_POSITION_PARAMETERS[0],
    EVENT_POSITION_PARAMETERS[1],
    OperationParameterSpec {
        name: "limit",
        value_type: "uint32",
        required: false,
        minimum: Some(1),
        maximum: Some(1024),
    },
];
const EVENT_WAIT_PARAMETERS: &[OperationParameterSpec] = &[
    EVENT_POSITION_PARAMETERS[0],
    EVENT_POSITION_PARAMETERS[1],
    OperationParameterSpec {
        name: "kind",
        value_type: "string",
        required: true,
        minimum: None,
        maximum: None,
    },
    OperationParameterSpec {
        name: "tab",
        value_type: "stable_tab_id",
        required: false,
        minimum: None,
        maximum: None,
    },
    OperationParameterSpec {
        name: "timeout_ms",
        value_type: "uint32",
        required: false,
        minimum: Some(0),
        maximum: Some(60_000),
    },
];
const CAPTURE_PARAMETERS: &[OperationParameterSpec] = &[
    OperationParameterSpec {
        name: "tab",
        value_type: "stable_tab_id",
        required: true,
        minimum: None,
        maximum: None,
    },
    OperationParameterSpec {
        name: "max_bytes",
        value_type: "uint32",
        required: true,
        minimum: Some(1),
        maximum: Some(1024 * 1024),
    },
];
const TABS_WIDTH_PARAMETERS: &[OperationParameterSpec] = &[OperationParameterSpec {
    name: "width",
    value_type: "integer",
    required: true,
    minimum: Some(TABS_MIN_WIDTH as i64),
    maximum: Some(TABS_MAX_WIDTH as i64),
}];
const SESSION_TARGET_PARAMETERS: &[OperationParameterSpec] = &[OperationParameterSpec {
    name: "target",
    value_type: "session_name",
    required: false,
    minimum: None,
    maximum: None,
}];

pub const OPERATION_CATALOG: &[OperationSpec] = &[
    OperationSpec {
        id: "protocol.info",
        class: OperationClass::Observe,
        command: "protocol-info",
        action: None,
        aliases: &[],
        parameters: NO_PARAMETERS,
        result_type: "protocol_info",
        errors: &[],
        events: &[],
        destructive: false,
        available: true,
        since: "0.1.5",
    },
    OperationSpec {
        id: "ui.snapshot",
        class: OperationClass::Observe,
        command: "ui-snapshot",
        action: None,
        aliases: &[],
        parameters: NO_PARAMETERS,
        result_type: "ui_snapshot",
        errors: &["server_unavailable"],
        events: &[],
        destructive: false,
        available: true,
        since: "0.1.5",
    },
    OperationSpec {
        id: "workspace.info",
        class: OperationClass::Observe,
        command: "workspace-info",
        action: None,
        aliases: &[],
        parameters: NO_PARAMETERS,
        result_type: "workspace_metadata_with_event_position",
        errors: &["server_unavailable"],
        events: &[],
        destructive: false,
        available: true,
        since: "0.1.6",
    },
    OperationSpec {
        id: "tabs.list",
        class: OperationClass::Observe,
        command: "ui-snapshot",
        action: None,
        aliases: &[],
        parameters: NO_PARAMETERS,
        result_type: "tab_list",
        errors: &["server_unavailable"],
        events: &[],
        destructive: false,
        available: true,
        since: "0.1.6",
    },
    OperationSpec {
        id: "tabs.active",
        class: OperationClass::Observe,
        command: "ui-snapshot",
        action: None,
        aliases: &[],
        parameters: NO_PARAMETERS,
        result_type: "tab_or_null",
        errors: &["server_unavailable"],
        events: &[],
        destructive: false,
        available: true,
        since: "0.1.6",
    },
    OperationSpec {
        id: "pane.capture",
        class: OperationClass::Observe,
        command: "capture-pane",
        action: None,
        aliases: &["capturep"],
        parameters: CAPTURE_PARAMETERS,
        result_type: "bounded_capture",
        errors: &["operation_invalid_arguments", "server_unavailable"],
        events: &[],
        destructive: false,
        available: true,
        since: "0.1.6",
    },
    OperationSpec {
        id: "events.read",
        class: OperationClass::Observe,
        command: "read-events",
        action: None,
        aliases: &[],
        parameters: EVENT_READ_PARAMETERS,
        result_type: "event_batch",
        errors: &[
            "operation_invalid_arguments",
            "server_restart",
            "journal_gap",
            "future_sequence",
        ],
        events: &[],
        destructive: false,
        available: true,
        since: "0.1.5",
    },
    OperationSpec {
        id: "events.wait",
        class: OperationClass::Observe,
        command: "wait-events",
        action: None,
        aliases: &[],
        parameters: EVENT_WAIT_PARAMETERS,
        result_type: "event",
        errors: &[
            "operation_invalid_arguments",
            "event_wait_timeout",
            "server_restart",
            "journal_gap",
            "future_sequence",
        ],
        events: &[],
        destructive: false,
        available: true,
        since: "0.1.5",
    },
    OperationSpec {
        id: UI_TABS_SHOW,
        class: OperationClass::Control,
        command: "ui-action",
        action: Some("tabs-show"),
        aliases: &[],
        parameters: NO_PARAMETERS,
        result_type: "ui_snapshot",
        errors: &["operation_invalid_arguments"],
        events: &["layout.tabs.visibility"],
        destructive: false,
        available: true,
        since: "0.1.6",
    },
    OperationSpec {
        id: UI_TABS_HIDE,
        class: OperationClass::Control,
        command: "ui-action",
        action: Some("tabs-hide"),
        aliases: &[],
        parameters: NO_PARAMETERS,
        result_type: "ui_snapshot",
        errors: &["operation_invalid_arguments"],
        events: &["layout.tabs.visibility"],
        destructive: false,
        available: true,
        since: "0.1.6",
    },
    OperationSpec {
        id: UI_TABS_TOGGLE,
        class: OperationClass::Control,
        command: "ui-action",
        action: Some("tabs-toggle"),
        aliases: &["toggle-tabs"],
        parameters: NO_PARAMETERS,
        result_type: "ui_snapshot",
        errors: &["operation_invalid_arguments"],
        events: &["layout.tabs.visibility"],
        destructive: false,
        available: true,
        since: "0.1.6",
    },
    OperationSpec {
        id: UI_TABS_SET_WIDTH,
        class: OperationClass::Control,
        command: "ui-action",
        action: Some("tabs-set-width"),
        aliases: &[],
        parameters: TABS_WIDTH_PARAMETERS,
        result_type: "ui_snapshot",
        errors: &["operation_invalid_arguments"],
        events: &["layout.tabs.width"],
        destructive: false,
        available: true,
        since: "0.1.6",
    },
    OperationSpec {
        id: "server.kill",
        class: OperationClass::Destructive,
        command: "kill-server",
        action: None,
        aliases: &["server-kill"],
        parameters: SESSION_TARGET_PARAMETERS,
        result_type: "empty",
        errors: &["operation_target_not_found", "server_unavailable"],
        events: &["workspace.shutdown"],
        destructive: true,
        available: true,
        since: "0.1.5",
    },
    OperationSpec {
        id: "workspace.shutdown",
        class: OperationClass::Destructive,
        command: "shutdown",
        action: None,
        aliases: &[],
        parameters: NO_PARAMETERS,
        result_type: "empty",
        errors: &["operation_persistence_failed", "server_unavailable"],
        events: &["workspace.saved", "workspace.shutdown"],
        destructive: true,
        available: true,
        since: "0.1.5",
    },
];

pub fn operation_by_id(id: &str) -> Option<&'static OperationSpec> {
    OPERATION_CATALOG
        .iter()
        .find(|operation| operation.id == id)
}

pub(crate) fn operation_for_args(
    args: &[String],
) -> Result<Option<&'static OperationSpec>, String> {
    let Some(command) = args.first().map(String::as_str) else {
        return Ok(None);
    };
    let operation = match command {
        "protocol-info" => operation_by_id("protocol.info"),
        "ui-snapshot" => operation_by_id("ui.snapshot"),
        "read-events" => operation_by_id("events.read"),
        "wait-events" => operation_by_id("events.wait"),
        "kill-server" | "server-kill" => operation_by_id("server.kill"),
        "shutdown" => operation_by_id("workspace.shutdown"),
        "ui-action" => {
            let Some(action) = args.get(1).map(String::as_str) else {
                return Ok(None);
            };
            let id = match action {
                "tabs-show" => UI_TABS_SHOW,
                "tabs-hide" => UI_TABS_HIDE,
                "tabs-toggle" | "toggle-tabs" => UI_TABS_TOGGLE,
                "tabs-set-width" => UI_TABS_SET_WIDTH,
                action if action.starts_with("tabs-") => {
                    return Err(operation_error(
                        "operation_unknown",
                        action,
                        "unknown typed Tabs action",
                    ));
                }
                _ => return Ok(None),
            };
            operation_by_id(id)
        }
        _ => return Ok(None),
    };
    Ok(operation)
}

pub(crate) fn validate_operation_args(
    args: &[String],
) -> Result<Option<&'static OperationSpec>, String> {
    let operation = operation_for_args(args)?;
    let Some(operation) = operation else {
        return Ok(None);
    };
    if operation.id == UI_TABS_SET_WIDTH {
        if args.len() != 4 {
            return Err(operation_error(
                "operation_invalid_arguments",
                operation.id,
                "accepts exactly --width PX",
            ));
        }
        let Some(raw_width) = option_value(args, "--width") else {
            return Err(operation_error(
                "operation_invalid_arguments",
                operation.id,
                "requires --width PX",
            ));
        };
        let Ok(width) = raw_width.parse::<i32>() else {
            return Err(operation_error(
                "operation_invalid_arguments",
                operation.id,
                "--width must be an integer",
            ));
        };
        if !(TABS_MIN_WIDTH..=TABS_MAX_WIDTH).contains(&width) {
            return Err(operation_error(
                "operation_invalid_arguments",
                operation.id,
                &format!("--width must be from {TABS_MIN_WIDTH} to {TABS_MAX_WIDTH}"),
            ));
        }
    } else if operation.command == "ui-action" && args.len() != 2 {
        return Err(operation_error(
            "operation_invalid_arguments",
            operation.id,
            "does not accept additional arguments",
        ));
    }
    Ok(Some(operation))
}

fn option_value<'a>(args: &'a [String], option: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|pair| pair[0] == option)
        .map(|pair| pair[1].as_str())
}

fn operation_error(code: &str, identity: &str, message: &str) -> String {
    format!("{code}[{identity}]: {message}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn catalog_has_stable_unique_ids_and_all_classes() {
        let mut ids = OPERATION_CATALOG
            .iter()
            .map(|operation| operation.id)
            .collect::<Vec<_>>();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), OPERATION_CATALOG.len());
        assert!(
            OPERATION_CATALOG
                .iter()
                .any(|operation| { operation.class == OperationClass::Observe })
        );
        assert!(
            OPERATION_CATALOG
                .iter()
                .any(|operation| { operation.class == OperationClass::Control })
        );
        assert!(
            OPERATION_CATALOG
                .iter()
                .any(|operation| { operation.class == OperationClass::Destructive })
        );
        for operation in OPERATION_CATALOG {
            assert!(!operation.result_type.is_empty());
            assert!(!operation.since.is_empty());
            assert!(operation.available);
            assert_eq!(
                operation.destructive,
                operation.class == OperationClass::Destructive
            );
            for parameter in operation.parameters {
                assert!(!parameter.name.is_empty());
                assert!(!parameter.value_type.is_empty());
                assert!(
                    parameter
                        .minimum
                        .zip(parameter.maximum)
                        .is_none_or(|(minimum, maximum)| minimum <= maximum)
                );
            }
        }
    }

    #[test]
    fn legacy_toggle_tabs_resolves_to_the_stable_typed_identity() {
        let operation = validate_operation_args(&args(&["ui-action", "toggle-tabs"])).unwrap();
        assert_eq!(
            operation.map(|operation| operation.id),
            Some(UI_TABS_TOGGLE)
        );
    }

    #[test]
    fn validates_typed_tabs_width_boundaries() {
        let width = operation_by_id(UI_TABS_SET_WIDTH).unwrap();
        assert_eq!(width.parameters, TABS_WIDTH_PARAMETERS);
        assert_eq!(width.result_type, "ui_snapshot");
        assert_eq!(width.events, ["layout.tabs.width"]);
        for width in [TABS_MIN_WIDTH, TABS_MAX_WIDTH] {
            let operation = validate_operation_args(&args(&[
                "ui-action",
                "tabs-set-width",
                "--width",
                &width.to_string(),
            ]))
            .unwrap();
            assert_eq!(
                operation.map(|operation| operation.id),
                Some(UI_TABS_SET_WIDTH)
            );
        }
        let error = validate_operation_args(&args(&[
            "ui-action",
            "tabs-set-width",
            "--width",
            &(TABS_MIN_WIDTH - 1).to_string(),
        ]))
        .unwrap_err();
        assert!(error.starts_with("operation_invalid_arguments[ui.tabs.set-width]"));
    }

    #[test]
    fn rejects_unknown_typed_tabs_actions() {
        let error = validate_operation_args(&args(&["ui-action", "tabs-teleport"])).unwrap_err();
        assert_eq!(
            error,
            "operation_unknown[tabs-teleport]: unknown typed Tabs action"
        );
    }
}
