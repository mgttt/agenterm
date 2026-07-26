use std::{env, path::PathBuf, time::SystemTime};

pub(crate) const SUPPORTED_COMMANDS: &str = "\
attach-session (attach)
active-window (active-tab)
capture-pane (capturep)
display-message (display)
dump-cells
get-settings
has-session (has)
inspect
focus
kill-server
kill-session
kill-window (killw)
list-commands (lscm)
list-panes (lsp)
list-sessions (ls)
list-windows (lsw)
new-session (new)
new-window (neww)
next-window (next)
pane-snapshot
protocol-info
previous-window (prev)
rename-session (rename)
rename-window (renamew)
screenshot
screenshot-pane (screenshot-tab)
select-window (selectw)
send-keys (send)
send-composer
send-mouse
set-setting
set-composer
set-tab-note
show-composer
show-tab-note
show-options (show)
start-server
ui-action
ui-snapshot
wait-pane (expect-pane)
wait-ui";

pub(crate) fn option_value<'a>(args: &'a [String], option: &str) -> Option<&'a str> {
    args.iter()
        .position(|argument| argument == option)
        .and_then(|position| args.get(position + 1))
        .map(String::as_str)
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
            "-s" | "-t" | "-c" | "-F" => position += 2,
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
        "BSpace" | "Backspace" => b"\x08".as_slice(),
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
        assert_eq!(tmux_key_bytes("not-a-key"), None);
    }
}
