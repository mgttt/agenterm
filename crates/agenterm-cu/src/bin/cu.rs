//! `cu` — agenterm-cu shell command (PRD_02_29 shell layer).
//!
//! Machine-readable JSON on stdout; human usage on stderr.

use agenterm_cu::{Authorization, Command, Executor, PointerButton, TargetRef, WaitCondition};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("hotkeys") {
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

    if args[0] == "exec" {
        return dispatch_json(&args[1..]);
    }

    let mut grant: Option<String> = None;
    let mut target: Option<TargetRef> = None;
    while let Some(flag) = args.first() {
        match flag.as_str() {
            "--target" => {
                let value = take_value(&mut args, "--target");
                target = TargetRef::parse(&value).or_else(|| {
                    eprint_usage();
                    None
                });
                if target.is_none() {
                    return usage_err("unknown --target value; only 'current' is supported");
                }
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

    let Some(target) = target else {
        eprint_usage();
        return usage_err("--target is required on every command");
    };

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
            if name.as_ref().is_none_or(|value| value.is_empty()) {
                return usage_err("copy requires --window <handle> --name <pattern>");
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
            if name.as_ref().is_none_or(|value| value.is_empty()) {
                return usage_err("paste requires --window <handle> --name <pattern>");
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
            // `--` ends flag parsing so --text-equals may start with a dash.
            let literal_text = match args.iter().position(|arg| arg == "--") {
                Some(index) => Some(args.split_off(index)[1..].join(" ")),
                None => None,
            };
            let timeout_ms = flag_u64(&mut args, "--timeout-ms").unwrap_or(5_000);
            let text_equals_present = args
                .iter()
                .any(|arg| arg == "--text-equals" || arg == "--node-text-equals");
            let condition = if text_equals_present {
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
                    "wait requires one of --window-count-gte, --window-title-contains, --focused-handle, --node-name-contains, or --text-equals",
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
    Executor::new(auth).execute(&command)
}

fn dispatch_json(args: &[String]) -> agenterm_cu::CuReply {
    let mut grant: Option<String> = None;
    let mut payload = None;
    for arg in args {
        if let Some(value) = arg.strip_prefix("--grant=") {
            grant = Some(value.to_owned());
        } else if arg == "--json" {
            continue;
        } else if payload.is_none() {
            payload = Some(arg.clone());
        }
    }
    let Some(raw) = payload else {
        return usage_err("exec requires a JSON command payload argument");
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
        r#"usage: cu --target <current> [--grant observe,actuate] <command> [args...]
       cu exec [--grant observe,actuate] --json '<command-json>'
       cu hotkeys                     macOS: Spectacle-default global shortcuts

Global:
  --target current          explicit target reference (required)
  --grant observe,actuate   authorization scopes (or AGENTERM_CU_GRANT)

Commands:
  capabilities
  windows
  tree [--window HANDLE]
  screenshot --out PATH [--window HANDLE]
  click (--window HANDLE --node ID | --window HANDLE --name PAT [--role ROLE] | --coords X,Y --degraded)
        [--button left|right|middle] [--clicks N]
                              --name reuses wait NodeNameContains matching, then the --node AT-SPI path
  focus [--window HANDLE] (--node ID | --window HANDLE --name PAT [--role ROLE])
  send-text [--window HANDLE --name PAT [--role ROLE]] [--] <text...>
                              --name writes via AT-SPI EditableText (SetTextContents /
                              InsertText) or AT-SPI Text + toolkit set-value when
                              EditableText is absent (Chrome renderer AX; WebKitGTK
                              AT-SPI id + eval helper); a node with no
                              writeable text interface typed-fails (never XTest).
                              `--` ends flag parsing
  copy --window HANDLE --name PAT [--role ROLE]
                              copies AT-SPI Text.GetText from the unique showing
                              named node onto the native clipboard (Linux X11:
                              SetSelectionOwner, not xclip). addressing=
                              accessibility-tree. A node with no Text interface
                              typed-fails (never XTest / --coords / screenshot).
                              Close the circuit with paste --name (no --text)
                              then wait --text-equals; copy matched.text does
                              not count
  paste --window HANDLE --name PAT [--role ROLE] [--text TEXT]
                              writes clipboard text into the unique showing named
                              field via native AT-SPI EditableText / Text
                              (addressing=accessibility-tree). --text only seeds
                              the clipboard; the field write always reads the
                              clipboard. A node with no writeable text interface
                              typed-fails (never XTest / --coords / screenshot).
                              Close the circuit with wait --text-equals; paste
                              matched.text does not count. `--` ends flag parsing
  send-keys [--window HANDLE --name PAT [--role ROLE]] [--] <keys...>
                              --name delivers AT-SPI Device/key events
                              (DeviceEventListener NotifyEvent); a node with no
                              key interface typed-fails (never XTest). `--`
                              ends flag parsing. e.g. ctrl+c / enter / k
  wait --timeout-ms MS (--window-count-gte N | --window-title-contains PAT | --focused-handle HANDLE
                        | --node-name-contains PAT [--node-role ROLE] [--window HANDLE]
                        | --text-equals TEXT --name PAT [--role ROLE] --window HANDLE)
                              --text-equals / --node-text-equals polls AT-SPI Text.GetText
                              on the unique showing named node until that independent
                              text equals TEXT. send-text / paste / copy matched.text, last_text_write_via,
                              and the WebKit eval helper's queued-job OK are not this
                              condition. Timeout is typed ("timeout"). Never
                              screenshot / XTest / --coords. `--` ends flag parsing.
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
