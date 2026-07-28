use std::{env, path::PathBuf, time::SystemTime};

use serde::Serialize;
use serde_json::Value;

pub(crate) const BACKSPACE_INPUT: &[u8] = b"\x7f";

pub(crate) const COMMAND_CATALOG_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct CommandIdentity {
    pub(crate) id: &'static str,
    pub(crate) aliases: &'static [&'static str],
}

const fn command(id: &'static str, aliases: &'static [&'static str]) -> CommandIdentity {
    CommandIdentity { id, aliases }
}

pub(crate) const COMMAND_CATALOG: &[CommandIdentity] = &[
    command("attach-session", &["attach"]),
    command("active-window", &["active-tab"]),
    command("capture-pane", &["capturep"]),
    command("display-message", &["display"]),
    command("dump-cells", &[]),
    command("get-settings", &[]),
    command("has-session", &["has"]),
    command("inspect", &[]),
    command("focus", &[]),
    command("kill-server", &["server-kill"]),
    command("kill-session", &[]),
    command("kill-window", &["killw"]),
    command("list-tab-tree", &[]),
    command("list-commands", &["lscm"]),
    command("list-instances", &[]),
    command("list-panes", &["lsp"]),
    command("list-sessions", &["ls"]),
    command("list-windows", &["lsw"]),
    command("new-session", &["new"]),
    command("new-agent", &[]),
    command("new-window", &["neww"]),
    command("next-window", &["next"]),
    command("pane-snapshot", &[]),
    command("protocol-info", &[]),
    command("previous-window", &["prev"]),
    command("read-events", &[]),
    command("rename-session", &["rename"]),
    command("rename-window", &["renamew"]),
    command("screenshot", &[]),
    command("screenshot-pane", &["screenshot-tab"]),
    command("save-workspace", &[]),
    command("script", &[]),
    command("scroll-pane", &[]),
    command("select-window", &["selectw"]),
    command("send-keys", &["send"]),
    command("send-composer", &[]),
    command("send-mouse", &[]),
    command("server-list", &[]),
    command("set-setting", &[]),
    command("set-composer", &[]),
    command("set-tab-parent", &[]),
    command("set-tab-note", &[]),
    command("show-composer", &[]),
    command("show-tab-parent", &[]),
    command("show-tab-note", &[]),
    command("show-options", &["show"]),
    command("shutdown", &[]),
    command("start-server", &[]),
    command("ui-action", &[]),
    command("ui-bootstrap", &[]),
    command("ui-deltas", &[]),
    command("ui-hello", &[]),
    command("ui-interact", &[]),
    command("ui-lease", &[]),
    command("ui-snapshot", &[]),
    command("wait-pane", &["expect-pane"]),
    command("wait-events", &[]),
    command("wait-ui", &[]),
    command("workspace-info", &[]),
];

pub(crate) fn command_identity(name: &str) -> Option<&'static CommandIdentity> {
    COMMAND_CATALOG
        .iter()
        .find(|identity| identity.id == name || identity.aliases.contains(&name))
}

pub(crate) fn supported_commands() -> String {
    let mut output = String::new();
    for identity in COMMAND_CATALOG {
        output.push_str(identity.id);
        if !identity.aliases.is_empty() {
            output.push_str(" (");
            output.push_str(&identity.aliases.join(", "));
            output.push(')');
        }
        output.push('\n');
    }
    output
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MuxStatus {
    Supported,
    Unsupported(&'static str),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct MuxCommand {
    pub name: &'static str,
    pub status: MuxStatus,
}

const SPLIT_UNSUPPORTED: &str = "AgenTerm currently maps one ConPTY pane per tab";

pub(crate) const MUX_COMMANDS: &[MuxCommand] = &[
    MuxCommand {
        name: "attach",
        status: MuxStatus::Supported,
    },
    MuxCommand {
        name: "attach-session",
        status: MuxStatus::Supported,
    },
    MuxCommand {
        name: "capture-pane",
        status: MuxStatus::Supported,
    },
    MuxCommand {
        name: "capturep",
        status: MuxStatus::Supported,
    },
    MuxCommand {
        name: "display",
        status: MuxStatus::Supported,
    },
    MuxCommand {
        name: "display-message",
        status: MuxStatus::Supported,
    },
    MuxCommand {
        name: "has",
        status: MuxStatus::Supported,
    },
    MuxCommand {
        name: "has-session",
        status: MuxStatus::Supported,
    },
    MuxCommand {
        name: "kill-server",
        status: MuxStatus::Supported,
    },
    MuxCommand {
        name: "kill-session",
        status: MuxStatus::Supported,
    },
    MuxCommand {
        name: "kill-window",
        status: MuxStatus::Supported,
    },
    MuxCommand {
        name: "killw",
        status: MuxStatus::Supported,
    },
    MuxCommand {
        name: "list-commands",
        status: MuxStatus::Supported,
    },
    MuxCommand {
        name: "list-panes",
        status: MuxStatus::Supported,
    },
    MuxCommand {
        name: "list-sessions",
        status: MuxStatus::Supported,
    },
    MuxCommand {
        name: "list-windows",
        status: MuxStatus::Supported,
    },
    MuxCommand {
        name: "lscm",
        status: MuxStatus::Supported,
    },
    MuxCommand {
        name: "lsp",
        status: MuxStatus::Supported,
    },
    MuxCommand {
        name: "ls",
        status: MuxStatus::Supported,
    },
    MuxCommand {
        name: "lsw",
        status: MuxStatus::Supported,
    },
    MuxCommand {
        name: "new",
        status: MuxStatus::Supported,
    },
    MuxCommand {
        name: "new-session",
        status: MuxStatus::Supported,
    },
    MuxCommand {
        name: "new-window",
        status: MuxStatus::Supported,
    },
    MuxCommand {
        name: "neww",
        status: MuxStatus::Supported,
    },
    MuxCommand {
        name: "next",
        status: MuxStatus::Supported,
    },
    MuxCommand {
        name: "next-window",
        status: MuxStatus::Supported,
    },
    MuxCommand {
        name: "previous-window",
        status: MuxStatus::Supported,
    },
    MuxCommand {
        name: "prev",
        status: MuxStatus::Supported,
    },
    MuxCommand {
        name: "rename",
        status: MuxStatus::Supported,
    },
    MuxCommand {
        name: "rename-session",
        status: MuxStatus::Supported,
    },
    MuxCommand {
        name: "rename-window",
        status: MuxStatus::Supported,
    },
    MuxCommand {
        name: "renamew",
        status: MuxStatus::Supported,
    },
    MuxCommand {
        name: "select-window",
        status: MuxStatus::Supported,
    },
    MuxCommand {
        name: "selectw",
        status: MuxStatus::Supported,
    },
    MuxCommand {
        name: "send",
        status: MuxStatus::Supported,
    },
    MuxCommand {
        name: "send-keys",
        status: MuxStatus::Supported,
    },
    MuxCommand {
        name: "show",
        status: MuxStatus::Supported,
    },
    MuxCommand {
        name: "show-options",
        status: MuxStatus::Supported,
    },
    MuxCommand {
        name: "split-window",
        status: MuxStatus::Unsupported(SPLIT_UNSUPPORTED),
    },
    MuxCommand {
        name: "splitw",
        status: MuxStatus::Unsupported(SPLIT_UNSUPPORTED),
    },
    MuxCommand {
        name: "start-server",
        status: MuxStatus::Supported,
    },
];

pub(crate) fn mux_command(name: &str) -> Option<MuxCommand> {
    MUX_COMMANDS
        .iter()
        .find(|command| command.name == name)
        .copied()
}

#[derive(Clone, Copy)]
struct ControlCommandSpec {
    usage: &'static str,
    value_options: &'static [&'static str],
    flag_options: &'static [&'static str],
    child_at_first_positional: bool,
}

pub(crate) fn control_command_usage(command: &str) -> Option<&'static str> {
    control_command_spec(command).map(|specification| specification.usage)
}

pub(crate) fn control_command_requests_help(args: &[String]) -> bool {
    let Some(command) = args.first().map(String::as_str) else {
        return false;
    };
    let stop_at_child = control_command_spec(command)
        .is_some_and(|specification| specification.child_at_first_positional);
    for argument in args.iter().skip(1) {
        match argument.as_str() {
            "-h" | "--help" => return true,
            "--" => break,
            value if stop_at_child && !value.starts_with('-') => break,
            _ => {}
        }
    }
    false
}

pub(crate) fn validate_control_command(args: &[String]) -> Result<(), String> {
    let Some(command) = args.first().map(String::as_str) else {
        return Err("no command specified".to_owned());
    };
    let Some(specification) = control_command_spec(command) else {
        return Err(format!(
            "unknown AgenTerm command '{command}'; run `agenterm-cli list-commands`"
        ));
    };
    let mut position = 1;
    while position < args.len() {
        let argument = args[position].as_str();
        if argument == "--" {
            break;
        }
        if argument == "-" {
            position += 1;
            continue;
        }
        if specification.child_at_first_positional && !argument.starts_with('-') {
            break;
        }
        if !argument.starts_with('-') {
            position += 1;
            continue;
        }
        if specification.value_options.contains(&argument) {
            let Some(value) = args.get(position + 1) else {
                return Err(format!(
                    "{command} option {argument} requires a value\nUsage: {}",
                    specification.usage
                ));
            };
            if value == "--" {
                return Err(format!(
                    "{command} option {argument} requires a value\nUsage: {}",
                    specification.usage
                ));
            }
            position += 2;
            continue;
        }
        if specification.flag_options.contains(&argument) {
            position += 1;
            continue;
        }
        return Err(format!(
            "unknown option '{argument}' for '{command}'. To target an AgenTerm instance, put \
             `--address HOST:PORT` before the command or set AGENTERM_IPC_ADDRESS.\nUsage: {}",
            specification.usage
        ));
    }
    Ok(())
}

fn control_command_spec(command: &str) -> Option<ControlCommandSpec> {
    let (usage, value_options, flag_options, child_at_first_positional) = match command {
        "attach" | "attach-session" => (
            "agenterm-cli attach-session [-t session]",
            &["-t"][..],
            &[][..],
            false,
        ),
        "active-window" | "active-tab" => (
            "agenterm-cli active-window [-F format]",
            &["-F"][..],
            &[][..],
            false,
        ),
        "capture-pane" | "capturep" => (
            "agenterm-cli capture-pane (-p|--raw-escaped) [-t target] \
             [--max-bytes N --json]",
            &["-t", "--max-bytes"][..],
            &["-p", "--raw-escaped", "--json"][..],
            false,
        ),
        "display-message" | "display" => (
            "agenterm-cli display-message [-p] [-t target] [format]",
            &["-t"][..],
            &["-p"][..],
            false,
        ),
        "dump-cells" => (
            "agenterm-cli dump-cells [-t target] [-r row]",
            &["-t", "-r"][..],
            &[][..],
            false,
        ),
        "get-settings" => ("agenterm-cli get-settings", &[][..], &[][..], false),
        "has-session" | "has" => (
            "agenterm-cli has-session [-t session]",
            &["-t"][..],
            &[][..],
            false,
        ),
        "inspect" | "pane-snapshot" => (
            "agenterm-cli inspect [-t target]",
            &["-t"][..],
            &[][..],
            false,
        ),
        "focus" => (
            "agenterm-cli focus terminal|composer|sidebar [-t target]",
            &["-t"][..],
            &[][..],
            false,
        ),
        "kill-server" => ("agenterm-cli kill-server", &[][..], &[][..], false),
        "kill-session" => (
            "agenterm-cli kill-session [-t session]",
            &["-t"][..],
            &[][..],
            false,
        ),
        "kill-window" | "killw" => (
            "agenterm-cli kill-window -t target",
            &["-t"][..],
            &[][..],
            false,
        ),
        "list-tab-tree" => (
            "agenterm-cli list-tab-tree [-F format]",
            &["-F"][..],
            &[][..],
            false,
        ),
        "list-commands" | "lscm" => ("agenterm-cli list-commands", &[][..], &[][..], false),
        "list-instances" => (
            "agenterm-cli list-instances [--json] [--prune]",
            &[][..],
            &["--json", "--prune"][..],
            false,
        ),
        "list-panes" | "lsp" => (
            "agenterm-cli list-panes [-a] [-t target] [-F format]",
            &["-t", "-F"][..],
            &["-a"][..],
            false,
        ),
        "list-sessions" | "ls" => ("agenterm-cli list-sessions", &[][..], &[][..], false),
        "list-windows" | "lsw" => (
            "agenterm-cli list-windows [-F format]",
            &["-F"][..],
            &[][..],
            false,
        ),
        "new-session" | "new" => (
            "agenterm-cli new-session [-s name] [-- command [args...]]",
            &[
                "-n",
                "-s",
                "-t",
                "-c",
                "-F",
                "--parent",
                "-e",
                "--env",
                "--proxy",
                "--no-proxy",
                "--program",
            ][..],
            &["-d", "-A", "-P", "-E"][..],
            true,
        ),
        "new-window" | "neww" => (
            "agenterm-cli new-window [-d] [-n name] [--parent target] \
             [-F format] [-e NAME=VALUE] [-- command [args...]]",
            &[
                "-n",
                "-s",
                "-t",
                "-c",
                "-F",
                "--parent",
                "-e",
                "--env",
                "--proxy",
                "--no-proxy",
                "--program",
            ][..],
            &["-d", "-A", "-P", "-E"][..],
            true,
        ),
        "new-agent" => (
            "agenterm-cli new-agent [-d] [-n name] [--parent target] [--program exe] \
             [--proxy URL] [--yolo] [-- agent args...]",
            &[
                "-n",
                "-s",
                "-t",
                "-c",
                "-F",
                "--parent",
                "-e",
                "--env",
                "--proxy",
                "--no-proxy",
                "--program",
            ][..],
            &["-d", "-A", "-P", "-E", "--yolo"][..],
            false,
        ),
        "next-window" | "next" => ("agenterm-cli next-window", &[][..], &[][..], false),
        "previous-window" | "prev" => ("agenterm-cli previous-window", &[][..], &[][..], false),
        "protocol-info" => (
            "agenterm-cli protocol-info [--running]",
            &[][..],
            &["--running"][..],
            false,
        ),
        "rename-session" | "rename" => (
            "agenterm-cli rename-session new-name",
            &[][..],
            &[][..],
            false,
        ),
        "rename-window" | "renamew" => (
            "agenterm-cli rename-window [-t target] new-name",
            &["-t"][..],
            &[][..],
            false,
        ),
        "screenshot" => (
            "agenterm-cli screenshot [-o file.png]",
            &["-o"][..],
            &[][..],
            false,
        ),
        "screenshot-pane" | "screenshot-tab" => (
            "agenterm-cli screenshot-pane [-t target] [-o file.png]",
            &["-t", "-o"][..],
            &[][..],
            false,
        ),
        "save-workspace" => ("agenterm-cli save-workspace", &[][..], &[][..], false),
        "script" => (
            "agenterm-cli script api [MODULE] [--status shipped|planned|all] [--tree|--json] | \
             check FILE|- [--profile pure|observe|local] [--project-root DIR] | \
             eval EXPRESSION [--profile pure|observe|local] | run FILE|- \
             [--profile pure|observe|local] [--cwd DIR] [--project-root DIR] [-- ARGS...] | \
             task list|show|run [TASK] [--manifest FILE] [--json] (default profile: local)",
            &[
                "--profile",
                "--timeout-ms",
                "--max-operations",
                "--cwd",
                "--project-root",
                "--manifest",
                "--status",
            ][..],
            &["--tree", "--json"][..],
            false,
        ),
        "read-events" => (
            "agenterm-cli read-events --epoch EPOCH --after SEQUENCE [--limit COUNT]",
            &["--epoch", "--after", "--limit"][..],
            &[][..],
            false,
        ),
        "scroll-pane" => (
            "agenterm-cli scroll-pane [-t target] \
             up|down|page-up|page-down|top|bottom [rows]",
            &["-t"][..],
            &[][..],
            false,
        ),
        "select-window" | "selectw" => (
            "agenterm-cli select-window (-t target|-n|-p)",
            &["-t"][..],
            &["-n", "-p"][..],
            false,
        ),
        "send-keys" | "send" => (
            "agenterm-cli send-keys [-t target] [-l] key...",
            &["-t"][..],
            &["-l", "-R", "-X"][..],
            false,
        ),
        "send-composer" => (
            "agenterm-cli send-composer [-t target]",
            &["-t"][..],
            &[][..],
            false,
        ),
        "send-mouse" => (
            "agenterm-cli send-mouse [-t target] -x col -y row \
             [--button button] [--action action] [--protocol protocol]",
            &["-t", "-x", "-y", "--button", "--action", "--protocol"][..],
            &[][..],
            false,
        ),
        "server-kill" => ("agenterm-cli server-kill", &[][..], &[][..], false),
        "server-list" => (
            "agenterm-cli server-list [--json] [--prune]",
            &[][..],
            &["--json", "--prune"][..],
            false,
        ),
        "set-setting" => (
            "agenterm-cli set-setting key value",
            &[][..],
            &[][..],
            false,
        ),
        "set-composer" => (
            "agenterm-cli set-composer [-t target] (text|--stdin|--file path)",
            &["-t", "--file"][..],
            &["--stdin"][..],
            false,
        ),
        "set-tab-parent" => (
            "agenterm-cli set-tab-parent -t child --parent parent|root",
            &["-t", "--parent"][..],
            &[][..],
            false,
        ),
        "set-tab-note" => (
            "agenterm-cli set-tab-note [-t target] text",
            &["-t"][..],
            &[][..],
            false,
        ),
        "show-composer" => (
            "agenterm-cli show-composer [-t target]",
            &["-t"][..],
            &[][..],
            false,
        ),
        "show-options" | "show" => ("agenterm-cli show-options", &[][..], &[][..], false),
        "show-tab-parent" => (
            "agenterm-cli show-tab-parent [-t target]",
            &["-t"][..],
            &[][..],
            false,
        ),
        "show-tab-note" => (
            "agenterm-cli show-tab-note [-t target]",
            &["-t"][..],
            &[][..],
            false,
        ),
        "shutdown" => ("agenterm-cli shutdown", &[][..], &[][..], false),
        "start-server" => ("agenterm-cli start-server", &[][..], &[][..], false),
        "ui-action" => (
            "agenterm-cli ui-action ACTION [-t target] [--path PATH] \
             [--mode empty|append|replace] [--stdin] [--width PX --height PX]",
            &[
                "-t",
                "--path",
                "--mode",
                "--proxy-input",
                "--width",
                "--height",
            ][..],
            &["--stdin"][..],
            false,
        ),
        "ui-bootstrap" => ("agenterm-cli ui-bootstrap", &[][..], &[][..], false),
        "ui-deltas" => (
            "agenterm-cli ui-deltas --epoch EPOCH --after SEQUENCE [--limit 1..64]",
            &["--epoch", "--after", "--limit"][..],
            &[][..],
            false,
        ),
        "ui-hello" => (
            "agenterm-cli ui-hello --minimum VERSION --maximum VERSION \
             [--client-id ID] [--client-build-json JSON]",
            &[
                "--minimum",
                "--maximum",
                "--client-id",
                "--client-build-json",
            ][..],
            &[][..],
            false,
        ),
        "ui-interact" => (
            "agenterm-cli ui-interact (select|input|resize) \
             --lease-id ID --client-pid PID -t @ID \
             [--hex HEX|--rows ROWS --columns COLUMNS]",
            &[
                "--lease-id",
                "--client-pid",
                "-t",
                "--hex",
                "--rows",
                "--columns",
            ][..],
            &[][..],
            false,
        ),
        "ui-lease" => (
            "agenterm-cli ui-lease \
             (attach --client-id ID --client-pid PID|heartbeat|\
             acknowledge --sequence N|detach|status) \
             [--lease-id ID --client-pid PID] [--client-build-json JSON]",
            &[
                "--client-id",
                "--client-pid",
                "--lease-id",
                "--sequence",
                "--client-build-json",
            ][..],
            &[][..],
            false,
        ),
        "ui-snapshot" => ("agenterm-cli ui-snapshot", &[][..], &[][..], false),
        "wait-pane" | "expect-pane" => (
            "agenterm-cli wait-pane [-t target] \
             (--contains text|--dead|--submit-complete|--finalized) [--timeout-ms ms]",
            &["-t", "--contains", "--timeout-ms"][..],
            &["--dead", "--submit-complete", "--finalized"][..],
            false,
        ),
        "wait-events" => (
            "agenterm-cli wait-events --epoch EPOCH --after SEQUENCE --kind KIND \
             [--tab @ID] [--timeout-ms MS]",
            &["--epoch", "--after", "--kind", "--tab", "--timeout-ms"][..],
            &[][..],
            false,
        ),
        "wait-ui" => (
            "agenterm-cli wait-ui [--active @id] [--focus surface] \
             [-t target --tab-state state] [--window-state state] \
             [-t target --proxy-state state] \
             [--client-width PX --client-height PX] \
             [--modal-kind KIND|none|closed] [--modal-target target] \
             [--timeout-ms ms]",
            &[
                "--active",
                "--focus",
                "-t",
                "--tab-state",
                "--proxy-state",
                "--window-state",
                "--client-width",
                "--client-height",
                "--modal-kind",
                "--modal-target",
                "--timeout-ms",
            ][..],
            &[][..],
            false,
        ),
        "workspace-info" => ("agenterm-cli workspace-info", &[][..], &[][..], false),
        _ => return None,
    };
    Some(ControlCommandSpec {
        usage,
        value_options,
        flag_options,
        child_at_first_positional,
    })
}

pub(crate) fn canonical_control_command(command: &str) -> &str {
    command_identity(command).map_or(command, |identity| identity.id)
}

pub(crate) fn has_option(args: &[String], option: &str) -> bool {
    args.iter()
        .take_while(|argument| argument.as_str() != "--")
        .any(|argument| argument == option)
}

pub(crate) fn option_value<'a>(args: &'a [String], option: &str) -> Option<&'a str> {
    args.iter()
        .take_while(|argument| argument.as_str() != "--")
        .position(|argument| argument == option)
        .and_then(|position| args.get(position + 1))
        .filter(|value| value.as_str() != "--")
        .map(String::as_str)
}

pub(crate) fn snapshot_modal_matches(
    snapshot: &Value,
    expected_kind: Option<&str>,
    expected_target: Option<&str>,
) -> bool {
    let modal = snapshot.get("modal").filter(|value| !value.is_null());
    let kind_matches = expected_kind.is_none_or(|expected| {
        if matches!(expected, "none" | "closed") {
            modal.is_none()
        } else {
            modal.and_then(|value| value["kind"].as_str()) == Some(expected)
        }
    });
    let target_matches = expected_target.is_none_or(|selector| {
        let Some(actual) = modal.and_then(|value| value["window_id"].as_str()) else {
            return false;
        };
        if actual == selector {
            return true;
        }
        snapshot["tabs"].as_array().is_some_and(|tabs| {
            tabs.iter().any(|tab| {
                let selector_matches = tab["id"].as_str() == Some(selector)
                    || tab["name"].as_str() == Some(selector)
                    || selector
                        .parse::<u64>()
                        .ok()
                        .is_some_and(|index| tab["index"].as_u64() == Some(index));
                selector_matches && tab["id"].as_str() == Some(actual)
            })
        })
    });
    kind_matches && target_matches
}

pub(crate) fn parse_new_command(args: &[String]) -> (Option<String>, bool, Vec<String>) {
    let mut title = None;
    let mut detached = false;
    let mut position = 1;
    while position < args.len() {
        match args[position].as_str() {
            "-n" => {
                title = args.get(position + 1).cloned();
                position += 2;
            }
            "-d" => {
                detached = true;
                position += 1;
            }
            "-A" | "-P" | "-E" => position += 1,
            "-s" | "-t" | "-c" | "-F" | "--parent" | "-e" | "--env" | "--proxy" | "--no-proxy"
            | "--program" => position += 2,
            "--" => {
                position += 1;
                break;
            }
            option if option.starts_with('-') => position += 1,
            _ => break,
        }
    }
    (title, detached, args[position..].to_vec())
}

pub(crate) fn parse_tab_environment(args: &[String]) -> Result<Vec<(String, String)>, String> {
    let mut environment = Vec::new();
    let mut position = 1;
    while position < args.len() {
        let argument = args[position].as_str();
        if matches!(argument, "-e" | "--env") {
            let assignment = args
                .get(position + 1)
                .ok_or_else(|| format!("{argument} requires NAME=VALUE"))?;
            let (name, value) = assignment
                .split_once('=')
                .ok_or_else(|| format!("{argument} requires NAME=VALUE"))?;
            validate_environment_name(name)?;
            validate_environment_value(value, argument)?;
            upsert_environment(&mut environment, name, value);
            position += 2;
        } else if argument == "--proxy" {
            let value = args
                .get(position + 1)
                .ok_or_else(|| "--proxy requires a URL".to_owned())?;
            if value.is_empty() {
                return Err("--proxy requires a non-empty URL".to_owned());
            }
            validate_environment_value(value, "--proxy")?;
            upsert_environment(&mut environment, "HTTP_PROXY", value);
            upsert_environment(&mut environment, "HTTPS_PROXY", value);
            position += 2;
        } else if argument == "--no-proxy" {
            let value = args
                .get(position + 1)
                .ok_or_else(|| "--no-proxy requires a host list".to_owned())?;
            validate_environment_value(value, "--no-proxy")?;
            upsert_environment(&mut environment, "NO_PROXY", value);
            position += 2;
        } else if argument == "--" {
            break;
        } else if matches!(
            argument,
            "-n" | "-s" | "-t" | "-c" | "-F" | "--parent" | "--program"
        ) {
            position += 2;
        } else if matches!(argument, "-d" | "-A" | "-P" | "-E") || argument.starts_with('-') {
            position += 1;
        } else {
            break;
        }
    }
    Ok(environment)
}

fn validate_environment_value(value: &str, option: &str) -> Result<(), String> {
    if value.contains('\0') {
        return Err(format!("{option} value must not contain NUL"));
    }
    Ok(())
}

fn validate_environment_name(name: &str) -> Result<(), String> {
    if name.is_empty()
        || name.contains(['=', '\0'])
        || !name
            .chars()
            .all(|character| character == '_' || character.is_ascii_alphanumeric())
        || name
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_digit())
    {
        return Err(format!("invalid environment variable name: {name}"));
    }
    if name.to_ascii_uppercase().starts_with("AGENTERM_") {
        return Err(format!(
            "{name} is reserved; AgenTerm injects its own tab context"
        ));
    }
    Ok(())
}

fn upsert_environment(environment: &mut Vec<(String, String)>, name: &str, value: &str) {
    if let Some(existing) = environment
        .iter_mut()
        .find(|(existing, _)| existing.eq_ignore_ascii_case(name))
    {
        existing.0 = name.to_owned();
        existing.1 = value.to_owned();
    } else {
        environment.push((name.to_owned(), value.to_owned()));
    }
}

pub(crate) fn positional_values<'a>(
    args: &'a [String],
    value_options: &[&str],
    boolean_options: &[&str],
) -> Vec<&'a str> {
    let mut values = Vec::new();
    let mut position = 1;
    while position < args.len() {
        let argument = args[position].as_str();
        if value_options.contains(&argument) {
            position += 2;
        } else if boolean_options.contains(&argument) {
            position += 1;
        } else if argument == "--" {
            values.extend(args[position + 1..].iter().map(String::as_str));
            break;
        } else if argument.starts_with('-') {
            position += 1;
        } else {
            values.push(argument);
            position += 1;
        }
    }
    values
}

pub(crate) fn last_positional<'a>(args: &'a [String], value_options: &[&str]) -> Option<&'a str> {
    positional_values(args, value_options, &["-p", "-v", "-a", "-g"])
        .last()
        .copied()
}

pub(crate) fn screenshot_output_path(args: &[String], stem: &str) -> PathBuf {
    if let Some(path) = option_value(args, "-o").or_else(|| last_positional(args, &["-t", "-o"])) {
        return PathBuf::from(path);
    }
    let timestamp = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(format!("{stem}-{timestamp}.png"))
}

pub(crate) fn tmux_key_bytes(key: &str) -> Option<Vec<u8>> {
    let bytes = match key {
        "Enter" => b"\r".as_slice(),
        "Escape" | "Esc" => b"\x1b".as_slice(),
        "Space" => b" ".as_slice(),
        "BSpace" | "Backspace" => BACKSPACE_INPUT,
        "Tab" => b"\t".as_slice(),
        "Up" => b"\x1b[A".as_slice(),
        "Down" => b"\x1b[B".as_slice(),
        "Right" => b"\x1b[C".as_slice(),
        "Left" => b"\x1b[D".as_slice(),
        "Home" => b"\x1b[H".as_slice(),
        "End" => b"\x1b[F".as_slice(),
        "DC" | "Delete" => b"\x1b[3~".as_slice(),
        "PPage" | "PageUp" => b"\x1b[5~".as_slice(),
        "NPage" | "PageDown" => b"\x1b[6~".as_slice(),
        "F1" => b"\x1bOP".as_slice(),
        "F2" => b"\x1bOQ".as_slice(),
        "F3" => b"\x1bOR".as_slice(),
        "F4" => b"\x1bOS".as_slice(),
        "F5" => b"\x1b[15~".as_slice(),
        "F6" => b"\x1b[17~".as_slice(),
        "F7" => b"\x1b[18~".as_slice(),
        "F8" => b"\x1b[19~".as_slice(),
        "F9" => b"\x1b[20~".as_slice(),
        "F10" => b"\x1b[21~".as_slice(),
        "F11" => b"\x1b[23~".as_slice(),
        "F12" => b"\x1b[24~".as_slice(),
        _ => {
            if let Some(character) = key.strip_prefix("C-").and_then(|value| {
                let mut characters = value.chars();
                let first = characters.next()?;
                characters.next().is_none().then_some(first)
            }) {
                let upper = character.to_ascii_uppercase();
                if upper.is_ascii_alphabetic() {
                    return Some(vec![(upper as u8) - b'@']);
                }
            }
            return None;
        }
    };
    Some(bytes.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn parses_new_window_options_and_child_command() {
        let parsed = parse_new_command(&args(&[
            "new-window",
            "-d",
            "-n",
            "build",
            "--parent",
            "@1",
            "--",
            "cmd.exe",
            "/k",
            "echo ready",
        ]));
        assert_eq!(parsed.0.as_deref(), Some("build"));
        assert!(parsed.1);
        assert_eq!(parsed.2, args(&["cmd.exe", "/k", "echo ready"]));
    }

    #[test]
    fn extracts_positionals_without_option_values() {
        let input = args(&["rename-window", "-t", "@2", "build", "logs"]);
        assert_eq!(
            positional_values(&input, &["-t"], &[]),
            vec!["build", "logs"]
        );
        assert_eq!(last_positional(&input, &["-t"]), Some("logs"));
    }

    #[test]
    fn maps_tmux_function_and_control_keys() {
        assert_eq!(tmux_key_bytes("F2"), Some(b"\x1bOQ".to_vec()));
        assert_eq!(tmux_key_bytes("C-c"), Some(vec![3]));
        assert_eq!(tmux_key_bytes("Backspace"), Some(vec![0x7f]));
        assert_eq!(tmux_key_bytes("not-a-key"), None);
    }

    #[test]
    fn parses_scoped_environment_and_proxy_convenience() {
        let parsed = parse_tab_environment(&args(&[
            "new-window",
            "-e",
            "ROLE=reviewer",
            "--proxy",
            "http://127.0.0.1:7890",
            "--no-proxy",
            "localhost,127.0.0.1",
        ]))
        .unwrap();
        assert_eq!(
            parsed,
            vec![
                ("ROLE".to_owned(), "reviewer".to_owned()),
                ("HTTP_PROXY".to_owned(), "http://127.0.0.1:7890".to_owned()),
                ("HTTPS_PROXY".to_owned(), "http://127.0.0.1:7890".to_owned()),
                ("NO_PROXY".to_owned(), "localhost,127.0.0.1".to_owned()),
            ]
        );
    }

    #[test]
    fn rejects_reserved_or_malformed_environment_names() {
        assert!(parse_tab_environment(&args(&["new-window", "-e", "1BAD=x"])).is_err());
        assert!(
            parse_tab_environment(&args(&["new-window", "-e", "AGENTERM_TAB_ID=fake"])).is_err()
        );
        assert!(parse_tab_environment(&args(&["new-window", "-e", "ROLE=a\0b"])).is_err());
        assert!(parse_tab_environment(&args(&["new-window", "--proxy", "a\0b"])).is_err());
    }

    #[test]
    fn option_lookup_stops_at_child_argument_delimiter() {
        let input = args(&[
            "new-agent",
            "--program",
            "cmd.exe",
            "--",
            "--program",
            "wrong.exe",
            "--parent",
            "@999",
            "--yolo",
        ]);
        assert_eq!(option_value(&input, "--program"), Some("cmd.exe"));
        assert_eq!(option_value(&input, "--parent"), None);
        assert!(!has_option(&input, "--yolo"));
    }

    #[test]
    fn control_help_is_detected_before_a_child_command_only() {
        assert!(control_command_requests_help(&args(&[
            "capture-pane",
            "--help"
        ])));
        assert!(control_command_requests_help(&args(&[
            "new-window",
            "--help"
        ])));
        assert!(!control_command_requests_help(&args(&[
            "new-window",
            "bash.exe",
            "--help"
        ])));
        assert!(!control_command_requests_help(&args(&[
            "new-agent",
            "--",
            "--help"
        ])));
    }

    #[test]
    fn control_options_fail_fast_with_instance_targeting_help() {
        let error = validate_control_command(&args(&["capture-pane", "-a", "127.0.0.1:48914"]))
            .unwrap_err();
        assert!(error.contains("unknown option '-a'"));
        assert!(error.contains("--address HOST:PORT"));
        assert!(validate_control_command(&args(&["capture-pane", "-p", "-t", "@1"])).is_ok());
    }

    #[test]
    fn script_api_catalog_accepts_module_and_status_options() {
        assert!(
            validate_control_command(&args(&[
                "script", "api", "std::fs", "--status", "shipped", "--json",
            ]))
            .is_ok()
        );
    }

    #[test]
    fn command_catalog_is_unique_and_drives_public_identity() {
        let mut names = std::collections::BTreeSet::new();
        for identity in COMMAND_CATALOG {
            assert!(
                names.insert(identity.id),
                "duplicate command {}",
                identity.id
            );
            assert!(
                control_command_spec(identity.id).is_some(),
                "command {} lacks an argument contract",
                identity.id
            );
            for alias in identity.aliases {
                assert!(names.insert(alias), "duplicate command alias {alias}");
                assert!(
                    control_command_spec(alias).is_some(),
                    "alias {alias} lacks an argument contract"
                );
                assert_eq!(canonical_control_command(alias), identity.id);
            }
        }
        assert_eq!(supported_commands().lines().count(), COMMAND_CATALOG.len());
    }

    #[test]
    fn canonicalizes_aliases_to_stable_command_identity() {
        assert_eq!(canonical_control_command("server-kill"), "kill-server");
        assert_eq!(canonical_control_command("neww"), "new-window");
        assert_eq!(canonical_control_command("capturep"), "capture-pane");
        assert_eq!(canonical_control_command("server-list"), "server-list");
    }

    #[test]
    fn modal_wait_matches_kind_and_stable_or_resolved_target() {
        let snapshot = serde_json::json!({
            "modal": {
                "kind": "cwd-editor",
                "window_id": "@7",
            },
            "tabs": [{
                "id": "@7",
                "index": 2,
                "name": "build",
            }],
        });
        assert!(snapshot_modal_matches(
            &snapshot,
            Some("cwd-editor"),
            Some("@7")
        ));
        assert!(snapshot_modal_matches(
            &snapshot,
            Some("cwd-editor"),
            Some("build")
        ));
        assert!(snapshot_modal_matches(
            &snapshot,
            Some("cwd-editor"),
            Some("2")
        ));
        assert!(snapshot_modal_matches(&snapshot, None, Some("build")));
        assert!(!snapshot_modal_matches(
            &snapshot,
            Some("proxy-editor"),
            Some("@7")
        ));
        assert!(!snapshot_modal_matches(
            &snapshot,
            Some("cwd-editor"),
            Some("@8")
        ));
    }

    #[test]
    fn modal_wait_none_and_closed_require_no_open_modal() {
        let closed = serde_json::json!({"modal": null, "tabs": []});
        let settings = serde_json::json!({
            "modal": {"kind": "settings"},
            "tabs": [],
        });
        assert!(snapshot_modal_matches(&closed, Some("none"), None));
        assert!(snapshot_modal_matches(&closed, Some("closed"), None));
        assert!(!snapshot_modal_matches(&closed, None, Some("@1")));
        assert!(!snapshot_modal_matches(&settings, Some("none"), None));
        assert!(snapshot_modal_matches(&settings, Some("settings"), None));
        assert!(!snapshot_modal_matches(
            &settings,
            Some("settings"),
            Some("@1")
        ));
    }
}
