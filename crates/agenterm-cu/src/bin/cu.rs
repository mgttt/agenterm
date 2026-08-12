//! `cu` — agenterm-cu shell command (PRD_02_29 shell layer).
//!
//! Machine-readable JSON on stdout; usage on stderr when arguments are bad.

use agenterm_cu::{Command, CuReply, CurrentExecutor, PointerButton, WindowShowState};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let reply = dispatch(&args);
    let json = serde_json::to_string(&reply).unwrap_or_else(|_| {
        r#"{"ok":false,"command":"","error":{"code":"serialize","message":"reply serialization failed"}}"#
            .to_string()
    });
    println!("{json}");
}

fn dispatch(args: &[String]) -> CuReply {
    let executor = CurrentExecutor::new();
    let Some(verb) = args.first() else {
        eprint_usage();
        return CuReply {
            ok: false,
            command: "help".into(),
            data: None,
            error: Some(agenterm_cu::CuError::new(
                "usage",
                "no command; see usage below",
            )),
        };
    };

    let command = match verb.as_str() {
        "capabilities" => Command::Capabilities,
        "window-list" => Command::WindowList,
        "window-find" => Command::WindowFind {
            pattern: arg(args, 1),
        },
        "window-show" => Command::WindowShow {
            handle: parse_handle(args, 1),
            state: match args.get(2).map(String::as_str) {
                Some("hide") => WindowShowState::Hide,
                Some("show") => WindowShowState::Show,
                Some("minimize") => WindowShowState::Minimize,
                Some("maximize") => WindowShowState::Maximize,
                Some("restore") => WindowShowState::Restore,
                other => {
                    return usage_err(format!(
                        "window-show needs state in [hide|show|minimize|maximize|restore], got {other:?}"
                    ));
                }
            },
        },
        "window-move" => Command::WindowMove {
            handle: parse_handle(args, 1),
            x: parse_int(args, 2),
            y: parse_int(args, 3),
            width: parse_u32(args, 4, 0),
            height: parse_u32(args, 5, 0),
        },
        "window-topmost" => Command::WindowTopmost {
            handle: parse_handle(args, 1),
            topmost: match args.get(2).map(String::as_str) {
                Some("on") | Some("1") | Some("true") => true,
                Some("off") | Some("0") | Some("false") => false,
                other => return usage_err(format!("window-topmost needs on|off, got {other:?}")),
            },
        },
        "window-close" => Command::WindowClose {
            handle: parse_handle(args, 1),
        },
        "pointer-move" => Command::PointerMove {
            x: parse_int(args, 1),
            y: parse_int(args, 2),
        },
        "pointer-click" => Command::PointerClick {
            x: parse_int(args, 1),
            y: parse_int(args, 2),
            button: match args.get(3).map(String::as_str) {
                Some("right") => PointerButton::Right,
                Some("middle") => PointerButton::Middle,
                _ => PointerButton::Left,
            },
            clicks: parse_u32(args, 4, 1),
        },
        "type-text" => Command::TypeText {
            text: args[1..].join(" "),
        },
        "keys" => Command::Keys {
            shortcut: args[1..].join("+"),
        },
        "help" | "--help" | "-h" => {
            eprint_usage();
            return CuReply {
                ok: true,
                command: "help".into(),
                data: Some(serde_json::json!({ "usage": "see stderr" })),
                error: None,
            };
        }
        other => {
            return usage_err(format!("unknown command '{other}'"));
        }
    };

    executor.execute(&command)
}

fn usage_err(message: impl Into<String>) -> CuReply {
    eprint_usage();
    CuReply {
        ok: false,
        command: "usage".into(),
        data: None,
        error: Some(agenterm_cu::CuError::new("usage", message)),
    }
}

fn arg(args: &[String], index: usize) -> String {
    args.get(index).cloned().unwrap_or_default()
}

fn parse_handle(args: &[String], index: usize) -> i64 {
    arg(args, index).parse().unwrap_or_default()
}

fn parse_int(args: &[String], index: usize) -> i32 {
    arg(args, index).parse().unwrap_or_default()
}

fn parse_u32(args: &[String], index: usize, default: u32) -> u32 {
    match args.get(index) {
        Some(raw) => raw.parse().unwrap_or(default),
        None => default,
    }
}

fn eprint_usage() {
    eprintln!(
        r#"usage: cu <command> [args...]
  capabilities                          declare target capability status
  window-list                           list visible top-level windows (JSON)
  window-find <pattern>                 find window by title/app/pid: prefix
  window-show <handle> <state>          state: hide|show|minimize|maximize|restore
  window-move <handle> <x> <y> [w] [h]  move/resize window
  window-topmost <handle> <on|off>      pin window on top
  window-close <handle>                 close window
  pointer-move <x> <y>                  move the pointer
  pointer-click <x> <y> [btn] [clicks]  btn: left|right|middle (default left)
  type-text <text...>                   type into the focused control
  keys <shortcut>                       hotkey, e.g. ctrl+s / alt+f4 / enter
All replies are JSON on stdout: {{"ok":bool,"command":..,"data":..,"error":..}}
"#
    );
}
