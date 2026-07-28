use crate::{
    SCROLLBACK_LINES,
    commands::{
        has_option, last_positional, option_value, parse_new_command, parse_tab_environment,
        positional_values, supported_commands, tmux_key_bytes,
    },
    protocol::IpcResponse,
    terminal_runtime::TerminalTab,
};

pub(crate) const CAPTURE_PUBLIC_MAX_BYTES: usize = 1024 * 1024;

pub(crate) fn command_name(args: &[String]) -> Option<&str> {
    args.first().map(String::as_str)
}

pub(crate) fn bounded_utf8_prefix(value: &str, maximum: usize) -> &str {
    let mut take = maximum.min(value.len());
    while take > 0 && !value.is_char_boundary(take) {
        take -= 1;
    }
    &value[..take]
}

pub(crate) fn render_format(
    format: &str,
    tab: &TerminalTab,
    session_name: &str,
    active: bool,
) -> String {
    let dead = tab.exited.is_some();
    format
        .replace("#{?pane_dead,dead,}", if dead { "dead" } else { "" })
        .replace("#{?window_active,*,}", if active { "*" } else { "" })
        .replace("#{session_name}", session_name)
        .replace("#{window_index}", &tab.index.to_string())
        .replace("#{window_id}", &format!("@{}", tab.id))
        .replace(
            "#{tab_parent_id}",
            &tab
                .parent_id
                .map(|id| format!("@{id}"))
                .unwrap_or_else(|| "root".to_owned()),
        )
        .replace("#{window_name}", &tab.title)
        .replace("#{window_note}", &tab.note)
        .replace("#{terminal_title}", &tab.parser.callbacks().title)
        .replace("#{window_active}", if active { "1" } else { "0" })
        .replace("#{pane_index}", "0")
        .replace("#{pane_id}", &format!("%{}", tab.id))
        .replace("#{pane_dead}", if dead { "1" } else { "0" })
        .replace(
            "#{pane_pid}",
            &tab.process_id
                .map(|pid| pid.to_string())
                .unwrap_or_default(),
        )
        .replace("#{pane_current_command}", &tab.command_name)
        .replace("#{pane_start_command}", &tab.command_name)
        .replace("#{pane_input_bytes}", &tab.input_bytes.to_string())
        .replace("#{pane_output_bytes}", &tab.output_bytes.to_string())
        .replace("#{pane_error}", tab.error.as_deref().unwrap_or(""))
        .replace("#{pane_width}", &tab.last_size.1.to_string())
        .replace("#{pane_height}", &tab.last_size.0.to_string())
        .replace("#{history_limit}", &SCROLLBACK_LINES.to_string())
        .replace("#I", &tab.index.to_string())
        .replace("#W", &tab.title)
        .replace("#S", session_name)
        .replace("#P", "0")
}

pub(crate) fn resolve_target_position(
    tabs: &[TerminalTab],
    active: Option<u64>,
    target: Option<&str>,
) -> Option<usize> {
    let Some(target) = target else {
        let active = active?;
        return tabs.iter().position(|tab| tab.id == active);
    };
    let target = target
        .rsplit(':')
        .next()
        .unwrap_or(target)
        .trim_start_matches(['=', '%']);
    if let Some(id) = target
        .strip_prefix('@')
        .and_then(|value| value.parse::<u64>().ok())
    {
        return tabs.iter().position(|tab| tab.id == id);
    }
    if let Ok(index) = target.parse::<u32>() {
        return tabs.iter().position(|tab| tab.index == index);
    }
    tabs.iter().position(|tab| tab.title == target)
}

fn tab_depth(tabs: &[TerminalTab], id: u64) -> usize {
    let Some(tab) = tabs.iter().find(|tab| tab.id == id) else {
        return 0;
    };
    let mut depth = 0;
    let mut current_parent = tab.parent_id;
    while let Some(parent_id) = current_parent {
        depth += 1;
        current_parent = tabs
            .iter()
            .find(|tab| tab.id == parent_id)
            .and_then(|tab| tab.parent_id);
        if depth > tabs.len() {
            break;
        }
    }
    depth
}

pub(crate) trait ControlHost {
    fn session_name(&self) -> &str;
    fn started_at_unix_secs(&self) -> u64;
    fn tabs(&self) -> &[TerminalTab];
    fn tabs_mut(&mut self) -> &mut Vec<TerminalTab>;
    fn active_id(&self) -> Option<u64>;
    fn set_active_id(&mut self, id: Option<u64>);
    fn request_shutdown(&mut self);

    /// Win: finish note edit / cancel selection; Unix: no-op default.
    fn before_destructive_ui(&mut self) {}

    /// Win: save composer from HWND; Unix: no-op default.
    fn sync_composer_from_ui(&mut self) {}

    fn set_session_name(&mut self, name: String);

    /// Create a tab. Returns the stable window index used in -F output.
    fn create_tab(
        &mut self,
        title: Option<String>,
        command_line: Vec<String>,
        tab_environment: Vec<(String, String)>,
        select: bool,
        parent_id: Option<u64>,
    ) -> Result<u32, String>;

    /// Select tab at position (after resolve). Host does UI sync.
    fn select_tab_at(&mut self, position: usize) -> Result<(), String>;

    /// Close tab by id. Ok(true)=workers finished; Ok(false)=incomplete shutdown.
    fn close_tab_id(&mut self, id: u64) -> Result<bool, String>;

    /// Adjacent tab position for select-window -n/-p. Default: None.
    fn adjacent_tab_position(&self, direction: i32) -> Option<usize> {
        let tabs = self.tabs();
        if tabs.is_empty() {
            return None;
        }
        let current = resolve_target_position(tabs, self.active_id(), None).unwrap_or(0) as i32;
        Some((current + direction).rem_euclid(tabs.len() as i32) as usize)
    }

    fn resolve_parent_id(&self, target: &str) -> Result<Option<u64>, String> {
        if matches!(target, "root" | "none" | "-") {
            return Ok(None);
        }
        let Some(position) =
            resolve_target_position(self.tabs(), self.active_id(), Some(target))
        else {
            return Err(format!("can't find parent tab: {target}"));
        };
        Ok(Some(self.tabs()[position].id))
    }
}

pub(crate) fn dispatch_shared_command(
    host: &mut dyn ControlHost,
    args: &[String],
) -> Option<IpcResponse> {
    let command = command_name(args)?;

    match command {
        "protocol-info" => {
            Some(IpcResponse::success(crate::client::protocol_info_json(
                "running_host",
            )))
        }
        "lscm" | "list-commands" => Some(IpcResponse::success(supported_commands())),
        "ls" | "list-sessions" => Some(IpcResponse::success(format!(
            "{}: {} windows (created {}) (attached)",
            host.session_name(),
            host.tabs().len(),
            host.started_at_unix_secs()
        ))),
        "has" | "has-session" => {
            let requested = option_value(args, "-t");
            if requested.is_none_or(|name| name == host.session_name()) {
                Some(IpcResponse::success(""))
            } else {
                Some(IpcResponse::failure(format!(
                    "can't find session: {}",
                    requested.unwrap_or_default()
                )))
            }
        }
        "lsw" | "list-windows" => {
            let format = option_value(args, "-F").unwrap_or("#I:#W#{?window_active,*,}");
            let session_name = host.session_name().to_owned();
            let active = host.active_id();
            Some(IpcResponse::success(
                host.tabs()
                    .iter()
                    .map(|tab| {
                        render_format(
                            format,
                            tab,
                            &session_name,
                            active == Some(tab.id),
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
            ))
        }
        "lsp" | "list-panes" => {
            let format = option_value(args, "-F").unwrap_or(
                "#{pane_id}: [#{pane_width}x#{pane_height}] #{pane_current_command}",
            );
            let all = args.iter().any(|arg| arg == "-a");
            let tabs: Vec<&TerminalTab> = if all {
                host.tabs().iter().collect()
            } else {
                resolve_target_position(host.tabs(), host.active_id(), option_value(args, "-t"))
                    .and_then(|position| host.tabs().get(position))
                    .into_iter()
                    .collect()
            };
            let session_name = host.session_name().to_owned();
            let active = host.active_id();
            Some(IpcResponse::success(
                tabs
                    .into_iter()
                    .map(|tab| {
                        render_format(
                            format,
                            tab,
                            &session_name,
                            active == Some(tab.id),
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
            ))
        }
        "send" | "send-keys" => {
            let Some(position) =
                resolve_target_position(host.tabs(), host.active_id(), option_value(args, "-t"))
            else {
                return Some(IpcResponse::typed_failure(
                    "can't find pane",
                    "operation_target_not_found",
                    "not_found",
                    false,
                ));
            };
            if host.tabs()[position].submission.is_pending() {
                return Some(IpcResponse::typed_failure(
                    "composer submission is pending; wait with \
                     `wait-pane --submit-complete` before sending keys",
                    "operation_conflict",
                    "conflict",
                    true,
                ));
            }
            let literal = args.iter().any(|arg| arg == "-l");
            for key in positional_values(args, &["-t"], &["-l", "-R", "-X"]) {
                let sent = if literal {
                    host.tabs_mut()[position].send(key.as_bytes())
                } else if let Some(bytes) = tmux_key_bytes(key) {
                    host.tabs_mut()[position].send(&bytes)
                } else {
                    host.tabs_mut()[position].send(key.as_bytes())
                };
                if !sent {
                    return Some(IpcResponse::typed_failure(
                        "terminal input was not accepted because the pane is no longer writable",
                        "terminal_not_writable",
                        "precondition",
                        false,
                    ));
                }
            }
            Some(IpcResponse::success(""))
        }
        "capturep" | "capture-pane" => {
            let Some(position) =
                resolve_target_position(host.tabs(), host.active_id(), option_value(args, "-t"))
            else {
                return Some(IpcResponse::typed_failure(
                    "can't find pane",
                    "operation_target_not_found",
                    "not_found",
                    false,
                ));
            };
            let requested_maximum = match option_value(args, "--max-bytes") {
                Some(value) => match value.parse::<usize>() {
                    Ok(value) if (1..=CAPTURE_PUBLIC_MAX_BYTES).contains(&value) => Some(value),
                    _ => {
                        return Some(IpcResponse::failure(format!(
                            "capture-pane --max-bytes must be from 1 to {CAPTURE_PUBLIC_MAX_BYTES}"
                        )));
                    }
                },
                None => None,
            };
            let json = args.iter().any(|argument| argument == "--json");
            let tab = &host.tabs()[position];
            let contents = if args.iter().any(|argument| argument == "--raw-escaped") {
                String::from_utf8_lossy(&tab.raw_output.to_vec())
                    .escape_debug()
                    .to_string()
            } else {
                tab.parser.screen().contents()
            };
            let original_bytes = contents.len();
            let maximum = requested_maximum.or(json.then_some(CAPTURE_PUBLIC_MAX_BYTES));
            let text = maximum.map_or(contents.as_str(), |maximum| {
                bounded_utf8_prefix(&contents, maximum)
            });
            if json {
                Some(IpcResponse::success(
                    serde_json::to_string_pretty(&serde_json::json!({
                        "tab_id": format!("@{}", tab.id),
                        "text": text,
                        "bytes": text.len(),
                        "original_bytes": original_bytes,
                        "max_bytes": maximum,
                        "truncated": text.len() < original_bytes,
                    }))
                    .unwrap_or_else(|_| "{}".to_owned()),
                ))
            } else {
                Some(IpcResponse::success(text.to_owned()))
            }
        }
        "inspect" | "pane-snapshot" => {
            host.sync_composer_from_ui();
            let selected: Vec<&TerminalTab> = match option_value(args, "-t") {
                Some(target) => {
                    let Some(position) =
                        resolve_target_position(host.tabs(), host.active_id(), Some(target))
                    else {
                        return Some(IpcResponse::failure("can't find window"));
                    };
                    vec![&host.tabs()[position]]
                }
                None => host.tabs().iter().collect(),
            };
            let tabs = host.tabs();
            let active = host.active_id();
            let session_name = host.session_name();
            let windows = selected
                .into_iter()
                .map(|tab| {
                    let observation = tab.observation();
                    serde_json::json!({
                        "id": format!("@{}", tab.id),
                        "index": tab.index,
                        "parent_id": tab.parent_id.map(|id| format!("@{id}")),
                        "depth": tab_depth(tabs, tab.id),
                        "name": tab.title,
                        "terminal_title": tab.parser.callbacks().title,
                        "note": tab.note,
                        "active": active == Some(tab.id),
                        "dead": observation.exit_code.is_some(),
                        "exit_code": observation.exit_code,
                        "pid": observation.process_id,
                        "command": tab.command_name,
                        "environment_names": tab.environment_names,
                        "rows": tab.last_size.0,
                        "cols": tab.last_size.1,
                        "input_bytes": observation.input_bytes,
                        "input_writes": observation.input_writes,
                        "submit_pending": observation.submit_pending,
                        "reader_closed": observation.reader_closed,
                        "parser_drained": observation.parser_drained,
                        "finalized": observation.finalized,
                        "scrollback_offset": tab.parser.screen().scrollback(),
                        "output_bytes": observation.output_bytes,
                        "composer": if tab.sensitive_composer.is_some() {
                            "<redacted>"
                        } else {
                            tab.composer.as_str()
                        },
                        "composer_sensitive": tab.sensitive_composer.is_some(),
                        "error": observation.error,
                        "text": tab.parser.screen().contents(),
                    })
                })
                .collect::<Vec<_>>();
            match serde_json::to_string_pretty(&serde_json::json!({
                "session": session_name,
                "active_window_id": active.map(|id| format!("@{id}")),
                "windows": windows,
            })) {
                Ok(json) => Some(IpcResponse::success(json)),
                Err(error) => Some(IpcResponse::failure(error.to_string())),
            }
        }
        "neww" | "new-window" => {
            let (title, detached, child_command) = parse_new_command(args);
            let tab_environment = match parse_tab_environment(args) {
                Ok(environment) => environment,
                Err(error) => return Some(IpcResponse::failure(error)),
            };
            let parent_id = match option_value(args, "--parent") {
                Some(target) => match host.resolve_parent_id(target) {
                    Ok(parent_id) => parent_id,
                    Err(error) => return Some(IpcResponse::failure(error)),
                },
                None => None,
            };
            match host.create_tab(
                title,
                child_command,
                tab_environment,
                !detached,
                parent_id,
            ) {
                Ok(index) => {
                    let format = option_value(args, "-F").unwrap_or("#{window_index}");
                    let tab = host
                        .tabs()
                        .iter()
                        .find(|tab| tab.index == index)
                        .expect("newly created tab must remain present");
                    Some(IpcResponse::success(render_format(
                        format,
                        tab,
                        host.session_name(),
                        host.active_id() == Some(tab.id),
                    )))
                }
                Err(error) => Some(IpcResponse::failure(error)),
            }
        }
        "selectw" | "select-window" => {
            let position = if has_option(args, "-n") {
                host.adjacent_tab_position(1)
            } else if has_option(args, "-p") {
                host.adjacent_tab_position(-1)
            } else {
                resolve_target_position(host.tabs(), host.active_id(), option_value(args, "-t"))
            };
            let Some(position) = position else {
                return Some(IpcResponse::failure("can't find window"));
            };
            match host.select_tab_at(position) {
                Ok(()) => Some(IpcResponse::success("")),
                Err(error) => Some(IpcResponse::failure(error)),
            }
        }
        "killw" | "kill-window" => {
            let Some(position) =
                resolve_target_position(host.tabs(), host.active_id(), option_value(args, "-t"))
            else {
                return Some(IpcResponse::failure("can't find window"));
            };
            let id = host.tabs()[position].id;
            match host.close_tab_id(id) {
                Ok(true) => Some(IpcResponse::success("")),
                Ok(false) => Some(IpcResponse::typed_failure(
                    "terminal was removed, but its workers did not finish bounded shutdown",
                    "terminal_shutdown_incomplete",
                    "internal",
                    false,
                )),
                Err(error) => Some(IpcResponse::failure(error)),
            }
        }
        "active-window" | "active-tab" => {
            let Some(position) =
                resolve_target_position(host.tabs(), host.active_id(), None)
            else {
                return Some(IpcResponse::failure("no active window"));
            };
            let format = option_value(args, "-F").unwrap_or("#{window_id}:#{window_name}");
            let tab = &host.tabs()[position];
            Some(IpcResponse::success(render_format(
                format,
                tab,
                host.session_name(),
                true,
            )))
        }
        "display" | "display-message" => {
            let Some(position) =
                resolve_target_position(host.tabs(), host.active_id(), option_value(args, "-t"))
            else {
                return Some(IpcResponse::failure("can't find pane"));
            };
            let format = last_positional(args, &["-t"])
                .unwrap_or("#{session_name}:#{window_index}.#{pane_index}");
            let tab = &host.tabs()[position];
            Some(IpcResponse::success(render_format(
                format,
                tab,
                host.session_name(),
                host.active_id() == Some(tab.id),
            )))
        }
        "rename" | "rename-session" => {
            let Some(name) = last_positional(args, &["-t"]) else {
                return Some(IpcResponse::failure("usage: rename-session new-name"));
            };
            host.set_session_name(name.to_owned());
            Some(IpcResponse::success(""))
        }
        "kill-session" | "kill-server" => {
            if let Some(requested) = option_value(args, "-t")
                && requested != host.session_name()
            {
                return Some(IpcResponse::failure(if command == "kill-session" {
                    format!("can't find session: {requested}")
                } else {
                    format!(
                        "operation_target_not_found[server.kill]: \
                         can't find session: {requested}"
                    )
                }));
            }
            host.before_destructive_ui();
            let mut terminal_shutdown_complete = true;
            for tab in host.tabs_mut() {
                terminal_shutdown_complete &= tab.close_process();
            }
            host.tabs_mut().clear();
            host.set_active_id(None);
            // Event journal persistence is host-specific; Unix has none in this slice.
            host.request_shutdown();
            if terminal_shutdown_complete {
                Some(IpcResponse::success(""))
            } else {
                Some(IpcResponse::typed_failure(
                    "server shutdown began, but a terminal worker missed its bounded deadline",
                    "terminal_shutdown_incomplete",
                    "internal",
                    false,
                ))
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_utf8_prefix_respects_char_boundaries() {
        let text = "ab终";
        assert_eq!(bounded_utf8_prefix(text, text.len()), text);
        assert_eq!(bounded_utf8_prefix(text, 4), "ab");
        assert_eq!(bounded_utf8_prefix(text, 5), "ab终");
        assert_eq!(bounded_utf8_prefix(text, 0), "");
    }

    #[test]
    fn resolve_target_position_empty_tabs() {
        let tabs: Vec<TerminalTab> = Vec::new();
        assert_eq!(resolve_target_position(&tabs, None, None), None);
        assert_eq!(resolve_target_position(&tabs, Some(1), None), None);
        assert_eq!(resolve_target_position(&tabs, None, Some("@1")), None);
        assert_eq!(resolve_target_position(&tabs, None, Some("title")), None);
    }
}
