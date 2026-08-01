use std::env;

use anyhow::{Context as _, Result};

use crate::ipc_endpoint::EndpointSelectorArgs;
use crate::wake_signal::WakeSignal;

/// Wake the Win32 message loop without posting one message per producer event.
pub(crate) fn request_gui_wake(wake_window: isize, wake_signal: &WakeSignal) {
    if wake_signal.request() {
        // SAFETY: the GUI owns the wake HWND for the duration of this call.
        if let Some(window) =
            unsafe { agenterm_platform::activation::NativeWindowHandle::from_raw(wake_window) }
        {
            let _ = agenterm_platform::activation::post_application_wake(window);
        }
    }
}

/// Windows-subsystem launcher entry point.
///
/// The GUI owns only HWND/layout/render/input state. Session, tab, PTY and
/// event truth live in the independently replaceable server process.
pub fn run_gui_entry() -> i32 {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    if arguments
        .first()
        .is_some_and(|argument| !argument.starts_with("--"))
    {
        write_best_effort_stderr(&gui_cli_guidance(&arguments));
        return 2;
    }
    let launch_options = match configure_gui_launch(&arguments) {
        Ok(options) => options,
        Err(error) => {
            write_best_effort_stderr(&format!(
                "AgenTerm GUI argument error: {error:#}\n\
                 No GUI server was started by this invocation.\n\
                 More CLI commands: agenterm-cli.exe -h"
            ));
            return 2;
        }
    };
    let no_activate = launch_options.no_activate || crate::client::no_activate_from_environment();
    write_best_effort_stderr(&gui_console_summary(&crate::ipc_address()));

    // Preserve the historical launcher handoff when a compatible UI already
    // owns this server. A headless server explicitly asks us to create the
    // replaceable client instead.
    if env::var_os("AGENTERM_SERVER").is_none() && !launch_options.ui_client {
        let handoff = if no_activate {
            "__show-no-activate"
        } else {
            "__focus"
        };
        match crate::client::send_ipc_request(vec![handoff.to_owned()]) {
            Ok(response) if response.ok => return 0,
            Ok(response) if response.error_code == "ui_client_unavailable" => {}
            Ok(response) => {
                write_best_effort_stderr(&format!(
                    "The running AgenTerm server rejected the launcher handoff: {}\n\
                     Restart that server to use this launcher capability.",
                    response.error
                ));
                return 1;
            }
            Err(_) => {}
        }
    }

    if let Err(error) = super::remote_frontend::run_remote_gui(no_activate) {
        show_startup_error(&error);
        return 1;
    }
    0
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct GuiLaunchOptions {
    no_activate: bool,
    ui_client: bool,
}

fn configure_gui_launch(arguments: &[String]) -> Result<GuiLaunchOptions> {
    let (options, selectors) = parse_gui_launch(arguments)?;
    crate::client::set_ipc_selectors(selectors)?;
    Ok(options)
}

fn parse_gui_launch(arguments: &[String]) -> Result<(GuiLaunchOptions, EndpointSelectorArgs)> {
    let mut options = GuiLaunchOptions::default();
    let mut selectors = EndpointSelectorArgs::default();
    let mut position = 0;
    while position < arguments.len() {
        match arguments[position].as_str() {
            "--no-activate" | "--not-foreground" => {
                if options.no_activate {
                    anyhow::bail!(
                        "agenterm.exe --no-activate/--not-foreground may be specified only once"
                    );
                }
                options.no_activate = true;
                position += 1;
            }
            "--ui-client" => {
                if options.ui_client {
                    anyhow::bail!("agenterm.exe --ui-client may be specified only once");
                }
                options.ui_client = true;
                position += 1;
            }
            "--address" => {
                if selectors.address.is_some() {
                    anyhow::bail!("agenterm.exe --address may be specified only once");
                }
                let value = arguments
                    .get(position + 1)
                    .context("agenterm.exe --address requires HOST:PORT")?;
                if value.starts_with("--") {
                    anyhow::bail!("agenterm.exe --address requires HOST:PORT");
                }
                crate::client::parse_loopback_ipc_address(value)?;
                selectors.address = Some(value.clone());
                position += 2;
            }
            "--endpoint" => {
                if selectors.endpoint.is_some() {
                    anyhow::bail!("agenterm.exe --endpoint may be specified only once");
                }
                selectors.endpoint = Some(
                    arguments
                        .get(position + 1)
                        .context("agenterm.exe --endpoint requires ENDPOINT")?
                        .clone(),
                );
                position += 2;
            }
            "--instance" => {
                if selectors.instance.is_some() {
                    anyhow::bail!("agenterm.exe --instance may be specified only once");
                }
                selectors.instance = Some(
                    arguments
                        .get(position + 1)
                        .context("agenterm.exe --instance requires NAME")?
                        .clone(),
                );
                position += 2;
            }
            argument => {
                anyhow::bail!("unsupported AgenTerm GUI argument: {argument}")
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
        anyhow::bail!("agenterm.exe --endpoint, --address, and --instance are mutually exclusive");
    }
    Ok((options, selectors))
}

fn quote_argument_for_display(argument: &str) -> String {
    if argument.is_empty() || argument.chars().any(char::is_whitespace) {
        format!("\"{}\"", argument.replace('"', "\\\""))
    } else {
        argument.to_owned()
    }
}

fn gui_cli_guidance(arguments: &[String]) -> String {
    let forwarded = arguments
        .iter()
        .map(|argument| quote_argument_for_display(argument))
        .collect::<Vec<_>>()
        .join(" ");
    format!(
        "AgenTerm GUI entry point\n\n\
         No CLI command was executed and no GUI server was started by this invocation.\n\n\
         Use instead:\nagenterm-cli.exe {forwarded}\n\n\
         Launcher PID: {}\nConfigured server address: {}\n\n\
         List running server PID and port: agenterm-cli.exe server-list\n\
         More CLI commands: agenterm-cli.exe -h",
        std::process::id(),
        crate::ipc_address()
    )
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
    use super::{gui_cli_guidance, parse_gui_launch};

    #[test]
    fn gui_cli_guidance_preserves_arguments_and_names_the_real_cli() {
        let guidance = gui_cli_guidance(&[
            "list-windows".to_owned(),
            "-F".to_owned(),
            "#{window_id} #{window_name}".to_owned(),
        ]);
        assert!(guidance.contains("No CLI command was executed"));
        assert!(
            guidance.contains("agenterm-cli.exe list-windows -F \"#{window_id} #{window_name}\"")
        );
        assert!(guidance.contains("Launcher PID:"));
        assert!(guidance.contains("Configured server address:"));
        assert!(guidance.contains("agenterm-cli.exe server-list"));
        assert!(guidance.contains("agenterm-cli.exe -h"));
    }

    #[test]
    fn gui_launcher_accepts_no_activate_and_address_in_either_order() {
        let (options, selectors) = parse_gui_launch(&[
            "--no-activate".to_owned(),
            "--address".to_owned(),
            "127.0.0.1:48815".to_owned(),
        ])
        .unwrap();
        assert!(options.no_activate);
        assert!(!options.ui_client);
        assert_eq!(selectors.address.as_deref(), Some("127.0.0.1:48815"));

        let (options, selectors) = parse_gui_launch(&[
            "--address".to_owned(),
            "127.0.0.1:48816".to_owned(),
            "--not-foreground".to_owned(),
        ])
        .unwrap();
        assert!(options.no_activate);
        assert!(!options.ui_client);
        assert_eq!(selectors.address.as_deref(), Some("127.0.0.1:48816"));

        let (options, selectors) = parse_gui_launch(&[
            "--ui-client".to_owned(),
            "--address".to_owned(),
            "127.0.0.1:48817".to_owned(),
            "--no-activate".to_owned(),
        ])
        .unwrap();
        assert!(options.ui_client);
        assert!(options.no_activate);
        assert_eq!(selectors.address.as_deref(), Some("127.0.0.1:48817"));
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
                parse_gui_launch(&arguments.into_iter().map(str::to_owned).collect::<Vec<_>>())
                    .is_err()
            );
        }
    }
}
