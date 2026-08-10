use std::{env, path::PathBuf, time::SystemTime};

use serde::Serialize;
use serde_json::Value;

use crate::platform::ModifierState;

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
    command("agent-tools", &[]),
    command("attach-session", &["attach"]),
    command("active-window", &["active-tab"]),
    command("capture-pane", &["capturep"]),
    command("control-center", &[]),
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
    command("rh-pack", &[]),
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
    command("set-buffer", &["setb"]),
    command("set-setting", &[]),
    command("set-composer", &[]),
    command("set-tab-parent", &[]),
    command("set-tab-note", &[]),
    command("show-buffer", &["showb"]),
    command("show-composer", &[]),
    command("show-tab-parent", &[]),
    command("show-tab-note", &[]),
    command("show-options", &["show"]),
    command("load-buffer", &["loadb"]),
    command("list-buffers", &["lsb"]),
    command("delete-buffer", &["deleteb"]),
    command("paste-buffer", &["pasteb"]),
    command("shutdown", &[]),
    command("start-server", &[]),
    command("ui-action", &[]),
    command("ui-bootstrap", &[]),
    command("ui-client-state", &[]),
    command("ui-client-command", &[]),
    command("ui-deltas", &[]),
    command("ui-input", &[]),
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
const SAVE_BUFFER_UNSUPPORTED: &str =
    "save-buffer is not implemented; use show-buffer and redirect or load-buffer from a file";

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
        name: "set-buffer",
        status: MuxStatus::Supported,
    },
    MuxCommand {
        name: "setb",
        status: MuxStatus::Supported,
    },
    MuxCommand {
        name: "load-buffer",
        status: MuxStatus::Supported,
    },
    MuxCommand {
        name: "loadb",
        status: MuxStatus::Supported,
    },
    MuxCommand {
        name: "show-buffer",
        status: MuxStatus::Supported,
    },
    MuxCommand {
        name: "showb",
        status: MuxStatus::Supported,
    },
    MuxCommand {
        name: "list-buffers",
        status: MuxStatus::Supported,
    },
    MuxCommand {
        name: "lsb",
        status: MuxStatus::Supported,
    },
    MuxCommand {
        name: "delete-buffer",
        status: MuxStatus::Supported,
    },
    MuxCommand {
        name: "deleteb",
        status: MuxStatus::Supported,
    },
    MuxCommand {
        name: "paste-buffer",
        status: MuxStatus::Supported,
    },
    MuxCommand {
        name: "pasteb",
        status: MuxStatus::Supported,
    },
    MuxCommand {
        name: "save-buffer",
        status: MuxStatus::Unsupported(SAVE_BUFFER_UNSUPPORTED),
    },
    MuxCommand {
        name: "saveb",
        status: MuxStatus::Unsupported(SAVE_BUFFER_UNSUPPORTED),
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
            "unknown AgenTerm command '{command}'; run `agenterm cli list-commands`"
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
             `--endpoint ENDPOINT`, legacy `--address HOST:PORT`, or `--instance NAME` before the command.\nUsage: {}",
            specification.usage
        ));
    }
    Ok(())
}

fn control_command_spec(command: &str) -> Option<ControlCommandSpec> {
    let (usage, value_options, flag_options, child_at_first_positional) = match command {
        // Derived LLM tool table; a pure projection of `OPERATION_CATALOG`,
        // so it answers without a server and never mutates anything.
        "agent-tools" => (
            "agenterm cli agent-tools [--format agenterm|mcp] [--include-unavailable]",
            &["--format"][..],
            &["--include-unavailable"][..],
            false,
        ),
        "attach" | "attach-session" => (
            "agenterm cli attach-session [-t session]",
            &["-t"][..],
            &[][..],
            false,
        ),
        "active-window" | "active-tab" => (
            "agenterm cli active-window [-F format]",
            &["-F"][..],
            &[][..],
            false,
        ),
        "capture-pane" | "capturep" => (
            "agenterm cli capture-pane (-p|--raw-escaped) [-t target] \
             [--max-bytes N --json]",
            &["-t", "--max-bytes"][..],
            &["-p", "--raw-escaped", "--json"][..],
            false,
        ),
        "control-center" => (
            "agenterm cli control-center open|status|snapshot|close [--no-activate]",
            &[][..],
            &["--no-activate"][..],
            false,
        ),
        "display-message" | "display" => (
            "agenterm cli display-message [-p] [-t target] [format]",
            &["-t"][..],
            &["-p"][..],
            false,
        ),
        "dump-cells" => (
            "agenterm cli dump-cells [-t target] [-r row]",
            &["-t", "-r"][..],
            &[][..],
            false,
        ),
        "get-settings" => ("agenterm cli get-settings", &[][..], &[][..], false),
        "has-session" | "has" => (
            "agenterm cli has-session [-t session]",
            &["-t"][..],
            &[][..],
            false,
        ),
        "inspect" | "pane-snapshot" => (
            "agenterm cli inspect [-t target]",
            &["-t"][..],
            &[][..],
            false,
        ),
        "focus" => (
            "agenterm cli focus terminal|composer|sidebar [-t target]",
            &["-t"][..],
            &[][..],
            false,
        ),
        "kill-server" => ("agenterm cli kill-server", &[][..], &[][..], false),
        "kill-session" => (
            "agenterm cli kill-session [-t session]",
            &["-t"][..],
            &[][..],
            false,
        ),
        "kill-window" | "killw" => (
            "agenterm cli kill-window -t target",
            &["-t"][..],
            &[][..],
            false,
        ),
        "list-tab-tree" => (
            "agenterm cli list-tab-tree [-F format]",
            &["-F"][..],
            &[][..],
            false,
        ),
        "list-commands" | "lscm" => ("agenterm cli list-commands", &[][..], &[][..], false),
        "list-instances" => (
            "agenterm cli list-instances [--json] [--prune]",
            &[][..],
            &["--json", "--prune"][..],
            false,
        ),
        "list-panes" | "lsp" => (
            "agenterm cli list-panes [-a] [-t target] [-F format]",
            &["-t", "-F"][..],
            &["-a"][..],
            false,
        ),
        "list-sessions" | "ls" => ("agenterm cli list-sessions", &[][..], &[][..], false),
        "list-windows" | "lsw" => (
            "agenterm cli list-windows [-F format]",
            &["-F"][..],
            &[][..],
            false,
        ),
        "new-session" | "new" => (
            "agenterm cli new-session [-s name] [-- command [args...]]",
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
            "agenterm cli new-window [-d] [-n name] [--parent target] \
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
            "agenterm cli new-agent [-d] [-n name] [--parent target] [--program exe] \
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
        "next-window" | "next" => ("agenterm cli next-window", &[][..], &[][..], false),
        "previous-window" | "prev" => ("agenterm cli previous-window", &[][..], &[][..], false),
        "protocol-info" => (
            "agenterm cli protocol-info [--running]",
            &[][..],
            &["--running"][..],
            false,
        ),
        "rh-pack" => (
            "agenterm cli rh-pack --path PATH [--json]",
            &["--path"][..],
            &["--json"][..],
            false,
        ),
        "rename-session" | "rename" => (
            "agenterm cli rename-session new-name",
            &[][..],
            &[][..],
            false,
        ),
        "rename-window" | "renamew" => (
            "agenterm cli rename-window [-t target] new-name",
            &["-t"][..],
            &[][..],
            false,
        ),
        "screenshot" => (
            "agenterm cli screenshot [-o file.png]",
            &["-o"][..],
            &[][..],
            false,
        ),
        "screenshot-pane" | "screenshot-tab" => (
            "agenterm cli screenshot-pane [-t target] [-o file.png]",
            &["-t", "-o"][..],
            &[][..],
            false,
        ),
        "save-workspace" => ("agenterm cli save-workspace", &[][..], &[][..], false),
        "script" => (
            "agenterm cli script api [MODULE] [--status shipped|planned|all] [--tree|--json] | \
             check FILE|- [--project-root DIR] | eval EXPRESSION | \
             check-many --manifest FILE [--project-root DIR] | \
             repl [--fail-fast] [--json] | run FILE|- \
             [--cwd DIR] [--project-root DIR] [-- ARGS...] | \
            task list|show|run [TASK] [--manifest FILE] [--json]",
            &[
                // Legacy compatibility label. It is accepted but intentionally
                // omitted from public usage because it never changes runtime APIs.
                "--profile",
                "--timeout-ms",
                "--max-operations",
                "--max-collection-items",
                "--max-string-bytes",
                "--max-output-bytes",
                "--max-source-bytes",
                "--cwd",
                "--project-root",
                "--manifest",
                "--status",
            ][..],
            &["--tree", "--json", "--fail-fast"][..],
            false,
        ),
        "read-events" => (
            "agenterm cli read-events --epoch EPOCH --after SEQUENCE [--limit COUNT]",
            &["--epoch", "--after", "--limit"][..],
            &[][..],
            false,
        ),
        "scroll-pane" => (
            "agenterm cli scroll-pane [-t target] \
             up|down|page-up|page-down|top|bottom [rows]",
            &["-t"][..],
            &[][..],
            false,
        ),
        "select-window" | "selectw" => (
            "agenterm cli select-window (-t target|-n|-p)",
            &["-t"][..],
            &["-n", "-p"][..],
            false,
        ),
        "send-keys" | "send" => (
            "agenterm cli send-keys [-t target] [-l|--native] key...\n\
             PowerShell: quote targets as -t '@2' (unquoted @N is a splat).",
            &["-t"][..],
            &["-l", "--native", "-R", "-X"][..],
            false,
        ),
        "set-buffer" | "setb" => (
            "agenterm cli set-buffer [-b name] [--] data...",
            &["-b"][..],
            &[][..],
            false,
        ),
        "load-buffer" | "loadb" => (
            "agenterm cli load-buffer [-b name] path",
            &["-b"][..],
            &[][..],
            false,
        ),
        "show-buffer" | "showb" => (
            "agenterm cli show-buffer [-b name]",
            &["-b"][..],
            &[][..],
            false,
        ),
        "list-buffers" | "lsb" => ("agenterm cli list-buffers", &[][..], &[][..], false),
        "delete-buffer" | "deleteb" => (
            "agenterm cli delete-buffer [-b name]",
            &["-b"][..],
            &[][..],
            false,
        ),
        "paste-buffer" | "pasteb" => (
            "agenterm cli paste-buffer [-b name] [-t target]\n\
             Injects buffer bytes into the target pane PTY (not an agent mailbox).\n\
             Empty buffers fail; UTF-8 text is normalized and respects bracketed-paste.\n\
             Collab/status → note/handoff; shell typing → send-keys/paste-buffer \
             (see PRD_02_15 B′ vs agent messaging).",
            &["-b", "-t"][..],
            &[][..],
            false,
        ),
        "save-buffer" | "saveb" => (
            "agenterm cli save-buffer is unsupported: use show-buffer or load-buffer",
            &[][..],
            &[][..],
            false,
        ),
        "send-composer" => (
            "agenterm cli send-composer [-t target]",
            &["-t"][..],
            &[][..],
            false,
        ),
        "send-mouse" => (
            "agenterm cli send-mouse [-t target] -x col -y row \
             [--button button] [--action action] [--protocol protocol]",
            &["-t", "-x", "-y", "--button", "--action", "--protocol"][..],
            &[][..],
            false,
        ),
        "server-kill" => ("agenterm cli server-kill", &[][..], &[][..], false),
        "server-list" => (
            "agenterm cli server-list [--json] [--prune]",
            &[][..],
            &["--json", "--prune"][..],
            false,
        ),
        "set-setting" => (
            "agenterm cli set-setting key value",
            &[][..],
            &[][..],
            false,
        ),
        "set-composer" => (
            "agenterm cli set-composer [-t target] (text|--stdin|--file path)",
            &["-t", "--file"][..],
            &["--stdin"][..],
            false,
        ),
        "set-tab-parent" => (
            "agenterm cli set-tab-parent -t child --parent parent|root",
            &["-t", "--parent"][..],
            &[][..],
            false,
        ),
        "set-tab-note" => (
            "agenterm cli set-tab-note [-t target] text",
            &["-t"][..],
            &[][..],
            false,
        ),
        "show-composer" => (
            "agenterm cli show-composer [-t target]",
            &["-t"][..],
            &[][..],
            false,
        ),
        "show-options" | "show" => ("agenterm cli show-options", &[][..], &[][..], false),
        "show-tab-parent" => (
            "agenterm cli show-tab-parent [-t target]",
            &["-t"][..],
            &[][..],
            false,
        ),
        "show-tab-note" => (
            "agenterm cli show-tab-note [-t target]",
            &["-t"][..],
            &[][..],
            false,
        ),
        "shutdown" => ("agenterm cli shutdown", &[][..], &[][..], false),
        "start-server" => ("agenterm cli start-server", &[][..], &[][..], false),
        "ui-action" => (
            "agenterm cli ui-action ACTION [-t target] [--path PATH] \
             [--mode empty|append|replace|attach|open-another] [--name NAME] [--pid N] \
             [--stdin] [--width PX --height PX]",
            &[
                "-t",
                "--path",
                "--mode",
                "--name",
                "--logical-instance",
                "--pid",
                "--proxy-input",
                "--width",
                "--height",
            ][..],
            &["--stdin"][..],
            false,
        ),
        "ui-bootstrap" => ("agenterm cli ui-bootstrap", &[][..], &[][..], false),
        "ui-input" => (
            "agenterm cli ui-input pointer --x PX --y PX \
             [--button left|right|middle] [--action press|release|move] \
             [--count 1|2|3] [--mods shift,ctrl,alt,meta] | \
             ui-input wheel --x PX --y PX --delta-y N [--units lines|pixels] | \
             ui-input key --key NAME [--mods shift,ctrl,alt,meta]",
            &[
                "--x",
                "--y",
                "--button",
                "--action",
                "--count",
                "--mods",
                "--delta-y",
                "--units",
                "--key",
            ][..],
            &[][..],
            false,
        ),
        "ui-client-state" => (
            "agenterm cli ui-client-state publish --lease-id ID \
             --client-pid PID --snapshot-json JSON",
            &["--lease-id", "--client-pid", "--snapshot-json"][..],
            &[][..],
            false,
        ),
        "ui-client-command" => (
            "agenterm cli ui-client-command poll|apply|complete|result \
             [--lease-id ID --client-pid PID] [--command-id ID] \
             [--response-json JSON]",
            &[
                "--lease-id",
                "--client-pid",
                "--command-id",
                "--response-json",
                "--args-json",
            ][..],
            &["--detach", "--shutdown-after-result"][..],
            false,
        ),
        "ui-deltas" => (
            "agenterm cli ui-deltas --epoch EPOCH --after SEQUENCE [--limit 1..64]",
            &["--epoch", "--after", "--limit"][..],
            &[][..],
            false,
        ),
        "ui-hello" => (
            "agenterm cli ui-hello --minimum VERSION --maximum VERSION \
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
            "agenterm cli ui-interact (select|input|resize) \
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
            "agenterm cli ui-lease \
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
        "ui-snapshot" => ("agenterm cli ui-snapshot", &[][..], &[][..], false),
        "wait-pane" | "expect-pane" => (
            "agenterm cli wait-pane [-t target] \
             (--contains text|--dead|--submit-complete|--finalized) [--timeout-ms ms]",
            &["-t", "--contains", "--timeout-ms"][..],
            &["--dead", "--submit-complete", "--finalized"][..],
            false,
        ),
        "wait-events" => (
            "agenterm cli wait-events --epoch EPOCH --after SEQUENCE --kind KIND \
             [--tab @ID] [--timeout-ms MS]",
            &["--epoch", "--after", "--kind", "--tab", "--timeout-ms"][..],
            &[][..],
            false,
        ),
        "wait-ui" => (
            "agenterm cli wait-ui [--active @id] [--focus surface] \
             [-t target --tab-state state] [--window-state state] \
             [-t target --proxy-state state] \
             [--client-width PX --client-height PX] \
             [--terminal-grid-changed-from ROWSxCOLS] \
             [--modal-kind KIND|none|closed] [--modal-target target] \
             [-t target --tab-editor-state open|closed] \
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
                "--terminal-grid-changed-from",
                "--modal-kind",
                "--modal-target",
                "--tab-editor-state",
                "--timeout-ms",
            ][..],
            &[][..],
            false,
        ),
        "workspace-info" => ("agenterm cli workspace-info", &[][..], &[][..], false),
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
            // Explicit generic overlays remain supported, except that AgenTerm
            // temporarily does not override inherited HTTP(S) proxy settings.
            if !is_http_proxy_environment(name) {
                upsert_environment(&mut environment, name, value);
            }
            position += 2;
        } else if argument == "--proxy" {
            let value = args
                .get(position + 1)
                .ok_or_else(|| "--proxy requires a URL".to_owned())?;
            if value.is_empty() {
                return Err("--proxy requires a non-empty URL".to_owned());
            }
            validate_environment_value(value, "--proxy")?;
            // Temporarily leave inherited HTTP(S) proxy variables untouched.
            // Keep consuming and validating this compatibility option until a
            // later proxy design defines explicit child-environment semantics.
            // upsert_environment(&mut environment, "HTTP_PROXY", value);
            // upsert_environment(&mut environment, "HTTPS_PROXY", value);
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

fn is_http_proxy_environment(name: &str) -> bool {
    name.eq_ignore_ascii_case("HTTP_PROXY") || name.eq_ignore_ascii_case("HTTPS_PROXY")
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
        "IC" | "Insert" => b"\x1b[2~".as_slice(),
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

/// xterm's modifier parameter for CSI sequences: 1 + (shift=1, alt=2, ctrl=4).
/// Returns `None` when no modifier is held, so callers can fall back to the
/// unmodified escape sequence instead of emitting a redundant `;1`.
fn xterm_modifier_code(modifiers: ModifierState) -> Option<u8> {
    let mut code = 1u8;
    if modifiers.shift {
        code += 1;
    }
    if modifiers.alt {
        code += 2;
    }
    if modifiers.control {
        code += 4;
    }
    (code != 1).then_some(code)
}

/// Same as [`tmux_key_bytes`] but modifier-aware: named keys that carry a
/// distinct escape sequence per modifier (arrows, Home/End, PageUp/PageDown,
/// Delete, function keys, Tab) get the xterm `CSI ...;<mod>~`/`CSI 1;<mod><L>`
/// variant instead of silently degrading to the unmodified key. Live keyboard
/// input should go through this; `tmux_key_bytes` alone is for callers (like
/// the `send-keys` control command) that only ever have a bare key name.
pub(crate) fn tmux_key_bytes_with_modifiers(
    key: &str,
    modifiers: ModifierState,
) -> Option<Vec<u8>> {
    if key == "Tab" {
        return Some(if modifiers.shift {
            b"\x1b[Z".to_vec()
        } else {
            b"\t".to_vec()
        });
    }
    let Some(code) = xterm_modifier_code(modifiers) else {
        return tmux_key_bytes(key);
    };
    let bytes = match key {
        "Up" => format!("\x1b[1;{code}A").into_bytes(),
        "Down" => format!("\x1b[1;{code}B").into_bytes(),
        "Right" => format!("\x1b[1;{code}C").into_bytes(),
        "Left" => format!("\x1b[1;{code}D").into_bytes(),
        "Home" => format!("\x1b[1;{code}H").into_bytes(),
        "End" => format!("\x1b[1;{code}F").into_bytes(),
        "IC" | "Insert" => format!("\x1b[2;{code}~").into_bytes(),
        "DC" | "Delete" => format!("\x1b[3;{code}~").into_bytes(),
        "PPage" | "PageUp" => format!("\x1b[5;{code}~").into_bytes(),
        "NPage" | "PageDown" => format!("\x1b[6;{code}~").into_bytes(),
        "F1" => format!("\x1b[1;{code}P").into_bytes(),
        "F2" => format!("\x1b[1;{code}Q").into_bytes(),
        "F3" => format!("\x1b[1;{code}R").into_bytes(),
        "F4" => format!("\x1b[1;{code}S").into_bytes(),
        "F5" => format!("\x1b[15;{code}~").into_bytes(),
        "F6" => format!("\x1b[17;{code}~").into_bytes(),
        "F7" => format!("\x1b[18;{code}~").into_bytes(),
        "F8" => format!("\x1b[19;{code}~").into_bytes(),
        "F9" => format!("\x1b[20;{code}~").into_bytes(),
        "F10" => format!("\x1b[21;{code}~").into_bytes(),
        "F11" => format!("\x1b[23;{code}~").into_bytes(),
        "F12" => format!("\x1b[24;{code}~").into_bytes(),
        _ => return tmux_key_bytes(key),
    };
    Some(bytes)
}

/// Convert a local wheel gesture into cursor-key input for an application that
/// owns the alternate screen and therefore has no local scrollback viewport.
/// The bounded repeat preserves high-resolution wheel accumulation without
/// allowing one malformed delta to enqueue unbounded PTY input.
pub(crate) fn alternate_screen_wheel_bytes(
    up: bool,
    rows: usize,
    application_cursor: bool,
) -> Vec<u8> {
    let sequence: &[u8] = match (up, application_cursor) {
        (true, true) => b"\x1bOA",
        (false, true) => b"\x1bOB",
        (true, false) => b"\x1b[A",
        (false, false) => b"\x1b[B",
    };
    sequence.repeat(rows.min(120))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn save_buffer_is_explicitly_unsupported_on_mux_surface() {
        let save = mux_command("save-buffer").expect("save-buffer registered");
        let alias = mux_command("saveb").expect("saveb registered");
        assert!(matches!(
            save.status,
            MuxStatus::Unsupported(reason) if reason.contains("save-buffer is not implemented")
        ));
        assert_eq!(save.status, alias.status);
        assert!(
            control_command_usage("save-buffer").is_some_and(|usage| usage.contains("unsupported")),
            "CLI help should document save-buffer as unsupported"
        );
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
    fn shift_tab_sends_back_tab_instead_of_plain_tab() {
        let shift = ModifierState {
            shift: true,
            ..ModifierState::empty()
        };
        assert_eq!(
            tmux_key_bytes_with_modifiers("Tab", shift),
            Some(b"\x1b[Z".to_vec())
        );
        assert_eq!(
            tmux_key_bytes_with_modifiers("Tab", ModifierState::empty()),
            Some(b"\t".to_vec())
        );
    }

    #[test]
    fn modified_named_keys_get_xterm_csi_variants() {
        let shift = ModifierState {
            shift: true,
            ..ModifierState::empty()
        };
        let ctrl = ModifierState {
            control: true,
            ..ModifierState::empty()
        };
        assert_eq!(
            tmux_key_bytes_with_modifiers("Up", shift),
            Some(b"\x1b[1;2A".to_vec())
        );
        assert_eq!(
            tmux_key_bytes_with_modifiers("Right", ctrl),
            Some(b"\x1b[1;5C".to_vec())
        );
        assert_eq!(
            tmux_key_bytes_with_modifiers("Delete", shift),
            Some(b"\x1b[3;2~".to_vec())
        );
        assert_eq!(
            tmux_key_bytes_with_modifiers("Insert", ctrl),
            Some(b"\x1b[2;5~".to_vec())
        );
        assert_eq!(
            tmux_key_bytes_with_modifiers("F5", ctrl),
            Some(b"\x1b[15;5~".to_vec())
        );
    }

    #[test]
    fn unmodified_named_keys_fall_back_to_tmux_key_bytes() {
        assert_eq!(
            tmux_key_bytes_with_modifiers("Up", ModifierState::empty()),
            tmux_key_bytes("Up")
        );
        assert_eq!(
            tmux_key_bytes_with_modifiers("not-a-key", ModifierState::empty()),
            None
        );
    }

    #[test]
    fn alternate_screen_wheel_respects_cursor_mode_and_bounds_input() {
        assert_eq!(
            alternate_screen_wheel_bytes(true, 2, false),
            b"\x1b[A\x1b[A"
        );
        assert_eq!(
            alternate_screen_wheel_bytes(false, 2, true),
            b"\x1bOB\x1bOB"
        );
        assert_eq!(alternate_screen_wheel_bytes(true, 0, true), b"");
        assert_eq!(
            alternate_screen_wheel_bytes(true, usize::MAX, false).len(),
            120 * 3
        );
    }

    #[test]
    fn parses_scoped_environment_without_intervening_in_http_proxy() {
        let parsed = parse_tab_environment(&args(&[
            "new-window",
            "-e",
            "ROLE=reviewer",
            "-e",
            "http_proxy=http://explicit.example:8080",
            "--env",
            "HTTPS_PROXY=https://explicit.example:8443",
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
        assert!(error.contains("--endpoint ENDPOINT"));
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
