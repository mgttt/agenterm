use crate::frontend::{
    GuiHandoffResult, GuiLaunchResult, GuiWakeResult, WINDOWS_GUI_CLI_NAME,
    WINDOWS_GUI_LAUNCH_POLICY, WINDOWS_GUI_USAGE, attempt_gui_handoff, gui_cli_guidance,
    gui_help_result, gui_launch_argument_error, is_gui_cli_guidance_error, parse_gui_launch_target,
};
use crate::wake_signal::WakeSignal;
use std::env;

/// Wake the Win32 message loop without posting one message per producer event.
pub(crate) fn request_gui_wake(wake_window: isize, wake_signal: &WakeSignal) -> GuiWakeResult {
    if wake_signal.request() {
        // SAFETY: the GUI owns the wake HWND for the duration of this call.
        if let Some(window) =
            unsafe { agenterm_platform::activation::NativeWindowHandle::from_raw(wake_window) }
        {
            if agenterm_platform::activation::post_application_wake(window).is_ok() {
                return GuiWakeResult::Woke;
            }
            return GuiWakeResult::Failed("post_application_wake failed".to_owned());
        }
        return GuiWakeResult::NoTarget;
    }
    GuiWakeResult::Throttled
}

/// Windows-subsystem launcher entry point.
///
/// The GUI owns only HWND/layout/render/input state. Session, tab, PTY and
/// event truth live in the independently replaceable server process.
pub(crate) fn run_gui_entry_result() -> GuiLaunchResult {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    if let Some(result) = gui_help_result(&arguments, WINDOWS_GUI_USAGE) {
        return result;
    }
    let launch_options = match parse_gui_launch_target(&arguments, WINDOWS_GUI_LAUNCH_POLICY) {
        Ok(options) => options,
        Err(error) => {
            let rendered = if is_gui_cli_guidance_error(&error) {
                gui_cli_guidance(&arguments, WINDOWS_GUI_CLI_NAME, WINDOWS_GUI_USAGE)
            } else {
                gui_launch_argument_error(&error, WINDOWS_GUI_USAGE, true)
            };
            write_best_effort_stderr(&rendered);
            return GuiLaunchResult::UsageError;
        }
    };
    let no_activate = launch_options.no_activate || crate::client::no_activate_from_environment();
    write_best_effort_stderr(&gui_console_summary(&crate::ipc_address()));

    // Preserve the historical launcher handoff when a compatible GUI/lease
    // already owns this server.
    if !launch_options.ui_client {
        match attempt_gui_handoff(no_activate, true) {
            GuiHandoffResult::HandedOff => return GuiLaunchResult::Reused,
            GuiHandoffResult::Continue => {}
            GuiHandoffResult::Blocked(error) => {
                write_best_effort_stderr(&format!(
                    "The running AgenTerm server rejected the launcher handoff: {}\n\
                     Restart that server to use this launcher capability.",
                    error
                ));
                return GuiLaunchResult::BlockedByServer(error);
            }
        }
    }

    if let Err(error) = super::remote_frontend::run_remote_gui(no_activate) {
        show_startup_error(&error);
        return GuiLaunchResult::StartupFailed(error.to_string());
    }
    GuiLaunchResult::Launched
}

fn gui_console_summary(address: &str) -> String {
    format!(
        "Launcher PID: {}\n\
         Configured server address: {address}\n\n\
         List running server PID and port: agenterm-cli.exe server-list\n\
         More CLI commands: agenterm-cli.exe -h",
        std::process::id()
    )
}

fn write_best_effort_stderr(message: &str) {
    let _ = agenterm_platform::process::write_parent_console_stderr(message);
}

fn show_startup_error(error: &anyhow::Error) {
    write_best_effort_stderr(&format!("AgenTerm failed to start:\n\n{error:#}"));
}

#[cfg(test)]
mod tests {
    use super::{
        GuiLaunchResult, WINDOWS_GUI_LAUNCH_POLICY, WINDOWS_GUI_USAGE, gui_help_result,
        parse_gui_launch_target,
    };

    #[test]
    fn gui_launcher_accepts_no_activate_and_address_in_either_order() {
        let options = parse_gui_launch_target(
            &[
                "--no-activate".to_owned(),
                "--address".to_owned(),
                "127.0.0.1:48815".to_owned(),
            ],
            WINDOWS_GUI_LAUNCH_POLICY,
        )
        .unwrap();
        assert!(options.no_activate);
        assert!(!options.ui_client);
        assert_eq!(
            options.selectors.address.as_deref(),
            Some("127.0.0.1:48815")
        );

        let options = parse_gui_launch_target(
            &[
                "--address".to_owned(),
                "127.0.0.1:48816".to_owned(),
                "--not-foreground".to_owned(),
            ],
            WINDOWS_GUI_LAUNCH_POLICY,
        )
        .unwrap();
        assert!(options.no_activate);
        assert!(!options.ui_client);
        assert_eq!(
            options.selectors.address.as_deref(),
            Some("127.0.0.1:48816")
        );

        let options = parse_gui_launch_target(
            &[
                "--ui-client".to_owned(),
                "--address".to_owned(),
                "127.0.0.1:48817".to_owned(),
                "--no-activate".to_owned(),
            ],
            WINDOWS_GUI_LAUNCH_POLICY,
        )
        .unwrap();
        assert!(options.ui_client);
        assert!(options.no_activate);
        assert_eq!(
            options.selectors.address.as_deref(),
            Some("127.0.0.1:48817")
        );
    }

    #[test]
    fn gui_launcher_rejects_duplicate_unknown_and_missing_options() {
        for arguments in [
            vec!["--no-activate", "--no-activate"],
            vec!["--no-activate", "--not-foreground"],
            vec!["--not-foreground", "--not-foreground"],
            vec!["--ui-client", "--ui-client"],
            vec![
                "--address",
                "127.0.0.1:48815",
                "--address",
                "127.0.0.1:48816",
            ],
            vec!["--address"],
            vec!["--address", "--no-activate"],
            vec![
                "--endpoint",
                r"pipe:\\.\pipe\agenterm-test",
                "--instance",
                "dev",
            ],
            vec!["--unknown"],
        ] {
            assert!(
                parse_gui_launch_target(
                    &arguments.into_iter().map(str::to_owned).collect::<Vec<_>>(),
                    WINDOWS_GUI_LAUNCH_POLICY,
                )
                .is_err()
            );
        }
    }

    #[test]
    fn gui_launcher_help_is_supported_by_frontend_contract() {
        assert!(matches!(
            gui_help_result(&["--help".to_owned()], WINDOWS_GUI_USAGE),
            Some(GuiLaunchResult::UsageHelpPrinted)
        ));
    }
}
