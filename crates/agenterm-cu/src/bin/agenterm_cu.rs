//! `agenterm-cu` shell command (PRD_02_29 shell layer).
//!
//! Machine-readable JSON on stdout; human usage on stderr.

use std::path::PathBuf;

use agenterm_cu::{
    Authorization, Command, Executor, PointerButton, SshEndpoint, TargetRef, VncEndpoint,
    WaitCondition,
};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if matches!(args.as_slice(), [arg] if arg == "--version" || arg == "-V") {
        println!("agenterm-cu {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    if matches!(args.first().map(String::as_str), Some("host" | "hotkeys")) {
        std::process::exit(agenterm_cu::hotkeys::run());
    }
    if args.first().map(String::as_str)
        == Some(agenterm_cu::mechanism::clipboard::X11_CLIPBOARD_OWNER_ARG)
    {
        std::process::exit(run_x11_clipboard_owner());
    }
    let reply = dispatch(args);
    let json = serde_json::to_string(&reply).unwrap_or_else(|_| {
        r#"{"ok":false,"target":"","command":"","error":{"code":"serialize","message":"reply serialization failed"}}"#
            .to_string()
    });
    println!("{json}");
}

fn dispatch(mut args: Vec<String>) -> agenterm_cu::CuReply {
    if args.is_empty() || matches!(args[0].as_str(), "help" | "--help" | "-h") {
        eprint_usage();
        return help_reply(true);
    }

    let mut grant: Option<String> = None;
    let mut target: Option<TargetRef> = None;
    let mut ssh_dest: Option<String> = None;
    let mut ssh_port: Option<u16> = None;
    let mut ssh_identity: Option<PathBuf> = None;
    let mut ssh_cu: Option<PathBuf> = None;
    let mut ssh_env: Vec<(String, String)> = Vec::new();
    let mut vnc_dest: Option<String> = None;
    let mut vnc_port: Option<u16> = None;
    let mut vnc_cu: Option<PathBuf> = None;
    let mut vnc_env: Vec<(String, String)> = Vec::new();
    while let Some(flag) = args.first() {
        match flag.as_str() {
            "--target" => {
                let value = take_value(&mut args, "--target");
                target = TargetRef::parse(&value).or_else(|| {
                    eprint_usage();
                    None
                });
                if target.is_none() {
                    return usage_err(
                        "unknown --target value; supported: 'current', 'ssh', and 'vnc'",
                    );
                }
            }
            "--ssh" => {
                let value = take_value(&mut args, "--ssh");
                if value.is_empty() {
                    return usage_err("--ssh requires <user@host>");
                }
                ssh_dest = Some(value);
                if target.is_none() {
                    target = Some(TargetRef::Ssh);
                }
            }
            "--ssh-port" => {
                let value = take_value(&mut args, "--ssh-port");
                match value.parse::<u16>() {
                    Ok(port) => ssh_port = Some(port),
                    Err(_) => return usage_err("--ssh-port requires a TCP port number"),
                }
            }
            "--ssh-identity" => {
                let value = take_value(&mut args, "--ssh-identity");
                if value.is_empty() {
                    return usage_err("--ssh-identity requires a private-key path");
                }
                ssh_identity = Some(PathBuf::from(value));
            }
            "--ssh-cu" => {
                let value = take_value(&mut args, "--ssh-cu");
                if value.is_empty() {
                    return usage_err("--ssh-cu requires a remote agenterm-cu path");
                }
                ssh_cu = Some(PathBuf::from(value));
            }
            "--ssh-env" => {
                let value = take_value(&mut args, "--ssh-env");
                let Some((key, val)) = value.split_once('=') else {
                    return usage_err("--ssh-env requires KEY=VAL");
                };
                if key.is_empty() {
                    return usage_err("--ssh-env requires a non-empty KEY");
                }
                ssh_env.push((key.to_owned(), val.to_owned()));
            }
            "--vnc" => {
                let value = take_value(&mut args, "--vnc");
                if value.is_empty() {
                    return usage_err("--vnc requires <host[:port]>");
                }
                vnc_dest = Some(value);
                if target.is_none() {
                    target = Some(TargetRef::Vnc);
                }
            }
            "--vnc-port" => {
                let value = take_value(&mut args, "--vnc-port");
                match value.parse::<u16>() {
                    Ok(port) => vnc_port = Some(port),
                    Err(_) => return usage_err("--vnc-port requires a TCP port number"),
                }
            }
            "--vnc-cu" => {
                let value = take_value(&mut args, "--vnc-cu");
                if value.is_empty() {
                    return usage_err(
                        "--vnc-cu requires an agenterm-cu path for the session worker",
                    );
                }
                vnc_cu = Some(PathBuf::from(value));
            }
            "--vnc-env" => {
                let value = take_value(&mut args, "--vnc-env");
                let Some((key, val)) = value.split_once('=') else {
                    return usage_err("--vnc-env requires KEY=VAL");
                };
                if key.is_empty() {
                    return usage_err("--vnc-env requires a non-empty KEY");
                }
                vnc_env.push((key.to_owned(), val.to_owned()));
            }
            "--grant" => {
                grant = Some(take_value(&mut args, "--grant"));
            }
            _ if flag.starts_with('-') => {
                return usage_err(format!("unknown global flag '{flag}'"));
            }
            _ => break,
        }
    }

    // Allow global flags before `exec` so remote workers can be invoked as
    // `agenterm-cu --grant observe exec --json -` as well as `exec` first.
    if args.first().map(String::as_str) == Some("exec") {
        let mut exec_args: Vec<String> = Vec::new();
        if let Some(raw) = grant.as_ref() {
            exec_args.push(format!("--grant={raw}"));
        }
        exec_args.extend(args.into_iter().skip(1));
        return dispatch_json(&exec_args);
    }

    if target.is_none()
        && let Ok(dest) = std::env::var("AGENTERM_CU_SSH")
        && !dest.is_empty()
    {
        ssh_dest = Some(dest);
        target = Some(TargetRef::Ssh);
    }
    if target.is_none()
        && let Ok(dest) = std::env::var("AGENTERM_CU_VNC")
        && !dest.is_empty()
    {
        vnc_dest = Some(dest);
        target = Some(TargetRef::Vnc);
    }

    let Some(target) = target else {
        eprint_usage();
        return usage_err(
            "--target current, --ssh <user@host>, or --vnc <host[:port]> is required on every command",
        );
    };

    if target == TargetRef::Current && ssh_dest.is_some() {
        return usage_err("--ssh cannot be combined with --target current");
    }
    if target == TargetRef::Current && vnc_dest.is_some() {
        return usage_err("--vnc cannot be combined with --target current");
    }
    if target == TargetRef::Ssh && vnc_dest.is_some() {
        return usage_err("--vnc cannot be combined with --target ssh / --ssh");
    }
    if target == TargetRef::Vnc && ssh_dest.is_some() {
        return usage_err("--ssh cannot be combined with --target vnc / --vnc");
    }
    if target == TargetRef::Ssh
        && ssh_dest.is_none()
        && let Ok(dest) = std::env::var("AGENTERM_CU_SSH")
        && !dest.is_empty()
    {
        ssh_dest = Some(dest);
    }
    if target == TargetRef::Ssh && ssh_dest.is_none() {
        return usage_err("ssh target requires --ssh <user@host> (or AGENTERM_CU_SSH)");
    }
    if target == TargetRef::Vnc
        && vnc_dest.is_none()
        && let Ok(dest) = std::env::var("AGENTERM_CU_VNC")
        && !dest.is_empty()
    {
        vnc_dest = Some(dest);
    }
    if target == TargetRef::Vnc && vnc_dest.is_none() {
        return usage_err("vnc target requires --vnc <host[:port]> (or AGENTERM_CU_VNC)");
    }

    let Some(verb) = args.first().cloned() else {
        eprint_usage();
        return usage_err("missing command verb");
    };
    args.remove(0);

    let command = match verb.as_str() {
        "capabilities" => Command::Capabilities { target },
        "windows" => Command::Windows { target },
        "tree" => {
            let window = flag_isize(&mut args, "--window");
            Command::Tree { target, window }
        }
        "screenshot" => {
            let path = flag_value(&mut args, "--out")
                .or_else(|| args.first().cloned())
                .unwrap_or_default();
            if !args.is_empty() {
                args.remove(0);
            }
            let window = flag_isize(&mut args, "--window");
            Command::Screenshot {
                target,
                path,
                window,
            }
        }
        "click" => {
            let window = flag_isize(&mut args, "--window");
            let node = flag_value(&mut args, "--node");
            let name = flag_value(&mut args, "--name");
            let role = flag_value(&mut args, "--role");
            let coords = flag_coords(&mut args, "--coords");
            let degraded = args.iter().any(|arg| arg == "--degraded");
            args.retain(|arg| arg != "--degraded");
            let clicks = flag_u32(&mut args, "--clicks").unwrap_or(1);
            let button = match flag_value(&mut args, "--button").as_deref() {
                Some("right") => PointerButton::Right,
                Some("middle") => PointerButton::Middle,
                _ => PointerButton::Left,
            };
            Command::Click {
                target,
                window,
                node,
                name,
                role,
                coords,
                degraded,
                clicks,
                button,
            }
        }
        "focus" => {
            let window = flag_isize(&mut args, "--window");
            let node = flag_value(&mut args, "--node");
            let name = flag_value(&mut args, "--name");
            let role = flag_value(&mut args, "--role");
            if node.as_ref().is_none_or(|value| value.is_empty())
                && name.as_ref().is_none_or(|value| value.is_empty())
            {
                return usage_err(
                    "focus requires --node <path-id> or --window <handle> --name <pattern>",
                );
            }
            Command::Focus {
                target,
                window,
                node,
                name,
                role,
            }
        }
        "send-text" => {
            // `--` ends flag parsing so the text may itself start with a dash.
            let literal_text = match args.iter().position(|arg| arg == "--") {
                Some(index) => Some(args.split_off(index)[1..].join(" ")),
                None => None,
            };
            let window = flag_isize(&mut args, "--window");
            let name = flag_value(&mut args, "--name");
            let role = flag_value(&mut args, "--role");
            Command::SendText {
                target,
                text: literal_text.unwrap_or_else(|| args.join(" ")),
                window,
                name,
                role,
            }
        }
        "copy" => {
            let window = flag_isize(&mut args, "--window");
            let name = flag_value(&mut args, "--name");
            let role = flag_value(&mut args, "--role");
            if window.is_none() {
                return usage_err("copy requires --window <handle> [--name <pattern>]");
            }
            if name.as_ref().is_none_or(|value| value.is_empty())
                && role.as_ref().is_some_and(|value| !value.is_empty())
            {
                return usage_err("copy --role requires --name <pattern>");
            }
            Command::Copy {
                target,
                window,
                name,
                role,
            }
        }
        "paste" => {
            // `--` ends flag parsing so --text may itself start with a dash.
            let literal_text = match args.iter().position(|arg| arg == "--") {
                Some(index) => Some(args.split_off(index)[1..].join(" ")),
                None => None,
            };
            let window = flag_isize(&mut args, "--window");
            let name = flag_value(&mut args, "--name");
            let role = flag_value(&mut args, "--role");
            let text = flag_value(&mut args, "--text").or(literal_text);
            if window.is_none() {
                return usage_err(
                    "paste requires --window <handle> [--name <pattern>] [--text TEXT]",
                );
            }
            if name.as_ref().is_none_or(|value| value.is_empty())
                && role.as_ref().is_some_and(|value| !value.is_empty())
            {
                return usage_err("paste --role requires --name <pattern>");
            }
            Command::Paste {
                target,
                text,
                window,
                name,
                role,
            }
        }
        "send-keys" => {
            // `--` ends flag parsing so a chord may itself start with a dash.
            let literal_keys = match args.iter().position(|arg| arg == "--") {
                Some(index) => Some(args.split_off(index)[1..].join("+")),
                None => None,
            };
            let window = flag_isize(&mut args, "--window");
            let name = flag_value(&mut args, "--name");
            let role = flag_value(&mut args, "--role");
            Command::SendKeys {
                target,
                keys: literal_keys.unwrap_or_else(|| args.join("+")),
                window,
                name,
                role,
            }
        }
        "scroll" => {
            let window = flag_isize(&mut args, "--window");
            let name = flag_value(&mut args, "--name");
            let role = flag_value(&mut args, "--role");
            if name.as_ref().is_none_or(|value| value.is_empty()) {
                return usage_err("scroll requires --window <handle> --name <pattern>");
            }
            Command::Scroll {
                target,
                window,
                name,
                role,
            }
        }
        "get-extents" => {
            let window = flag_isize(&mut args, "--window");
            let name = flag_value(&mut args, "--name");
            let role = flag_value(&mut args, "--role");
            if name.as_ref().is_none_or(|value| value.is_empty()) {
                return usage_err("get-extents requires --window <handle> --name <pattern>");
            }
            Command::GetExtents {
                target,
                window,
                name,
                role,
            }
        }
        "select" => {
            let window = flag_isize(&mut args, "--window");
            let name = flag_value(&mut args, "--name");
            let role = flag_value(&mut args, "--role");
            let start = flag_i32(&mut args, "--start");
            let end = flag_i32(&mut args, "--end");
            if name.as_ref().is_none_or(|value| value.is_empty())
                || start.is_none()
                || end.is_none()
            {
                return usage_err(
                    "select requires --window <handle> --name <pattern> --start N --end M",
                );
            }
            Command::Select {
                target,
                start: start.unwrap_or(0),
                end: end.unwrap_or(0),
                window,
                name,
                role,
            }
        }
        "get-selection" => {
            let window = flag_isize(&mut args, "--window");
            let name = flag_value(&mut args, "--name");
            let role = flag_value(&mut args, "--role");
            if name.as_ref().is_none_or(|value| value.is_empty()) {
                return usage_err("get-selection requires --window <handle> --name <pattern>");
            }
            Command::GetSelection {
                target,
                window,
                name,
                role,
            }
        }
        "set-caret" => {
            let window = flag_isize(&mut args, "--window");
            let name = flag_value(&mut args, "--name");
            let role = flag_value(&mut args, "--role");
            let offset = flag_i32(&mut args, "--offset");
            if name.as_ref().is_none_or(|value| value.is_empty()) || offset.is_none() {
                return usage_err(
                    "set-caret requires --window <handle> --name <pattern> --offset N",
                );
            }
            Command::SetCaret {
                target,
                offset: offset.unwrap_or(0),
                window,
                name,
                role,
            }
        }
        "get-caret" => {
            let window = flag_isize(&mut args, "--window");
            let name = flag_value(&mut args, "--name");
            let role = flag_value(&mut args, "--role");
            if name.as_ref().is_none_or(|value| value.is_empty()) {
                return usage_err("get-caret requires --window <handle> --name <pattern>");
            }
            Command::GetCaret {
                target,
                window,
                name,
                role,
            }
        }
        "get-text" => {
            let window = flag_isize(&mut args, "--window");
            let name = flag_value(&mut args, "--name");
            let role = flag_value(&mut args, "--role");
            if window.is_none() && name.as_ref().is_none_or(|value| value.is_empty()) {
                return usage_err("get-text requires --window <handle> [--name <pattern>]");
            }
            Command::GetText {
                target,
                window,
                name,
                role,
            }
        }
        "window-place" => {
            let action = flag_value(&mut args, "--action")
                .or_else(|| args.first().cloned())
                .unwrap_or_default();
            if action.is_empty() {
                return usage_err("window-place requires --action <id>");
            }
            let window = flag_isize(&mut args, "--window");
            Command::WindowPlace {
                target,
                action,
                window,
            }
        }
        "wait" => {
            // `--` ends flag parsing so --text-equals / --text-contains may start with a dash.
            let literal_text = match args.iter().position(|arg| arg == "--") {
                Some(index) => Some(args.split_off(index)[1..].join(" ")),
                None => None,
            };
            let timeout_ms = flag_u64(&mut args, "--timeout-ms").unwrap_or(5_000);
            let text_equals_present = args
                .iter()
                .any(|arg| arg == "--text-equals" || arg == "--node-text-equals");
            let text_contains_present = args
                .iter()
                .any(|arg| arg == "--text-contains" || arg == "--node-text-contains");
            let condition = if text_equals_present && text_contains_present {
                return usage_err("wait accepts one of --text-equals or --text-contains, not both");
            } else if text_equals_present {
                let expected = flag_value(&mut args, "--text-equals")
                    .or_else(|| flag_value(&mut args, "--node-text-equals"))
                    .filter(|value| value != "--")
                    .or(literal_text);
                let Some(expected) = expected else {
                    return usage_err(
                        "wait --text-equals / --node-text-equals requires the expected text",
                    );
                };
                let name = flag_value(&mut args, "--name")
                    .or_else(|| flag_value(&mut args, "--node-name-contains"))
                    .filter(|value| !value.is_empty());
                let Some(name) = name else {
                    return usage_err("wait --text-equals requires --name <pattern>");
                };
                WaitCondition::NodeTextEquals {
                    expected,
                    name,
                    role: flag_value(&mut args, "--role")
                        .or_else(|| flag_value(&mut args, "--node-role")),
                    window: flag_isize(&mut args, "--window"),
                }
            } else if text_contains_present {
                let substring = flag_value(&mut args, "--text-contains")
                    .or_else(|| flag_value(&mut args, "--node-text-contains"))
                    .filter(|value| value != "--")
                    .or(literal_text);
                let Some(substring) = substring else {
                    return usage_err(
                        "wait --text-contains / --node-text-contains requires the substring",
                    );
                };
                let name = flag_value(&mut args, "--name")
                    .or_else(|| flag_value(&mut args, "--node-name-contains"))
                    .filter(|value| !value.is_empty());
                let Some(name) = name else {
                    return usage_err("wait --text-contains requires --name <pattern>");
                };
                WaitCondition::NodeTextContains {
                    substring,
                    name,
                    role: flag_value(&mut args, "--role")
                        .or_else(|| flag_value(&mut args, "--node-role")),
                    window: flag_isize(&mut args, "--window"),
                }
            } else if let Some(count) = flag_usize(&mut args, "--window-count-gte") {
                WaitCondition::WindowCountGte { count }
            } else if let Some(pattern) = flag_value(&mut args, "--window-title-contains") {
                WaitCondition::WindowTitleContains { pattern }
            } else if let Some(handle) = flag_isize(&mut args, "--focused-handle") {
                WaitCondition::FocusedHandle { handle }
            } else if let Some(pattern) = flag_value(&mut args, "--node-name-contains") {
                WaitCondition::NodeNameContains {
                    pattern,
                    role: flag_value(&mut args, "--node-role"),
                    window: flag_isize(&mut args, "--window"),
                }
            } else {
                return usage_err(
                    "wait requires one of --window-count-gte, --window-title-contains, --focused-handle, --node-name-contains, --text-equals, or --text-contains",
                );
            };
            Command::Wait {
                target,
                timeout_ms,
                condition,
            }
        }
        other => return usage_err(format!("unknown command '{other}'")),
    };

    let auth = Authorization::from_cli_and_env(grant.as_deref());
    let mut executor = Executor::new(auth);
    if target == TargetRef::Ssh {
        let dest = ssh_dest.expect("ssh destination checked above");
        match SshEndpoint::from_parts(dest, ssh_port, ssh_identity, ssh_cu, ssh_env) {
            Ok(endpoint) => executor = executor.with_ssh(endpoint),
            Err(error) => {
                return agenterm_cu::CuReply {
                    ok: false,
                    target: "ssh".into(),
                    command: "usage".into(),
                    data: None,
                    error: Some(error),
                };
            }
        }
    }
    if target == TargetRef::Vnc {
        let dest = vnc_dest.expect("vnc destination checked above");
        match VncEndpoint::from_parts(dest, vnc_port, vnc_cu, vnc_env) {
            Ok(endpoint) => executor = executor.with_vnc(endpoint),
            Err(error) => {
                return agenterm_cu::CuReply {
                    ok: false,
                    target: "vnc".into(),
                    command: "usage".into(),
                    data: None,
                    error: Some(error),
                };
            }
        }
    }
    executor.execute(&command)
}

fn dispatch_json(args: &[String]) -> agenterm_cu::CuReply {
    let mut grant: Option<String> = None;
    let mut payload = None;
    let mut read_stdin = false;
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if let Some(value) = arg.strip_prefix("--grant=") {
            grant = Some(value.to_owned());
            i += 1;
        } else if arg == "--grant" {
            i += 1;
            if let Some(value) = args.get(i) {
                grant = Some(value.clone());
                i += 1;
            }
        } else if arg == "--json" {
            i += 1;
            if let Some(value) = args.get(i) {
                if value == "-" {
                    read_stdin = true;
                } else {
                    payload = Some(value.clone());
                }
                i += 1;
            } else {
                read_stdin = true;
            }
        } else if arg == "--json-stdin" || arg == "-" {
            read_stdin = true;
            i += 1;
        } else if payload.is_none() {
            if arg == "-" {
                read_stdin = true;
            } else {
                payload = Some(arg.clone());
            }
            i += 1;
        } else {
            i += 1;
        }
    }
    let raw = if read_stdin {
        let mut buf = String::new();
        if let Err(error) = std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf) {
            return usage_err(format!("could not read JSON command from stdin: {error}"));
        }
        buf
    } else {
        let Some(raw) = payload else {
            return usage_err(
                "exec requires a JSON command payload argument, --json '-', or --json-stdin",
            );
        };
        raw
    };
    let command: Command = match serde_json::from_str(&raw) {
        Ok(command) => command,
        Err(error) => return usage_err(format!("invalid JSON command: {error}")),
    };
    let auth = Authorization::from_cli_and_env(grant.as_deref());
    Executor::new(auth).execute(&command)
}

fn take_value(args: &mut Vec<String>, flag: &str) -> String {
    args.remove(0);
    if args.is_empty() {
        eprintln!("missing value for {flag}");
        return String::new();
    }
    args.remove(0)
}

fn flag_value(args: &mut Vec<String>, flag: &str) -> Option<String> {
    let index = args.iter().position(|arg| arg == flag)?;
    args.remove(index);
    args.get(index).cloned()
}

fn flag_isize(args: &mut Vec<String>, flag: &str) -> Option<isize> {
    flag_value(args, flag)?.parse().ok()
}

fn flag_i32(args: &mut Vec<String>, flag: &str) -> Option<i32> {
    flag_value(args, flag)?.parse().ok()
}

fn flag_u32(args: &mut Vec<String>, flag: &str) -> Option<u32> {
    flag_value(args, flag)?.parse().ok()
}

fn flag_u64(args: &mut Vec<String>, flag: &str) -> Option<u64> {
    flag_value(args, flag)?.parse().ok()
}

fn flag_usize(args: &mut Vec<String>, flag: &str) -> Option<usize> {
    flag_value(args, flag)?.parse().ok()
}

fn flag_coords(args: &mut Vec<String>, flag: &str) -> Option<[i32; 2]> {
    let raw = flag_value(args, flag)?;
    let mut parts = raw.split(',');
    let x = parts.next()?.trim().parse().ok()?;
    let y = parts.next()?.trim().parse().ok()?;
    Some([x, y])
}

fn usage_err(message: impl Into<String>) -> agenterm_cu::CuReply {
    eprint_usage();
    agenterm_cu::CuReply {
        ok: false,
        target: String::new(),
        command: "usage".into(),
        data: None,
        error: Some(agenterm_cu::CuError::new("usage", message)),
    }
}

fn help_reply(ok: bool) -> agenterm_cu::CuReply {
    agenterm_cu::CuReply {
        ok,
        target: String::new(),
        command: "help".into(),
        data: Some(serde_json::json!({ "usage": "see stderr" })),
        error: None,
    }
}

fn run_x11_clipboard_owner() -> i32 {
    use std::io::Read;
    let mut text = String::new();
    if std::io::stdin().read_to_string(&mut text).is_err() {
        return 1;
    }
    match agenterm_cu::mechanism::clipboard::own_text(&text) {
        Ok(()) => 0,
        Err(_) => 1,
    }
}

fn eprint_usage() {
    eprintln!(
        r#"usage: agenterm-cu --target <current|ssh|vnc> [--grant observe,actuate] <command> [args...]
       agenterm-cu --ssh <user@host> [--ssh-port N] [--ssh-identity PATH] [--ssh-cu PATH]
                   [--ssh-env KEY=VAL]... [--grant observe,actuate] <command> [args...]
       agenterm-cu --vnc <host[:port]> [--vnc-port N] [--vnc-cu PATH]
                   [--vnc-env KEY=VAL]... [--grant observe,actuate] <command> [args...]
       agenterm-cu exec [--grant observe,actuate] --json '<command-json>'
       agenterm-cu exec [--grant observe,actuate] --json -   # JSON command on stdin
       agenterm-cu host                        desktop menu and global shortcuts
       agenterm-cu hotkeys                     compatibility alias for host

Global:
  --target current|ssh|vnc  explicit target reference (required unless --ssh/--vnc)
  --ssh <user@host>         ssh target destination (implies --target ssh)
  --ssh-port N              OpenSSH -p (or AGENTERM_CU_SSH_PORT)
  --ssh-identity PATH       OpenSSH -i (or AGENTERM_CU_SSH_IDENTITY)
  --ssh-cu PATH             remote agenterm-cu path (or AGENTERM_CU_SSH_CU; default: this exe)
  --ssh-env KEY=VAL         remote env for the worker (repeatable; also AGENTERM_CU_SSH_ENV)
  --vnc <host[:port]>       vnc/RFB endpoint (implies --target vnc; or AGENTERM_CU_VNC)
  --vnc-port N              RFB TCP port when --vnc omits :port (or AGENTERM_CU_VNC_PORT; default 5900)
  --vnc-cu PATH             session worker agenterm-cu path (or AGENTERM_CU_VNC_CU; default: this exe)
  --vnc-env KEY=VAL         session env for the worker (repeatable; also AGENTERM_CU_VNC_ENV)
  --grant observe,actuate   authorization scopes (or AGENTERM_CU_GRANT)

  ssh transport runs the same verbs on a remote agenterm-cu --target current
  worker over OpenSSH stdio (no new verb). Get-selection evidence: loopback
  sshd + second agenterm-con, host send-text seed into Command, host select a
  range, host independent get-selection --name Command returns that range
  (via=get-selection; native AT-SPI GetNSelections+GetSelection; never
  screenshot / --coords / mouse-drag).

  vnc transport handshakes RFB (security type None / x11vnc -nopw), then runs
  the same verbs (observe and actuate) on a local agenterm-cu --target current
  worker against the shared session (DISPLAY/AT-SPI env; no new verb). Get-extents
  evidence: gate-owned loopback x11vnc + second agenterm-con, host get-extents
  --window H --name OffscreenField returns screen extents x/y/width/height as
  ints (via=get-extents; native AT-SPI Component.GetExtents(Screen);
  width/height >= 0; never screenshot / --coords / RFB framebuffer OCR /
  copied matched.bounds / node.bounds).

Commands:
  capabilities
  windows
  tree [--window HANDLE]
  screenshot --out PATH [--window HANDLE]
  click (--window HANDLE --node ID | --window HANDLE --name PAT [--role ROLE] | --coords X,Y --degraded)
        [--button left|right|middle] [--clicks N]
                              --name reuses wait NodeNameContains matching, then the --node AT-SPI path
  focus [--window HANDLE] (--node ID | --window HANDLE --name PAT [--role ROLE])
  send-text [--window HANDLE [--name PAT [--role ROLE]]] [--] <text...>
                              --name writes via AT-SPI EditableText (SetTextContents /
                              InsertText) or AT-SPI Text + toolkit set-value when
                              EditableText is absent (Chrome renderer AX; WebKitGTK
                              AT-SPI id + eval helper); a node with no
                              writeable text interface typed-fails (never XTest).
                              --window without --name writes that same path on
                              the showing focused node (same innermost Text
                              candidate as get-text --window). Never XTest when
                              --window is set. Without --window stays the
                              plain type-into-focused inject.
                              `--` ends flag parsing
  copy --window HANDLE [--name PAT [--role ROLE]]
                              copies AT-SPI Text.GetText onto the native
                              clipboard (Linux X11: SetSelectionOwner, not
                              xclip). addressing=accessibility-tree via=gettext.
                              --name targets the unique showing named node.
                              --window without --name copies that same path on
                              the showing focused node (same innermost Text
                              candidate as get-text --window; con Command
                              via=gettext on a second con that never steals the
                              resident control socket; Chrome GetTextField;
                              Reasonix Message Reasonix… under
                              scripts/reasonix-desktop-a11y.sh). Never XTest when
                              --window is set. A node with no Text interface
                              typed-fails (never XTest / --coords / screenshot).
                              Close the circuit with paste --window (no --text /
                              no --name) then get-text --window /
                              wait --text-equals; copy matched.text does not
                              count.
  paste --window HANDLE [--name PAT [--role ROLE]] [--text TEXT]
                              writes clipboard text via native AT-SPI EditableText
                              / Text (addressing=accessibility-tree). --text only
                              seeds the clipboard; the field write always reads
                              the clipboard. --name targets the unique showing
                              named field. --window without --name writes that
                              same path on the showing focused node (same
                              innermost Text candidate as get-text --window;
                              con Command via=editable-text on a second con
                              that never steals the resident control socket;
                              Chrome GetTextField; Reasonix Message Reasonix…).
                              Never XTest when --window is set. A node with no
                              writeable text interface typed-fails (never XTest
                              / --coords / screenshot). Close the circuit with
                              get-text --window / wait --text-equals; paste
                              matched.text does not count. `--` ends flag
                              parsing
  send-keys [--window HANDLE [--name PAT [--role ROLE]]] [--] <keys...>
                              --name delivers AT-SPI Device/key events
                              (DeviceEventListener NotifyEvent); a node with no
                              key interface typed-fails (never XTest).
                              --window without --name targets the showing
                              focused node (same innermost Text candidate as
                              get-text --window). Prefers DeviceEventListener;
                              plain typeable text falls back to the AT-SPI
                              EditableText/Text write path when that interface
                              is absent (con Command; Chrome; Reasonix).
                              Never XTest when --window is set.
                              Without --window stays the plain focused inject.
                              `--` ends flag parsing. e.g. ctrl+c / enter / k
  scroll --window HANDLE --name PAT [--role ROLE]
                              one-shot AT-SPI Component.ScrollTo(TopEdge).
                              addressing=accessibility-tree via=scroll-to.
                              Missing / false / UnknownMethod typed-fails
                              (a11y_scroll_unavailable). Never Action scroll*,
                              XTest wheel, --coords, or screenshot.
  get-extents --window HANDLE --name PAT [--role ROLE]
                              independent AT-SPI Component.GetExtents(Screen).
                              Snapshot node.bounds do not count. Empty extents
                              typed-fail (a11y_extents_unavailable).
  select --window HANDLE --name PAT --start N --end M [--role ROLE]
                              one-shot AT-SPI Text.SetSelection(0, start, end).
                              addressing=accessibility-tree via=set-selection.
                              Missing Text / UnknownMethod typed-fails
                              (a11y_selection_unavailable). SetSelection false
                              typed-fails (a11y_selection_no_effect). Never
                              XTest, mouse-drag, --coords, or screenshot. The
                              reply is not proof; observe with get-selection.
  get-selection --window HANDLE --name PAT [--role ROLE]
                              independent AT-SPI Text.GetNSelections +
                              GetSelection(0). Not the select reply payload.
                              Missing Text typed-fails
                              (a11y_selection_unavailable). n=0 is empty
                              success.
  set-caret --window HANDLE --name PAT --offset N [--role ROLE]
                              one-shot AT-SPI Text.SetCaretOffset.
                              addressing=accessibility-tree via=set-caret-offset.
                              Missing Text / UnknownMethod typed-fails
                              (a11y_caret_unavailable). SetCaretOffset false
                              typed-fails (a11y_caret_no_effect). Never
                              XTest, --coords, or screenshot. The reply is
                              not proof; observe with get-caret.
  get-caret --window HANDLE --name PAT [--role ROLE]
                              independent AT-SPI Text.CaretOffset /
                              GetCaretOffset. Not the set-caret reply payload.
                              Missing Text typed-fails
                              (a11y_caret_unavailable).
  get-text --window HANDLE [--name PAT] [--role ROLE]
                              one-shot independent AT-SPI Text.GetText on
                              the unique showing named node, or with no
                              --name on the node carrying the AT-SPI
                              focused state — the same
                              text authority wait --text-equals polls,
                              without a timeout. Not send-text / paste /
                              copy matched.text, last_text_write_via, the
                              WebKit eval helper queued-job OK, or a tree
                              snapshot text. Missing Text typed-fails
                              (a11y_text_unavailable). Never XTest /
                              --coords / screenshot.
  wait --timeout-ms MS (--window-count-gte N | --window-title-contains PAT | --focused-handle HANDLE
                        | --node-name-contains PAT [--node-role ROLE] [--window HANDLE]
                        | --text-equals TEXT --name PAT [--role ROLE] --window HANDLE
                        | --text-contains SUB --name PAT [--role ROLE] --window HANDLE)
                              --text-equals / --node-text-equals and --text-contains /
                              --node-text-contains poll AT-SPI Text.GetText on the unique
                              showing named node until that independent text equals TEXT
                              or contains SUB. send-text / paste / copy matched.text,
                              last_text_write_via, and the WebKit eval helper's queued-job
                              OK are not this condition. Timeout is typed ("timeout")
                              and reports the last GetText. Never screenshot / XTest /
                              --coords. `--` ends flag parsing.
  window-place --action <id> [--window HANDLE]
      ids: center|fullscreen|left-half|right-half|top-half|bottom-half
           upper-left|lower-left|upper-right|lower-right
           next-third|previous-third|next-display|previous-display
           larger|smaller|undo|redo
           (or SpectacleWindowAction* constants)

All replies are JSON on stdout: {{"ok":bool,"target":..,"command":..,"data":..,"error":..}}
"#
    );
}
