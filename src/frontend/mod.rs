//! Product frontend bootstrap and wakeup dispatcher.
//!
//! This module is the product-layer frontend ingress. `platform` owns native
//! capability adapters; this layer owns the app-level host selection for GUI
//! startup and wake delivery.

use crate::client::{parse_loopback_ipc_address, set_ipc_selectors};
use crate::ipc_endpoint::EndpointSelectorArgs;
use crate::platform::adapters::unix::frontend as unix_frontend;
use crate::platform::adapters::windows::frontend as windows_frontend;
use crate::platform::{FrontendHost, frontend_host};
use crate::ui_command::{UI_CLIENT_COMMAND_FOCUS, UI_CLIENT_COMMAND_SHOW_NO_ACTIVATE};
use crate::wake_signal::WakeSignal;

pub(crate) mod action;
pub(crate) mod control_center;
pub(crate) mod toolbar;
pub(crate) mod window;

// Shared GUI launch usage for both platform frontends.
pub(crate) const WINDOWS_GUI_USAGE: &str = "\
Usage: agenterm.exe [--no-activate] [--endpoint ENDPOINT | --address HOST:PORT | --instance NAME]\n\n\
Options:\n  --endpoint ENDPOINT   Select a typed local IPC endpoint\n  --address HOST:PORT   Select a legacy loopback TCP endpoint\n  --instance NAME       Select a logical instance (main or dev)\n  --no-activate         Open without taking foreground focus\n  --not-foreground      Alias for --no-activate\n  -h, --help            Show this help\n\n\
This binary is the GUI launcher. For command operations use agenterm-cli.exe.";
pub(crate) const UNIX_GUI_USAGE: &str = "\
Usage: agenterm [--no-activate] [--endpoint ENDPOINT | --address HOST:PORT | --instance NAME]\n\
Options:\n  --endpoint ENDPOINT   Select a typed local IPC endpoint\n  --address HOST:PORT   Select a legacy loopback TCP endpoint\n  --instance NAME       Select a logical instance (main or dev)\n  --no-activate         Open without taking foreground focus\n  --not-foreground      Alias for --no-activate\n  -h, --help            Show this help";

pub(crate) const WINDOWS_GUI_CLI_NAME: &str = "agenterm-cli.exe";
pub(crate) const UNIX_GUI_CLI_NAME: &str = "agenterm-cli";
pub(crate) const GUI_CLI_GUIDANCE_MARKER: &str = "gui-cli-guidance";

pub(crate) fn gui_help_result(arguments: &[String], usage: &str) -> Option<GuiLaunchResult> {
    if arguments
        .iter()
        .any(|argument| matches!(argument.as_str(), "-h" | "--help"))
    {
        if arguments.len() == 1 {
            println!("{usage}");
            return Some(GuiLaunchResult::UsageHelpPrinted);
        }
        eprintln!("AgenTerm GUI argument error: --help cannot be combined with other options\n");
        return Some(GuiLaunchResult::UsageError);
    }
    None
}

pub(crate) fn gui_launch_argument_error(
    message: &str,
    usage: &str,
    include_server_hint: bool,
) -> String {
    let mut rendered = format!("AgenTerm GUI argument error: {message}");
    if include_server_hint {
        rendered.push_str(
            "\nNo GUI server was started by this invocation.\nMore CLI commands: agenterm-cli.exe -h",
        );
    }
    if !usage.is_empty() {
        rendered.push('\n');
        rendered.push('\n');
        rendered.push_str(usage);
    }
    rendered
}

fn quote_argument_for_display(argument: &str) -> String {
    if argument.is_empty() || argument.chars().any(char::is_whitespace) {
        format!("\"{}\"", argument.replace('"', "\\\""))
    } else {
        argument.to_owned()
    }
}

pub(crate) fn gui_cli_guidance(
    arguments: &[String],
    command_client_name: &str,
    usage: &str,
) -> String {
    let forwarded = arguments
        .iter()
        .map(|argument| quote_argument_for_display(argument))
        .collect::<Vec<_>>()
        .join(" ");
    format!(
        "AgenTerm GUI entry point\n\n\
         No CLI command was executed and no GUI server was started by this invocation.\n\n\
         Use instead:\n{command_client_name} {forwarded}\n\n\
         List running server PID and port: {command_client_name} server-list\n\
         More CLI commands: {command_client_name} -h\n\n\
         {usage}"
    )
}

pub(crate) fn is_gui_cli_guidance_error(message: &str) -> bool {
    message.starts_with(GUI_CLI_GUIDANCE_MARKER)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum GuiLaunchResult {
    Launched,
    Reused,
    UsageHelpPrinted,
    UnsupportedHost,
    UsageError,
    BlockedByServer(String),
    StartupFailed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum FrontendContractState {
    Supported,
    Unsupported,
    Failed,
    Blocked,
    UsageError,
}

impl GuiLaunchResult {
    #[allow(dead_code)]
    pub(crate) const fn contract_state(&self) -> FrontendContractState {
        match self {
            Self::Launched | Self::Reused | Self::UsageHelpPrinted => {
                FrontendContractState::Supported
            }
            Self::UnsupportedHost => FrontendContractState::Unsupported,
            Self::BlockedByServer(_) => FrontendContractState::Blocked,
            Self::UsageError | Self::StartupFailed(_) => FrontendContractState::Failed,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct GuiLaunchParsePolicy {
    pub launcher_name: &'static str,
    pub allow_ui_client: bool,
    pub validate_address: bool,
}

pub(crate) const WINDOWS_GUI_LAUNCH_POLICY: GuiLaunchParsePolicy = GuiLaunchParsePolicy {
    launcher_name: "agenterm.exe",
    allow_ui_client: true,
    validate_address: true,
};

pub(crate) const UNIX_GUI_LAUNCH_POLICY: GuiLaunchParsePolicy = GuiLaunchParsePolicy {
    launcher_name: "agenterm",
    allow_ui_client: false,
    validate_address: false,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParsedGuiLaunch {
    pub no_activate: bool,
    pub ui_client: bool,
    pub selectors: EndpointSelectorArgs,
}

impl ParsedGuiLaunch {
    const fn new(no_activate: bool, ui_client: bool, selectors: EndpointSelectorArgs) -> Self {
        Self {
            no_activate,
            ui_client,
            selectors,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GuiLaunchOptions {
    pub no_activate: bool,
    pub ui_client: bool,
    pub selectors: EndpointSelectorArgs,
}

pub(crate) fn parse_gui_launch_arguments(
    arguments: &[String],
    policy: GuiLaunchParsePolicy,
) -> Result<ParsedGuiLaunch, String> {
    let mut no_activate = false;
    let mut ui_client = false;
    let mut selectors = EndpointSelectorArgs::default();
    let mut position = 0;

    while position < arguments.len() {
        match arguments[position].as_str() {
            "--no-activate" | "--not-foreground" => {
                if no_activate {
                    return Err(format!(
                        "{name} --no-activate/--not-foreground may be specified only once",
                        name = policy.launcher_name
                    ));
                }
                no_activate = true;
                position += 1;
            }
            "--ui-client" if policy.allow_ui_client => {
                if ui_client {
                    return Err(format!(
                        "{name} --ui-client may be specified only once",
                        name = policy.launcher_name
                    ));
                }
                ui_client = true;
                position += 1;
            }
            "--ui-client" => {
                return Err(format!(
                    "{name} --ui-client may be specified only when supported by platform",
                    name = policy.launcher_name
                ));
            }
            "--address" => {
                if selectors.address.is_some() {
                    return Err(format!(
                        "{name} --address may be specified only once",
                        name = policy.launcher_name
                    ));
                }
                let value = arguments
                    .get(position + 1)
                    .filter(|value| !value.starts_with("--"))
                    .ok_or_else(|| {
                        format!(
                            "{name} --address requires HOST:PORT",
                            name = policy.launcher_name
                        )
                    })?;
                if policy.validate_address {
                    parse_loopback_ipc_address(value).map_err(|error| error.to_string())?;
                }
                selectors.address = Some(value.to_owned());
                position += 2;
            }
            "--endpoint" => {
                if selectors.endpoint.is_some() {
                    return Err(format!(
                        "{name} --endpoint may be specified only once",
                        name = policy.launcher_name
                    ));
                }
                let value = arguments
                    .get(position + 1)
                    .filter(|value| !value.starts_with("--"))
                    .ok_or_else(|| {
                        format!(
                            "{name} --endpoint requires ENDPOINT",
                            name = policy.launcher_name
                        )
                    })?;
                selectors.endpoint = Some(value.to_owned());
                position += 2;
            }
            "--instance" => {
                if selectors.instance.is_some() {
                    return Err(format!(
                        "{name} --instance may be specified only once",
                        name = policy.launcher_name
                    ));
                }
                let value = arguments
                    .get(position + 1)
                    .filter(|value| !value.starts_with("--"))
                    .ok_or_else(|| {
                        format!(
                            "{name} --instance requires NAME",
                            name = policy.launcher_name
                        )
                    })?;
                selectors.instance = Some(value.to_owned());
                position += 2;
            }
            other if other.starts_with('-') => {
                return Err(format!("unsupported option: {other}"));
            }
            other => {
                return Err(format!(
                    "{}: unexpected positional argument: {other}; \
                     the GUI launcher does not accept shell commands",
                    GUI_CLI_GUIDANCE_MARKER
                ));
            }
        }
    }
    let selector_count = [
        selectors.endpoint.is_some(),
        selectors.address.is_some(),
        selectors.instance.is_some(),
    ]
    .into_iter()
    .filter(|selected| *selected)
    .count();
    if selector_count > 1 {
        return Err(format!(
            "{name} --endpoint, --address, and --instance are mutually exclusive",
            name = policy.launcher_name
        ));
    }

    Ok(ParsedGuiLaunch::new(no_activate, ui_client, selectors))
}

pub(crate) fn parse_gui_launch_target(
    arguments: &[String],
    policy: GuiLaunchParsePolicy,
) -> Result<GuiLaunchOptions, String> {
    let ParsedGuiLaunch {
        no_activate,
        ui_client,
        selectors,
    } = parse_gui_launch_arguments(arguments, policy).map_err(|error| error.to_string())?;
    set_ipc_selectors(selectors.clone()).map_err(|error| error.to_string())?;
    Ok(GuiLaunchOptions {
        no_activate,
        ui_client,
        selectors,
    })
}

impl Default for GuiLaunchParsePolicy {
    fn default() -> Self {
        Self {
            launcher_name: "agenterm",
            allow_ui_client: false,
            validate_address: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum GuiHandoffResult {
    HandedOff,
    Continue,
    Blocked(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum GuiWakeResult {
    Woke,
    Throttled,
    Unsupported,
    NoTarget,
    Failed(String),
}

impl GuiWakeResult {
    #[allow(dead_code)]
    pub(crate) const fn contract_state(&self) -> FrontendContractState {
        match self {
            Self::Woke | Self::Throttled => FrontendContractState::Supported,
            Self::Unsupported => FrontendContractState::Unsupported,
            Self::NoTarget | Self::Failed(_) => FrontendContractState::Failed,
        }
    }
}

pub(crate) fn attempt_gui_handoff(
    no_activate: bool,
    skip_when_in_server: bool,
) -> GuiHandoffResult {
    if skip_when_in_server && std::env::var_os("AGENTERM_SERVER").is_some() {
        return GuiHandoffResult::Continue;
    }
    let handoff = if no_activate {
        UI_CLIENT_COMMAND_SHOW_NO_ACTIVATE
    } else {
        UI_CLIENT_COMMAND_FOCUS
    };
    match crate::client::send_ipc_request(vec![handoff.to_owned()]) {
        Ok(response) if response.ok => GuiHandoffResult::HandedOff,
        Ok(response)
            if response.error_code == "ui_client_unavailable"
                || response.error_code == "server_command_unsupported" =>
        {
            GuiHandoffResult::Continue
        }
        Ok(response) => GuiHandoffResult::Blocked(response.error),
        Err(_) => GuiHandoffResult::Continue,
    }
}

/// Start the product frontend for the current platform.
pub fn run_gui_entry() -> i32 {
    match run_gui_entry_result() {
        GuiLaunchResult::Launched | GuiLaunchResult::Reused | GuiLaunchResult::UsageHelpPrinted => {
            0
        }
        GuiLaunchResult::UsageError => 2,
        GuiLaunchResult::UnsupportedHost => {
            eprintln!("AgenTerm GUI is unsupported on this platform");
            1
        }
        GuiLaunchResult::BlockedByServer(error) | GuiLaunchResult::StartupFailed(error) => {
            eprintln!("AgenTerm GUI launch failed: {error}");
            1
        }
    }
}

fn run_gui_entry_result() -> GuiLaunchResult {
    match frontend_host() {
        FrontendHost::Windows => windows_frontend::run_gui_entry_result(),
        FrontendHost::Unix => unix_frontend::run_gui_entry_result(),
        FrontendHost::Unsupported => GuiLaunchResult::UnsupportedHost,
    }
}

#[allow(dead_code)]
fn gui_wake_result_is_terminal(result: &GuiWakeResult) -> bool {
    matches!(
        result,
        GuiWakeResult::Unsupported | GuiWakeResult::Failed(_) | GuiWakeResult::NoTarget
    )
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::{
        FrontendContractState, GuiLaunchResult, GuiWakeResult, UNIX_GUI_LAUNCH_POLICY,
        WINDOWS_GUI_LAUNCH_POLICY, gui_launch_argument_error, gui_wake_result_is_terminal,
        parse_gui_launch_arguments,
    };
    use crate::frontend_server::FrontendServerRecovery;

    #[test]
    fn gui_launch_result_maps_to_exit_code() {
        let map = |result: GuiLaunchResult| match result {
            GuiLaunchResult::Launched
            | GuiLaunchResult::Reused
            | GuiLaunchResult::UsageHelpPrinted => 0,
            GuiLaunchResult::UsageError => 2,
            GuiLaunchResult::UnsupportedHost => 1,
            GuiLaunchResult::BlockedByServer(_) | GuiLaunchResult::StartupFailed(_) => 1,
        };

        assert_eq!(map(GuiLaunchResult::Launched), 0);
        assert_eq!(map(GuiLaunchResult::UsageError), 2);
    }

    #[test]
    fn gui_wake_result_terminal_semantics_are_classified() {
        assert!(!gui_wake_result_is_terminal(&GuiWakeResult::Woke));
        assert!(!gui_wake_result_is_terminal(&GuiWakeResult::Throttled));
        assert!(gui_wake_result_is_terminal(&GuiWakeResult::Unsupported));
        assert!(gui_wake_result_is_terminal(&GuiWakeResult::NoTarget));
        assert!(gui_wake_result_is_terminal(&GuiWakeResult::Failed(
            "x".to_owned()
        )));
    }

    #[test]
    fn launch_and_wake_results_classify_contract_state() {
        assert!(matches!(
            GuiLaunchResult::UnsupportedHost.contract_state(),
            FrontendContractState::Unsupported
        ));
        assert!(matches!(
            GuiLaunchResult::StartupFailed("x".to_owned()).contract_state(),
            FrontendContractState::Failed
        ));
        assert!(matches!(
            GuiWakeResult::Failed("x".to_owned()).contract_state(),
            FrontendContractState::Failed
        ));
        assert!(matches!(
            GuiWakeResult::NoTarget.contract_state(),
            FrontendContractState::Failed
        ));
        assert!(matches!(
            FrontendServerRecovery::Failed("x".to_owned()).contract_state(),
            FrontendContractState::Failed
        ));
    }

    #[test]
    fn shared_gui_launch_parser_accepts_standard_windows_launch_flags() {
        let parsed = parse_gui_launch_arguments(
            &[
                "--no-activate".to_owned(),
                "--address".to_owned(),
                "127.0.0.1:48815".to_owned(),
                "--ui-client".to_owned(),
            ],
            WINDOWS_GUI_LAUNCH_POLICY,
        )
        .expect("windows launch parser should accept shared flags");
        assert!(parsed.no_activate);
        assert!(parsed.ui_client);
        assert_eq!(parsed.selectors.address.as_deref(), Some("127.0.0.1:48815"));
        assert!(parsed.selectors.endpoint.is_none());
        assert!(parsed.selectors.instance.is_none());
    }

    #[test]
    fn shared_gui_launch_parser_rejects_selector_conflicts() {
        let result = parse_gui_launch_arguments(
            &[
                "--endpoint".to_owned(),
                "tcp:127.0.0.1:48815".to_owned(),
                "--address".to_owned(),
                "127.0.0.1:48815".to_owned(),
            ],
            UNIX_GUI_LAUNCH_POLICY,
        );
        assert!(
            result.is_err(),
            "endpoint and address flags should remain mutually exclusive"
        );
    }

    #[test]
    fn shared_gui_launch_parser_rejects_unsupported_ui_client_for_unix() {
        let result =
            parse_gui_launch_arguments(&["--ui-client".to_owned()], UNIX_GUI_LAUNCH_POLICY);
        assert!(
            result.is_err(),
            "ui-client flag should be rejected when platform parser disallows it"
        );
    }

    #[test]
    fn gui_launch_argument_error_renders_shared_shape() {
        let rendered =
            gui_launch_argument_error("bad argument", "Usage: agenterm [--no-activate]", true);
        assert!(rendered.contains("AgenTerm GUI argument error: bad argument"));
        assert!(rendered.contains("No GUI server was started by this invocation."));
        assert!(rendered.contains("Usage: agenterm [--no-activate]"));
        assert!(rendered.contains("More CLI commands: agenterm-cli.exe -h"));
    }
}

/// Notify the running frontend host to refresh wake state.
pub(crate) fn request_gui_wake(wake_window: isize, wake_signal: &WakeSignal) -> GuiWakeResult {
    match frontend_host() {
        FrontendHost::Windows => windows_frontend::request_gui_wake(wake_window, wake_signal),
        FrontendHost::Unix => unix_frontend::request_gui_wake(wake_window, wake_signal),
        FrontendHost::Unsupported => GuiWakeResult::Unsupported,
    }
}

pub(crate) fn request_gui_wake_best_effort(
    wake_window: isize,
    wake_signal: &WakeSignal,
    location: &'static str,
) {
    if let GuiWakeResult::Failed(error) = request_gui_wake(wake_window, wake_signal) {
        eprintln!("GUI wake failed during {location}: {error}");
    }
}
