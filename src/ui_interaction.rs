use std::collections::BTreeMap;

use crate::ui_bridge::{UI_INPUT_MAX_BYTES, UI_SCREEN_MAX_COLUMNS, UI_SCREEN_MAX_ROWS};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum UiInteraction {
    Select {
        lease_id: String,
        client_pid: u32,
        tab_id: u64,
    },
    Input {
        lease_id: String,
        client_pid: u32,
        tab_id: u64,
        bytes: Vec<u8>,
    },
    Resize {
        lease_id: String,
        client_pid: u32,
        tab_id: u64,
        rows: u16,
        columns: u16,
    },
}

impl UiInteraction {
    pub(crate) fn lease_identity(&self) -> (&str, u32) {
        match self {
            Self::Select {
                lease_id,
                client_pid,
                ..
            }
            | Self::Input {
                lease_id,
                client_pid,
                ..
            }
            | Self::Resize {
                lease_id,
                client_pid,
                ..
            } => (lease_id, *client_pid),
        }
    }

    pub(crate) const fn tab_id(&self) -> u64 {
        match self {
            Self::Select { tab_id, .. }
            | Self::Input { tab_id, .. }
            | Self::Resize { tab_id, .. } => *tab_id,
        }
    }

    pub(crate) const fn action(&self) -> &'static str {
        match self {
            Self::Select { .. } => "select",
            Self::Input { .. } => "input",
            Self::Resize { .. } => "resize",
        }
    }
}

pub(crate) fn parse_ui_interaction(args: &[String]) -> Result<UiInteraction, String> {
    if args.first().map(String::as_str) != Some("ui-interact") {
        return Err("ui_interaction_command_invalid".to_owned());
    }
    let action = args
        .get(1)
        .map(String::as_str)
        .ok_or_else(|| "ui_interaction_action_missing".to_owned())?;
    let options = parse_pairs(&args[2..])?;
    let lease_id = required(&options, "--lease-id")?.to_owned();
    let client_pid = required(&options, "--client-pid")?
        .parse::<u32>()
        .ok()
        .filter(|pid| *pid != 0)
        .ok_or_else(|| "ui_interaction_client_pid_invalid".to_owned())?;
    let tab_id = required(&options, "-t")?
        .strip_prefix('@')
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|id| *id != 0)
        .ok_or_else(|| "ui_interaction_tab_id_invalid".to_owned())?;

    match action {
        "select" => {
            require_keys(&options, &["--lease-id", "--client-pid", "-t"])?;
            Ok(UiInteraction::Select {
                lease_id,
                client_pid,
                tab_id,
            })
        }
        "input" => {
            require_keys(&options, &["--lease-id", "--client-pid", "-t", "--hex"])?;
            let bytes = decode_hex(required(&options, "--hex")?)?;
            Ok(UiInteraction::Input {
                lease_id,
                client_pid,
                tab_id,
                bytes,
            })
        }
        "resize" => {
            require_keys(
                &options,
                &["--lease-id", "--client-pid", "-t", "--rows", "--columns"],
            )?;
            let rows =
                bounded_dimension(required(&options, "--rows")?, UI_SCREEN_MAX_ROWS, "rows")?;
            let columns = bounded_dimension(
                required(&options, "--columns")?,
                UI_SCREEN_MAX_COLUMNS,
                "columns",
            )?;
            Ok(UiInteraction::Resize {
                lease_id,
                client_pid,
                tab_id,
                rows,
                columns,
            })
        }
        _ => Err("ui_interaction_action_invalid".to_owned()),
    }
}

fn parse_pairs(values: &[String]) -> Result<BTreeMap<&str, &str>, String> {
    if !values.len().is_multiple_of(2) {
        return Err("ui_interaction_options_invalid".to_owned());
    }
    let mut options = BTreeMap::new();
    for pair in values.chunks_exact(2) {
        let name = pair[0].as_str();
        if !matches!(
            name,
            "--lease-id" | "--client-pid" | "-t" | "--hex" | "--rows" | "--columns"
        ) {
            return Err("ui_interaction_option_unknown".to_owned());
        }
        if options.insert(name, pair[1].as_str()).is_some() {
            return Err("ui_interaction_option_duplicate".to_owned());
        }
    }
    Ok(options)
}

fn required<'a>(options: &'a BTreeMap<&str, &str>, name: &str) -> Result<&'a str, String> {
    options
        .get(name)
        .copied()
        .ok_or_else(|| format!("ui_interaction_option_missing[{name}]"))
}

fn require_keys(options: &BTreeMap<&str, &str>, expected: &[&str]) -> Result<(), String> {
    if options.len() != expected.len() || expected.iter().any(|name| !options.contains_key(name)) {
        return Err("ui_interaction_options_invalid".to_owned());
    }
    Ok(())
}

fn bounded_dimension(value: &str, maximum: u32, name: &str) -> Result<u16, String> {
    value
        .parse::<u16>()
        .ok()
        .filter(|value| *value != 0 && u32::from(*value) <= maximum)
        .ok_or_else(|| format!("ui_interaction_{name}_invalid"))
}

fn decode_hex(value: &str) -> Result<Vec<u8>, String> {
    if value.is_empty() || !value.len().is_multiple_of(2) || value.len() / 2 > UI_INPUT_MAX_BYTES {
        return Err("ui_interaction_input_invalid".to_owned());
    }
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let high = hex_digit(pair[0])?;
            let low = hex_digit(pair[1])?;
            Ok((high << 4) | low)
        })
        .collect()
}

fn hex_digit(value: u8) -> Result<u8, String> {
    match value {
        b'0'..=b'9' => Ok(value - b'0'),
        b'a'..=b'f' => Ok(value - b'a' + 10),
        b'A'..=b'F' => Ok(value - b'A' + 10),
        _ => Err("ui_interaction_input_invalid".to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn parses_stable_select_binary_input_and_bounded_resize() {
        assert_eq!(
            parse_ui_interaction(&args(&[
                "ui-interact",
                "select",
                "--lease-id",
                "lease",
                "--client-pid",
                "42",
                "-t",
                "@7",
            ]))
            .unwrap(),
            UiInteraction::Select {
                lease_id: "lease".to_owned(),
                client_pid: 42,
                tab_id: 7,
            }
        );
        assert_eq!(
            parse_ui_interaction(&args(&[
                "ui-interact",
                "input",
                "-t",
                "@7",
                "--hex",
                "410dff",
                "--client-pid",
                "42",
                "--lease-id",
                "lease",
            ]))
            .unwrap(),
            UiInteraction::Input {
                lease_id: "lease".to_owned(),
                client_pid: 42,
                tab_id: 7,
                bytes: vec![b'A', b'\r', 0xff],
            }
        );
        assert!(matches!(
            parse_ui_interaction(&args(&[
                "ui-interact",
                "resize",
                "--rows",
                "30",
                "--columns",
                "100",
                "-t",
                "@7",
                "--lease-id",
                "lease",
                "--client-pid",
                "42",
            ]))
            .unwrap(),
            UiInteraction::Resize {
                rows: 30,
                columns: 100,
                ..
            }
        ));
    }

    #[test]
    fn rejects_mutable_targets_duplicates_unknown_options_and_unbounded_values() {
        for invalid in [
            args(&[
                "ui-interact",
                "select",
                "--lease-id",
                "lease",
                "--client-pid",
                "42",
                "-t",
                "name",
            ]),
            args(&[
                "ui-interact",
                "input",
                "--lease-id",
                "lease",
                "--client-pid",
                "42",
                "-t",
                "@7",
                "--hex",
                "xyz",
            ]),
            args(&[
                "ui-interact",
                "resize",
                "--lease-id",
                "lease",
                "--client-pid",
                "42",
                "-t",
                "@7",
                "--rows",
                "0",
                "--columns",
                "100",
            ]),
            args(&[
                "ui-interact",
                "select",
                "--lease-id",
                "lease",
                "--lease-id",
                "other",
                "-t",
                "@7",
            ]),
        ] {
            assert!(parse_ui_interaction(&invalid).is_err());
        }
    }
}
