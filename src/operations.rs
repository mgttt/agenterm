use serde::Serialize;

use crate::ui_geometry::{TABS_MAX_WIDTH, TABS_MIN_WIDTH};

pub(crate) const OPERATION_CATALOG_SCHEMA_VERSION: u32 = 1;

pub(crate) const UI_TABS_SHOW: &str = "ui.tabs.show";
pub(crate) const UI_TABS_HIDE: &str = "ui.tabs.hide";
pub(crate) const UI_TABS_TOGGLE: &str = "ui.tabs.toggle";
pub(crate) const UI_TABS_SET_WIDTH: &str = "ui.tabs.set-width";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum OperationClass {
    Observe,
    Control,
    Destructive,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct OperationSpec {
    pub(crate) id: &'static str,
    pub(crate) class: OperationClass,
    pub(crate) command: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) action: Option<&'static str>,
    pub(crate) aliases: &'static [&'static str],
}

pub(crate) const OPERATION_CATALOG: &[OperationSpec] = &[
    OperationSpec {
        id: "protocol.info",
        class: OperationClass::Observe,
        command: "protocol-info",
        action: None,
        aliases: &[],
    },
    OperationSpec {
        id: "ui.snapshot",
        class: OperationClass::Observe,
        command: "ui-snapshot",
        action: None,
        aliases: &[],
    },
    OperationSpec {
        id: "events.read",
        class: OperationClass::Observe,
        command: "read-events",
        action: None,
        aliases: &[],
    },
    OperationSpec {
        id: "events.wait",
        class: OperationClass::Observe,
        command: "wait-events",
        action: None,
        aliases: &[],
    },
    OperationSpec {
        id: UI_TABS_SHOW,
        class: OperationClass::Control,
        command: "ui-action",
        action: Some("tabs-show"),
        aliases: &[],
    },
    OperationSpec {
        id: UI_TABS_HIDE,
        class: OperationClass::Control,
        command: "ui-action",
        action: Some("tabs-hide"),
        aliases: &[],
    },
    OperationSpec {
        id: UI_TABS_TOGGLE,
        class: OperationClass::Control,
        command: "ui-action",
        action: Some("tabs-toggle"),
        aliases: &["toggle-tabs"],
    },
    OperationSpec {
        id: UI_TABS_SET_WIDTH,
        class: OperationClass::Control,
        command: "ui-action",
        action: Some("tabs-set-width"),
        aliases: &[],
    },
    OperationSpec {
        id: "server.kill",
        class: OperationClass::Destructive,
        command: "kill-server",
        action: None,
        aliases: &["server-kill"],
    },
    OperationSpec {
        id: "workspace.shutdown",
        class: OperationClass::Destructive,
        command: "shutdown",
        action: None,
        aliases: &[],
    },
];

pub(crate) fn operation_by_id(id: &str) -> Option<&'static OperationSpec> {
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
