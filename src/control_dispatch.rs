use std::{collections::BTreeSet, mem};

use crate::{
    SCROLLBACK_LINES,
    commands::{
        has_option, last_positional, option_value, parse_new_command, parse_tab_environment,
        positional_values, supported_commands, tmux_key_bytes,
    },
    event_journal::{EventEnvelope, EventJournal, EventKind, EventPosition},
    operations::{UI_TABS_HIDE, UI_TABS_SET_WIDTH, UI_TABS_SHOW, UI_TABS_TOGGLE},
    protocol::IpcResponse,
    settings::clamp_tabs_width,
    tab_tree::{TabTreeNode, TabTreeRow, tree_rows, would_create_cycle},
    terminal_runtime::TerminalTab,
    theme::ThemeId,
    ui_bridge::{
        UI_BOOTSTRAP_SCHEMA_VERSION, UI_BRIDGE_PROTOCOL_VERSION, UI_DELTA_MAX_EVENTS,
        UI_DELTA_SCHEMA_VERSION, UI_HELLO_SCHEMA_VERSION, UI_SCREEN_MAX_COLUMNS,
        UI_SCREEN_MAX_ROWS, UI_SCREEN_MAX_RUNS, UI_SCREEN_MAX_TEXT_BYTES, UI_SCREEN_SCHEMA_VERSION,
        UiBootstrapSnapshot, UiCellRun, UiCellStyle, UiColor, UiCompatibility, UiComposerSnapshot,
        UiCursorSnapshot, UiDeltaBatch, UiDeltaEvent, UiEventPosition, UiHelloRequest,
        UiHelloResponse, UiProtocolRange, UiScreenSnapshot, UiTabBootstrap,
        UiWorkingContextSnapshot, negotiate,
    },
    workspace::workspace_path,
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

fn ui_color(color: vt100::Color) -> UiColor {
    match color {
        vt100::Color::Default => UiColor::Default,
        vt100::Color::Idx(index) => UiColor::Indexed { index },
        vt100::Color::Rgb(red, green, blue) => UiColor::Rgb { red, green, blue },
    }
}

fn ui_cell_style(cell: &vt100::Cell) -> UiCellStyle {
    UiCellStyle {
        foreground: ui_color(cell.fgcolor()),
        background: ui_color(cell.bgcolor()),
        bold: cell.bold(),
        italic: cell.italic(),
        underline: cell.underline(),
        inverse: cell.inverse(),
    }
}

fn ui_screen_snapshot(tab: &mut TerminalTab, generation: u64) -> Result<UiScreenSnapshot, String> {
    let (_, max_scrollback) = tab.scrollback_bounds();
    let screen = tab.parser.screen();
    let (rows, columns) = screen.size();
    let rows = u32::from(rows);
    let columns = u32::from(columns);
    if rows == 0 || columns == 0 || rows > UI_SCREEN_MAX_ROWS || columns > UI_SCREEN_MAX_COLUMNS {
        return Err("ui_screen_dimensions_limit".to_owned());
    }
    let mut runs: Vec<UiCellRun> = Vec::new();
    let mut text_bytes = 0usize;
    let mut truncated = false;
    'rows: for row in 0..rows {
        for column in 0..columns {
            let Some(cell) = screen.cell(row as u16, column as u16) else {
                continue;
            };
            if cell.is_wide_continuation() {
                continue;
            }
            let cell_columns = if cell.is_wide() { 2 } else { 1 };
            let text = if cell.has_contents() {
                cell.contents()
            } else {
                " "
            };
            if runs.len() >= UI_SCREEN_MAX_RUNS
                || text_bytes
                    .checked_add(text.len())
                    .is_none_or(|total| total > UI_SCREEN_MAX_TEXT_BYTES)
            {
                truncated = true;
                break 'rows;
            }
            let style = ui_cell_style(cell);
            if let Some(previous) = runs.last_mut()
                && previous.row == row
                && previous.column + previous.columns == column
                && previous.style == style
            {
                previous.columns += cell_columns;
                previous.text.push_str(text);
            } else {
                runs.push(UiCellRun {
                    row,
                    column,
                    columns: cell_columns,
                    text: text.to_owned(),
                    style,
                });
            }
            text_bytes += text.len();
        }
    }
    let (cursor_row, cursor_column) = screen.cursor_position();
    let snapshot = UiScreenSnapshot {
        schema_version: UI_SCREEN_SCHEMA_VERSION,
        tab_id: format!("@{}", tab.id),
        generation,
        terminal_title: tab.parser.callbacks().title.clone(),
        rows,
        columns,
        alternate_screen: screen.alternate_screen(),
        application_cursor: screen.application_cursor(),
        scrollback_offset: screen.scrollback(),
        max_scrollback,
        cursor: UiCursorSnapshot {
            row: u32::from(cursor_row),
            column: u32::from(cursor_column),
            visible: !screen.hide_cursor(),
        },
        runs,
        complete: !truncated,
        truncated,
    };
    snapshot.validate()?;
    Ok(snapshot)
}

fn ui_tab_bootstrap(
    tab: &mut TerminalTab,
    generation: u64,
    collapsed: bool,
) -> Result<UiTabBootstrap, String> {
    let proxy = tab.proxy.facts();
    let composer = match tab.sensitive_composer.as_ref() {
        Some(secret) => UiComposerSnapshot {
            text: None,
            sensitive: true,
            byte_length: secret.expose_bytes().len(),
        },
        None => UiComposerSnapshot {
            text: Some(tab.composer.clone()),
            sensitive: false,
            byte_length: tab.composer.len(),
        },
    };
    Ok(UiTabBootstrap {
        id: format!("@{}", tab.id),
        index: tab.index,
        parent_id: tab.parent_id.map(|parent| format!("@{parent}")),
        collapsed,
        title: tab.title.clone(),
        note: tab.note.clone(),
        process_id: tab.process_id,
        dead: tab.exited.is_some(),
        exit_code: tab.exited,
        composer,
        working_context: UiWorkingContextSnapshot {
            cwd: tab.cwd.path().map(str::to_owned),
            cwd_confirmed_path: tab.cwd.confirmed_path().map(str::to_owned),
            cwd_confirmed: tab.cwd.path() == tab.cwd.confirmed_path(),
            cwd_source: tab.cwd.source().as_str().to_owned(),
            cwd_request_pending: tab.cwd.pending(),
            shell: tab.shell_kind.as_str().to_owned(),
            proxy_configured: proxy.configured,
            proxy_source: proxy.source.as_str().to_owned(),
            proxy_application_state: proxy.application_state.as_str().to_owned(),
            proxy_request_pending: proxy.request_pending,
        },
        screen: ui_screen_snapshot(tab, generation)?,
    })
}

pub(crate) fn ui_bootstrap_snapshot(
    host: &mut dyn ControlHost,
) -> Result<UiBootstrapSnapshot, String> {
    let position = host.event_journal().position();
    let collapsed_ids = host
        .collapsed_tab_ids()
        .into_iter()
        .collect::<BTreeSet<_>>();
    let tabs = host
        .tabs_mut()
        .iter_mut()
        .map(|tab| {
            let collapsed = collapsed_ids.contains(&tab.id);
            ui_tab_bootstrap(tab, position.sequence, collapsed)
        })
        .collect::<Result<Vec<_>, String>>()?;
    let truncated = tabs.iter().any(|tab| tab.screen.truncated);
    let snapshot = UiBootstrapSnapshot {
        schema_version: UI_BOOTSTRAP_SCHEMA_VERSION,
        server_pid: std::process::id(),
        server_epoch: position.epoch.clone(),
        position: UiEventPosition {
            server_epoch: position.epoch,
            sequence: position.sequence,
        },
        workspace_revision: None,
        active_tab_id: host.active_id().map(|id| format!("@{id}")),
        tabs,
        complete: !truncated,
        truncated,
    };
    snapshot.validate()?;
    Ok(snapshot)
}

fn ui_hello_response(
    host: &dyn ControlHost,
    request: UiHelloRequest,
) -> Result<UiHelloResponse, String> {
    request.validate()?;
    let client_build = request.client_build.clone();
    let compatibility = negotiate(request.protocol_range, UI_BRIDGE_PROTOCOL_VERSION);
    let position = host.event_journal().position();
    let mut capabilities = vec![
        "bootstrap_snapshot".to_owned(),
        "ordered_delta_poll".to_owned(),
        "full_screen_post_state".to_owned(),
        "epoch_restart_detection".to_owned(),
    ];
    let facts = host.ui_bridge_facts();
    if facts.interactive_lease {
        capabilities.push("interactive_lease".to_owned());
        capabilities.push("lease_gated_interaction".to_owned());
    }
    if facts.replaceable_ui {
        capabilities.push("replaceable_ui_client".to_owned());
        capabilities.push("lease_owned_client_state".to_owned());
        capabilities.push("lease_owned_client_commands".to_owned());
    }
    if facts.reconnect {
        capabilities.push("in_place_reconnect".to_owned());
    }
    let response = UiHelloResponse {
        schema_version: UI_HELLO_SCHEMA_VERSION,
        accepted: compatibility == UiCompatibility::Compatible,
        compatibility,
        client_id: request.client_id,
        protocol_version: UI_BRIDGE_PROTOCOL_VERSION,
        server_pid: std::process::id(),
        position: UiEventPosition {
            server_epoch: position.epoch,
            sequence: position.sequence,
        },
        bootstrap_schema_version: UI_BOOTSTRAP_SCHEMA_VERSION,
        delta_schema_version: UI_DELTA_SCHEMA_VERSION,
        capabilities,
        client_build,
        server_build: Some(crate::upgrade_identity::UpgradeIdentity::current(
            UI_BRIDGE_PROTOCOL_VERSION,
        )),
    };
    response.validate()?;
    Ok(response)
}

fn ui_delta_event(event: &EventEnvelope) -> UiDeltaEvent {
    UiDeltaEvent {
        sequence: event.sequence,
        kind: event.kind.clone(),
        tab_id: event.tab_id.map(|id| format!("@{id}")),
        request_id: event.request_id.clone(),
        operation_id: event.operation_id.clone(),
        payload: event.payload.clone(),
    }
}

fn ui_delta_batch(
    host: &mut dyn ControlHost,
    after_sequence: u64,
    position: &EventPosition,
    events: &[EventEnvelope],
) -> Result<UiDeltaBatch, String> {
    let mut event_count = events.len();
    loop {
        let selected = &events[..event_count];
        let affected_ids = selected
            .iter()
            .filter_map(|event| event.tab_id)
            .collect::<BTreeSet<_>>();
        let live_ids = host
            .tabs()
            .iter()
            .map(|tab| tab.id)
            .collect::<BTreeSet<_>>();
        let collapsed_ids = host
            .collapsed_tab_ids()
            .into_iter()
            .collect::<BTreeSet<_>>();
        let tab_updates = host
            .tabs_mut()
            .iter_mut()
            .filter(|tab| affected_ids.contains(&tab.id))
            .map(|tab| {
                let collapsed = collapsed_ids.contains(&tab.id);
                ui_tab_bootstrap(tab, position.sequence, collapsed)
            })
            .collect::<Result<Vec<_>, String>>()?;
        let closed_tab_ids = affected_ids
            .difference(&live_ids)
            .map(|id| format!("@{id}"))
            .collect::<Vec<_>>();
        let through_sequence = selected
            .last()
            .map_or(after_sequence, |event| event.sequence);
        let complete = through_sequence == position.sequence;
        let batch = UiDeltaBatch {
            schema_version: UI_DELTA_SCHEMA_VERSION,
            server_epoch: position.epoch.clone(),
            after_sequence,
            through_sequence,
            current_sequence: position.sequence,
            events: selected.iter().map(ui_delta_event).collect(),
            tab_updates,
            closed_tab_ids,
            active_tab_id: host.active_id().map(|id| format!("@{id}")),
            complete,
            truncated: !complete,
        };
        match batch.validate() {
            Ok(()) => return Ok(batch),
            Err(error) if error == "ui_delta_bytes_limit" && event_count > 1 => {
                event_count -= 1;
            }
            Err(error) => return Err(error),
        }
    }
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
            &tab.parent_id
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

    fn ui_bridge_facts(&self) -> crate::ui_bridge::UiBridgeFacts {
        crate::ui_bridge::current_facts()
    }

    /// Win: finish note edit / cancel selection; Unix: no-op default.
    fn before_destructive_ui(&mut self) {}

    /// Win: save composer from HWND; Unix: sync in-memory draft.
    fn sync_composer_from_ui(&mut self) {}

    /// Reload the active tab composer into the host UI surface.
    fn load_composer_to_ui(&mut self) {}

    /// Preserve host-owned modal/focus traps before a shared UI action runs.
    fn admit_ui_action(&mut self, _action: &str) -> Result<(), String> {
        Ok(())
    }

    #[allow(dead_code)]
    fn focus_surface(&self) -> &str {
        "terminal"
    }

    fn set_ipc_focus_surface(&mut self, surface: &str) -> Result<(), String> {
        match surface {
            "terminal" | "composer" | "tabs" | "sidebar" => Ok(()),
            other => Err(format!("unknown focus surface: {other}")),
        }
    }

    fn settings_json(&self) -> String {
        "{}".to_owned()
    }

    /// Win: note-editor `set-composer` override; default writes tab composer draft.
    fn apply_set_composer(&mut self, position: usize, text: String) -> Result<(), String> {
        let id = self.tabs()[position].id;
        self.tabs_mut()[position].composer = text.clone();
        self.event_journal_mut().commit(
            EventKind::ComposerDraft,
            Some(id),
            serde_json::json!({
                "length": text.chars().count(),
            }),
        );
        if self.active_id() == Some(id) {
            self.load_composer_to_ui();
        }
        Ok(())
    }

    fn apply_setting(&mut self, _key: &str, _value: &str) -> Result<(), String> {
        Err("set-setting is not supported on this host".to_owned())
    }

    /// Win: may queue close confirmation; default closes immediately.
    fn close_tab_by_ui_action(&mut self, id: u64) -> Result<(), String> {
        match self.close_tab_id(id) {
            Ok(_) => Ok(()),
            Err(error) => Err(error),
        }
    }

    /// Win: trap editors / note submit before `composer-send`.
    fn prepare_composer_send(&mut self) -> Result<bool, String> {
        Ok(false)
    }

    /// Win: open tab editor after `new-child`.
    fn after_create_tab(&mut self, _id: u64, _parent_id: Option<u64>) {}

    fn set_tabs_visible(
        &mut self,
        _visible: bool,
        _cause: &str,
        _operation_id: &str,
    ) -> Result<(), String> {
        Err("tabs visibility is not supported on this host".to_owned())
    }

    fn set_tabs_width(
        &mut self,
        _width: u16,
        _cause: &str,
        _operation_id: &str,
    ) -> Result<(), String> {
        Err("tabs width is not supported on this host".to_owned())
    }

    fn collapsed_tab_ids(&self) -> Vec<u64> {
        Vec::new()
    }

    fn toggle_tab_collapsed(&mut self, _tab_id: u64) -> Result<(), String> {
        Err("tab tree collapse is not supported on this host".to_owned())
    }

    fn prepare_cwd(&mut self, _tab_id: u64, _path: &str, _mode: &str) -> Result<(), String> {
        Err("CWD preparation is not supported on this host".to_owned())
    }

    fn send_cwd_now(&mut self, _tab_id: u64, _path: &str) -> Result<(), String> {
        Err("CWD submission is not supported on this host".to_owned())
    }

    fn open_settings_modal(&mut self) -> Result<(), String> {
        Err("settings UI is not supported on this host".to_owned())
    }

    fn close_settings_modal(&mut self, _apply: bool) -> Result<(), String> {
        Err("settings UI is not supported on this host".to_owned())
    }

    #[allow(dead_code)]
    fn preview_settings_theme(&mut self, _theme: ThemeId) {}

    fn open_tab_editor(&mut self, _tab_id: u64) -> Result<(), String> {
        Err("tab editor is not supported on this host".to_owned())
    }

    fn finish_tab_editor(&mut self, _save: bool) -> Result<(), String> {
        Err("tab editor is not supported on this host".to_owned())
    }

    fn ui_action_cancel(&mut self) -> Result<bool, String> {
        Ok(false)
    }

    fn ui_action_confirm(&mut self) -> Result<bool, String> {
        Ok(false)
    }

    fn copy_selection(&mut self) -> Result<(), String> {
        Err("copy-selection is not supported on this host".to_owned())
    }

    fn config_tabs_visible(&self) -> bool {
        true
    }

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
        let Some(position) = resolve_target_position(self.tabs(), self.active_id(), Some(target))
        else {
            return Err(format!("can't find parent tab: {target}"));
        };
        Ok(Some(self.tabs()[position].id))
    }

    fn event_journal(&self) -> &EventJournal;
    fn event_journal_mut(&mut self) -> &mut EventJournal;

    fn tree_nodes(&self) -> Vec<TabTreeNode> {
        self.tabs()
            .iter()
            .map(|tab| TabTreeNode {
                id: tab.id,
                parent_id: tab.parent_id,
                sort_key: tab.index,
            })
            .collect()
    }

    fn all_tree_rows(&self) -> Vec<TabTreeRow> {
        tree_rows(&self.tree_nodes())
    }

    fn request_ui_redraw(&mut self) {}

    fn on_viewport_scrolled(&mut self, position: usize, offset: usize, source: &str) {
        let id = self.tabs()[position].id;
        self.event_journal_mut().commit(
            EventKind::TerminalViewport,
            Some(id),
            serde_json::json!({
                "scrollback_offset": offset,
                "source": source,
            }),
        );
        self.request_ui_redraw();
    }

    /// Simplified UI snapshot JSON for automation; None = host handles ui-snapshot itself.
    fn ui_snapshot_json(&mut self) -> Option<String> {
        None
    }
}

pub(crate) fn set_tab_parent_on_host(
    host: &mut dyn ControlHost,
    child_id: u64,
    parent_id: Option<u64>,
) -> Result<(), String> {
    let nodes = host.tree_nodes();
    let Some(child_position) = host.tabs().iter().position(|tab| tab.id == child_id) else {
        return Err(format!("can't find child tab: @{child_id}"));
    };
    if let Some(parent_id) = parent_id {
        if !host.tabs().iter().any(|tab| tab.id == parent_id) {
            return Err(format!("can't find parent tab: @{parent_id}"));
        }
        if would_create_cycle(&nodes, child_id, parent_id) {
            return Err(format!(
                "moving @{child_id} under @{parent_id} would create a cycle"
            ));
        }
    }
    let previous_parent_id = host.tabs()[child_position].parent_id;
    host.tabs_mut()[child_position].parent_id = parent_id;
    host.event_journal_mut().commit(
        EventKind::TabParent,
        Some(child_id),
        serde_json::json!({
            "previous_parent_id": previous_parent_id,
            "parent_id": parent_id,
        }),
    );
    Ok(())
}

fn ui_snapshot_response(host: &mut dyn ControlHost) -> IpcResponse {
    match host.ui_snapshot_json() {
        Some(json) => IpcResponse::success(json),
        None => IpcResponse::success(""),
    }
}

fn send_composer_at_position(host: &mut dyn ControlHost, position: usize) -> IpcResponse {
    if let Some(secret) = host.tabs_mut()[position].sensitive_composer.take() {
        let marker = host.tabs_mut()[position].sensitive_proxy_marker.take();
        if !host.tabs_mut()[position].submit_sensitive(secret.expose()) {
            host.tabs_mut()[position].sensitive_composer = Some(secret);
            host.tabs_mut()[position].sensitive_proxy_marker = marker;
            return IpcResponse::failure("a composer submission is already pending");
        }
        let Some(marker) = marker else {
            host.tabs_mut()[position].proxy.mark_failed().ok();
            return IpcResponse::failure("sensitive proxy draft lost its confirmation identity");
        };
        if let Err(error) = host.tabs_mut()[position].proxy.mark_submitted() {
            return IpcResponse::failure(error.to_string());
        }
        let id = host.tabs_mut()[position].id;
        host.tabs_mut()[position].begin_proxy_confirmation(marker);
        host.event_journal_mut().commit(
            EventKind::WorkingContextProxySubmitted,
            Some(id),
            serde_json::json!({
                "sensitive": true,
                "application_state": "submitted",
            }),
        );
        if host.active_id() == Some(id) {
            host.load_composer_to_ui();
        }
        return IpcResponse::success("");
    }
    let text = mem::take(&mut host.tabs_mut()[position].composer);
    if !text.is_empty() && !host.tabs_mut()[position].submit(&text) {
        host.tabs_mut()[position].composer = text;
        return IpcResponse::failure("a composer submission is already pending");
    }
    if !text.is_empty() {
        let id = host.tabs_mut()[position].id;
        host.event_journal_mut().commit(
            EventKind::ComposerSubmitted,
            Some(id),
            serde_json::json!({
                "length": text.chars().count(),
            }),
        );
    }
    if host.active_id() == Some(host.tabs()[position].id) {
        host.load_composer_to_ui();
    }
    IpcResponse::success("")
}

fn dispatch_shared_ui_action(host: &mut dyn ControlHost, args: &[String]) -> Option<IpcResponse> {
    let action = args.get(1).map(String::as_str)?;
    if let Err(error) = host.admit_ui_action(action) {
        return Some(IpcResponse::failure(error));
    }
    match action {
        "new-tab" => match host.create_tab(None, Vec::new(), Vec::new(), true, None) {
            Ok(index) => {
                if let Some(id) = host
                    .tabs()
                    .iter()
                    .find(|tab| tab.index == index)
                    .map(|tab| tab.id)
                {
                    host.after_create_tab(id, None);
                }
                Some(ui_snapshot_response(host))
            }
            Err(error) => Some(IpcResponse::failure(error)),
        },
        "new-child" => {
            let Some(parent_position) =
                resolve_target_position(host.tabs(), host.active_id(), option_value(args, "-t"))
                    .or_else(|| resolve_target_position(host.tabs(), host.active_id(), None))
            else {
                return Some(IpcResponse::failure("can't find parent tab"));
            };
            let parent_id = host.tabs()[parent_position].id;
            match host.create_tab(
                Some("New child".to_owned()),
                Vec::new(),
                Vec::new(),
                true,
                Some(parent_id),
            ) {
                Ok(index) => {
                    if let Some(id) = host
                        .tabs()
                        .iter()
                        .find(|tab| tab.index == index)
                        .map(|tab| tab.id)
                    {
                        host.after_create_tab(id, Some(parent_id));
                    }
                    Some(ui_snapshot_response(host))
                }
                Err(error) => Some(IpcResponse::failure(error)),
            }
        }
        "select-tab" => {
            let Some(position) =
                resolve_target_position(host.tabs(), host.active_id(), option_value(args, "-t"))
            else {
                return Some(IpcResponse::failure("can't find tab"));
            };
            host.sync_composer_from_ui();
            if let Err(error) = host.select_tab_at(position) {
                return Some(IpcResponse::failure(error));
            }
            let _ = host.set_ipc_focus_surface("terminal");
            Some(ui_snapshot_response(host))
        }
        "close-tab" => {
            let Some(position) =
                resolve_target_position(host.tabs(), host.active_id(), option_value(args, "-t"))
            else {
                return Some(IpcResponse::failure("can't find tab"));
            };
            let id = host.tabs()[position].id;
            if let Err(error) = host.close_tab_by_ui_action(id) {
                return Some(IpcResponse::failure(error));
            }
            Some(ui_snapshot_response(host))
        }
        "composer-send" => {
            match host.prepare_composer_send() {
                Ok(true) => return Some(ui_snapshot_response(host)),
                Ok(false) => {}
                Err(error) => return Some(IpcResponse::failure(error)),
            }
            host.sync_composer_from_ui();
            let Some(position) =
                resolve_target_position(host.tabs(), host.active_id(), option_value(args, "-t"))
                    .or_else(|| resolve_target_position(host.tabs(), host.active_id(), None))
            else {
                return Some(IpcResponse::failure("can't find window"));
            };
            let response = send_composer_at_position(host, position);
            if response.ok {
                Some(ui_snapshot_response(host))
            } else {
                Some(response)
            }
        }
        "tabs-show" => match host.set_tabs_visible(true, "semantic", UI_TABS_SHOW) {
            Ok(()) => Some(ui_snapshot_response(host)),
            Err(error) => Some(IpcResponse::failure(error)),
        },
        "tabs-hide" => match host.set_tabs_visible(false, "semantic", UI_TABS_HIDE) {
            Ok(()) => Some(ui_snapshot_response(host)),
            Err(error) => Some(IpcResponse::failure(error)),
        },
        "tabs-toggle" | "toggle-tabs" => {
            let visible = !host.config_tabs_visible();
            match host.set_tabs_visible(visible, "semantic", UI_TABS_TOGGLE) {
                Ok(()) => Some(ui_snapshot_response(host)),
                Err(error) => Some(IpcResponse::failure(error)),
            }
        }
        "tabs-set-width" => {
            let Some(width) =
                option_value(args, "--width").and_then(|value| value.parse::<i32>().ok())
            else {
                return Some(IpcResponse::failure("tabs-set-width requires --width"));
            };
            let width = clamp_tabs_width(width);
            match host.set_tabs_width(width, "semantic", UI_TABS_SET_WIDTH) {
                Ok(()) => Some(ui_snapshot_response(host)),
                Err(error) => Some(IpcResponse::failure(error)),
            }
        }
        "toggle-tree" => {
            let Some(position) =
                resolve_target_position(host.tabs(), host.active_id(), option_value(args, "-t"))
            else {
                return Some(IpcResponse::failure("can't find tab"));
            };
            let id = host.tabs()[position].id;
            match host.toggle_tab_collapsed(id) {
                Ok(()) => Some(ui_snapshot_response(host)),
                Err(error) => Some(IpcResponse::failure(error)),
            }
        }
        "edit-tab" => {
            let Some(position) =
                resolve_target_position(host.tabs(), host.active_id(), option_value(args, "-t"))
            else {
                return Some(IpcResponse::failure("can't find tab"));
            };
            let id = host.tabs()[position].id;
            match host.open_tab_editor(id) {
                Ok(()) => Some(ui_snapshot_response(host)),
                Err(error) => Some(IpcResponse::failure(error)),
            }
        }
        "open-settings" => match host.open_settings_modal() {
            Ok(()) => Some(ui_snapshot_response(host)),
            Err(error) => Some(IpcResponse::failure(error)),
        },
        "settings-theme-dark" => {
            host.preview_settings_theme(ThemeId::Dark);
            Some(ui_snapshot_response(host))
        }
        "settings-theme-light" => {
            host.preview_settings_theme(ThemeId::Light);
            Some(ui_snapshot_response(host))
        }
        "settings-apply" => match host.close_settings_modal(true) {
            Ok(()) => Some(ui_snapshot_response(host)),
            Err(error) => Some(IpcResponse::failure(error)),
        },
        "tab-editor-save" => match host.finish_tab_editor(true) {
            Ok(()) => Some(ui_snapshot_response(host)),
            Err(error) => Some(IpcResponse::failure(error)),
        },
        "tab-editor-cancel" => match host.finish_tab_editor(false) {
            Ok(()) => Some(ui_snapshot_response(host)),
            Err(error) => Some(IpcResponse::failure(error)),
        },
        "cancel" => match host.ui_action_cancel() {
            Ok(true) => Some(ui_snapshot_response(host)),
            Ok(false) => Some(IpcResponse::failure("no modal is pending")),
            Err(error) => Some(IpcResponse::failure(error)),
        },
        "confirm" => match host.ui_action_confirm() {
            Ok(true) => Some(ui_snapshot_response(host)),
            Ok(false) => Some(IpcResponse::failure("no confirmation is pending")),
            Err(error) => Some(IpcResponse::failure(error)),
        },
        "copy-selection" => match host.copy_selection() {
            Ok(()) => Some(ui_snapshot_response(host)),
            Err(error) => Some(IpcResponse::failure(error)),
        },
        _ => None,
    }
}

pub(crate) fn dispatch_shared_command(
    host: &mut dyn ControlHost,
    args: &[String],
) -> Option<IpcResponse> {
    let command = command_name(args)?;

    match command {
        "start-server" => Some(IpcResponse::success("")),
        "protocol-info" => Some(IpcResponse::success(
            crate::client::protocol_info_json_with_ui_bridge(
                "running_host",
                host.ui_bridge_facts(),
            ),
        )),
        "ui-hello" => {
            let Some(minimum) =
                option_value(args, "--minimum").and_then(|value| value.parse::<u32>().ok())
            else {
                return Some(IpcResponse::typed_failure(
                    "ui-hello requires numeric --minimum",
                    "ui_hello_invalid_arguments",
                    "configuration",
                    false,
                ));
            };
            let Some(maximum) =
                option_value(args, "--maximum").and_then(|value| value.parse::<u32>().ok())
            else {
                return Some(IpcResponse::typed_failure(
                    "ui-hello requires numeric --maximum",
                    "ui_hello_invalid_arguments",
                    "configuration",
                    false,
                ));
            };
            let client_build = match option_value(args, "--client-build-json") {
                Some(value) => match serde_json::from_str(value) {
                    Ok(identity) => Some(identity),
                    Err(error) => {
                        return Some(IpcResponse::typed_failure(
                            format!("ui-hello --client-build-json is invalid: {error}"),
                            "ui_hello_invalid_arguments",
                            "configuration",
                            false,
                        ));
                    }
                },
                None => None,
            };
            let request = UiHelloRequest {
                schema_version: UI_HELLO_SCHEMA_VERSION,
                client_id: option_value(args, "--client-id")
                    .unwrap_or("agenterm-cli")
                    .to_owned(),
                protocol_range: UiProtocolRange { minimum, maximum },
                client_build,
            };
            match ui_hello_response(host, request) {
                Ok(response) => match serde_json::to_string_pretty(&response) {
                    Ok(json) => Some(IpcResponse::success(json)),
                    Err(error) => Some(IpcResponse::typed_failure(
                        error.to_string(),
                        "ui_hello_serialization_failed",
                        "internal",
                        false,
                    )),
                },
                Err(error) => Some(IpcResponse::typed_failure(
                    error,
                    "ui_hello_invalid_arguments",
                    "configuration",
                    false,
                )),
            }
        }
        "ui-bootstrap" => match ui_bootstrap_snapshot(host) {
            Ok(snapshot) => match serde_json::to_string_pretty(&snapshot) {
                Ok(json) => Some(IpcResponse::success(json)),
                Err(error) => Some(IpcResponse::typed_failure(
                    error.to_string(),
                    "ui_bootstrap_serialization_failed",
                    "internal",
                    false,
                )),
            },
            Err(error) => Some(IpcResponse::typed_failure(
                error,
                "ui_bootstrap_unavailable",
                "precondition",
                true,
            )),
        },
        "ui-deltas" => {
            let Some(epoch) = option_value(args, "--epoch") else {
                return Some(IpcResponse::typed_failure(
                    "ui-deltas requires --epoch",
                    "ui_delta_invalid_arguments",
                    "configuration",
                    false,
                ));
            };
            let Some(after) =
                option_value(args, "--after").and_then(|value| value.parse::<u64>().ok())
            else {
                return Some(IpcResponse::typed_failure(
                    "ui-deltas requires numeric --after",
                    "ui_delta_invalid_arguments",
                    "configuration",
                    false,
                ));
            };
            let limit = match option_value(args, "--limit") {
                Some(value) => match value.parse::<usize>() {
                    Ok(limit) if (1..=UI_DELTA_MAX_EVENTS).contains(&limit) => limit,
                    _ => {
                        return Some(IpcResponse::typed_failure(
                            format!("ui-deltas --limit must be from 1 to {UI_DELTA_MAX_EVENTS}"),
                            "ui_delta_invalid_arguments",
                            "configuration",
                            false,
                        ));
                    }
                },
                None => UI_DELTA_MAX_EVENTS,
            };
            match host.event_journal().read_after(epoch, after, limit) {
                Ok(events) => {
                    let position = host.event_journal().position();
                    match ui_delta_batch(host, after, &position, &events) {
                        Ok(batch) => match serde_json::to_string_pretty(&batch) {
                            Ok(json) => Some(IpcResponse::success(json)),
                            Err(error) => Some(IpcResponse::typed_failure(
                                error.to_string(),
                                "ui_delta_serialization_failed",
                                "internal",
                                false,
                            )),
                        },
                        Err(error) => Some(IpcResponse::typed_failure(
                            error,
                            "ui_delta_unavailable",
                            "precondition",
                            true,
                        )),
                    }
                }
                Err(error) => Some(IpcResponse::typed_failure(
                    error.to_json().to_string(),
                    error.code(),
                    "precondition",
                    false,
                )),
            }
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
                    .map(|tab| render_format(format, tab, &session_name, active == Some(tab.id)))
                    .collect::<Vec<_>>()
                    .join("\n"),
            ))
        }
        "lsp" | "list-panes" => {
            let format = option_value(args, "-F")
                .unwrap_or("#{pane_id}: [#{pane_width}x#{pane_height}] #{pane_current_command}");
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
                tabs.into_iter()
                    .map(|tab| render_format(format, tab, &session_name, active == Some(tab.id)))
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
        "new" | "new-session" | "neww" | "new-window" => {
            let (title, detached, child_command) = parse_new_command(args);
            let requested_session = matches!(command, "new" | "new-session")
                .then(|| option_value(args, "-s").map(str::to_owned))
                .flatten();
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
            match host.create_tab(title, child_command, tab_environment, !detached, parent_id) {
                Ok(index) => {
                    if let Some(session) = requested_session {
                        host.set_session_name(session);
                    }
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
        "new-agent" => {
            let (title, detached, agent_arguments) = parse_new_command(args);
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
            let mut child_command = if let Some(program) = option_value(args, "--program") {
                vec![program.to_owned()]
            } else {
                vec![
                    std::env::var("COMSPEC")
                        .unwrap_or_else(|_| r"C:\Windows\System32\cmd.exe".to_owned()),
                    "/d".to_owned(),
                    "/c".to_owned(),
                    "codex".to_owned(),
                ]
            };
            if has_option(args, "--yolo") {
                child_command.push("--dangerously-bypass-approvals-and-sandbox".to_owned());
            }
            child_command.extend(agent_arguments);
            match host.create_tab(
                title.or_else(|| Some("Codex".to_owned())),
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
                        .expect("newly created agent tab must remain present");
                    Some(IpcResponse::success(render_format(
                        format,
                        tab,
                        host.session_name(),
                        host.active_id() == Some(tab.id),
                    )))
                }
                Err(error) => Some(IpcResponse::failure(format!(
                    "failed to start Codex agent tab: {error}"
                ))),
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
            let Some(position) = resolve_target_position(host.tabs(), host.active_id(), None)
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
        "scroll-pane" => {
            let Some(position) =
                resolve_target_position(host.tabs(), host.active_id(), option_value(args, "-t"))
            else {
                return Some(IpcResponse::failure("can't find pane"));
            };
            let values = positional_values(args, &["-t"], &[]);
            let Some(action) = values.first().copied() else {
                return Some(IpcResponse::failure(
                    "usage: scroll-pane [-t target] \
                     up|down|page-up|page-down|top|bottom [rows]",
                ));
            };
            if values.len() > 2 {
                return Some(IpcResponse::failure(
                    "scroll-pane accepts at most one row count",
                ));
            }
            let count = match values.get(1) {
                Some(value) => match value.parse::<usize>() {
                    Ok(count) if count > 0 => Some(count),
                    _ => {
                        return Some(IpcResponse::failure(
                            "scroll-pane row count must be a positive integer",
                        ));
                    }
                },
                None => None,
            };
            match host.tabs_mut()[position].scroll_viewport(action, count) {
                Ok(offset) => {
                    host.on_viewport_scrolled(position, offset, "control");
                    Some(IpcResponse::success(offset.to_string()))
                }
                Err(error) => Some(IpcResponse::failure(format!("{error:#}"))),
            }
        }
        "cwd-prepare" | "cwd-prepare-append" | "cwd-prepare-replace" => {
            let Some(position) =
                resolve_target_position(host.tabs(), host.active_id(), option_value(args, "-t"))
            else {
                return Some(IpcResponse::failure("can't find tab"));
            };
            let Some(path) = option_value(args, "--path") else {
                return Some(IpcResponse::failure(format!("{command} requires --path")));
            };
            let mode = match command {
                "cwd-prepare-append" => "append",
                "cwd-prepare-replace" => "replace",
                _ => option_value(args, "--mode").unwrap_or("empty-only"),
            };
            if !matches!(mode, "empty-only" | "append" | "replace") {
                return Some(IpcResponse::failure(
                    "CWD composer mode must be empty-only, append, or replace",
                ));
            }
            let id = host.tabs()[position].id;
            match host.prepare_cwd(id, path, mode) {
                Ok(()) => Some(ui_snapshot_response(host)),
                Err(error) => Some(IpcResponse::failure(error)),
            }
        }
        "cwd-send-now" => {
            let Some(position) =
                resolve_target_position(host.tabs(), host.active_id(), option_value(args, "-t"))
            else {
                return Some(IpcResponse::failure("can't find tab"));
            };
            let Some(path) = option_value(args, "--path") else {
                return Some(IpcResponse::failure("cwd-send-now requires --path"));
            };
            let id = host.tabs()[position].id;
            match host.send_cwd_now(id, path) {
                Ok(()) => Some(ui_snapshot_response(host)),
                Err(error) => Some(IpcResponse::failure(error)),
            }
        }
        "read-events" => {
            let Some(epoch) = option_value(args, "--epoch") else {
                return Some(IpcResponse::failure("read-events requires --epoch"));
            };
            let Some(after) =
                option_value(args, "--after").and_then(|value| value.parse::<u64>().ok())
            else {
                return Some(IpcResponse::failure(
                    "read-events requires a numeric --after sequence",
                ));
            };
            let limit = match option_value(args, "--limit") {
                Some(value) => match value.parse::<usize>() {
                    Ok(limit) if (1..=1_024).contains(&limit) => limit,
                    _ => {
                        return Some(IpcResponse::failure(
                            "read-events --limit must be from 1 to 1024",
                        ));
                    }
                },
                None => 256,
            };
            match host.event_journal().read_after(epoch, after, limit) {
                Ok(events) => Some(IpcResponse::success(
                    serde_json::to_string_pretty(&serde_json::json!({
                        "position": host.event_journal().position(),
                        "events": events,
                    }))
                    .unwrap_or_default(),
                )),
                Err(error) => Some(IpcResponse::typed_failure(
                    error.to_json().to_string(),
                    error.code(),
                    "precondition",
                    false,
                )),
            }
        }
        "list-tab-tree" => {
            let format = option_value(args, "-F").unwrap_or("#{window_id}:#{window_name}");
            let rows = host.all_tree_rows();
            let session_name = host.session_name().to_owned();
            let active = host.active_id();
            let tabs = host.tabs();
            Some(IpcResponse::success(
                rows.iter()
                    .filter_map(|row| {
                        let tab = tabs.iter().find(|tab| tab.id == row.id)?;
                        let branch = if row.depth == 0 {
                            String::new()
                        } else {
                            let mut branch = row
                                .guides
                                .iter()
                                .map(|continues| if *continues { "│ " } else { "  " })
                                .collect::<String>();
                            branch.push_str(if row.is_last { "└─ " } else { "├─ " });
                            branch
                        };
                        let rendered =
                            render_format(format, tab, &session_name, active == Some(tab.id))
                                .replace("#{tab_depth}", &row.depth.to_string())
                                .replace(
                                    "#{tab_has_children}",
                                    if tabs.iter().any(|child| child.parent_id == Some(tab.id)) {
                                        "1"
                                    } else {
                                        "0"
                                    },
                                );
                        Some(format!("{branch}{rendered}"))
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
            ))
        }
        "dump-cells" => {
            let Some(position) =
                resolve_target_position(host.tabs(), host.active_id(), option_value(args, "-t"))
            else {
                return Some(IpcResponse::failure("can't find pane"));
            };
            let requested_row =
                option_value(args, "-r").and_then(|value| value.parse::<u16>().ok());
            let tab = &host.tabs()[position];
            let screen = tab.parser.screen();
            let mut cells = Vec::new();
            for row in 0..tab.last_size.0 {
                if requested_row.is_some_and(|requested| requested != row) {
                    continue;
                }
                for col in 0..tab.last_size.1 {
                    let Some(cell) = screen.cell(row, col) else {
                        continue;
                    };
                    let foreground = format!("{:?}", cell.fgcolor());
                    let background = format!("{:?}", cell.bgcolor());
                    if cell.contents().is_empty()
                        && foreground == "Default"
                        && background == "Default"
                        && !cell.inverse()
                    {
                        continue;
                    }
                    cells.push(serde_json::json!({
                        "row": row,
                        "col": col,
                        "text": cell.contents(),
                        "fg": foreground,
                        "bg": background,
                        "inverse": cell.inverse(),
                        "wide_continuation": cell.is_wide_continuation(),
                    }));
                }
            }
            match serde_json::to_string_pretty(&serde_json::json!({
                "window_id": format!("@{}", tab.id),
                "rows": tab.last_size.0,
                "cols": tab.last_size.1,
                "cells": cells,
            })) {
                Ok(json) => Some(IpcResponse::success(json)),
                Err(error) => Some(IpcResponse::failure(error.to_string())),
            }
        }
        "workspace-info" => Some(IpcResponse::success(
            serde_json::to_string_pretty(&serde_json::json!({
                "path": workspace_path(),
                "version": 1,
                "tab_count": host.tabs().len(),
                "active_id": host.active_id().map(|id| format!("@{id}")),
                "restore_behavior": "restart-processes",
            }))
            .unwrap_or_default(),
        )),
        "renamew" | "rename-window" => {
            let Some(name) = last_positional(args, &["-t"]) else {
                return Some(IpcResponse::failure(
                    "usage: rename-window [-t target] new-name",
                ));
            };
            let Some(position) =
                resolve_target_position(host.tabs(), host.active_id(), option_value(args, "-t"))
            else {
                return Some(IpcResponse::failure("can't find window"));
            };
            let id = host.tabs()[position].id;
            let previous_name = host.tabs()[position].title.clone();
            host.tabs_mut()[position].title = name.to_owned();
            host.event_journal_mut().commit(
                EventKind::TabRenamed,
                Some(id),
                serde_json::json!({
                    "previous_name": previous_name,
                    "name": name,
                }),
            );
            host.request_ui_redraw();
            Some(IpcResponse::success(""))
        }
        "set-tab-note" => {
            let Some(position) =
                resolve_target_position(host.tabs(), host.active_id(), option_value(args, "-t"))
            else {
                return Some(IpcResponse::failure("can't find tab"));
            };
            let note = positional_values(args, &["-t"], &[]).join(" ");
            let id = host.tabs()[position].id;
            let previous_note = mem::replace(&mut host.tabs_mut()[position].note, note.clone());
            host.event_journal_mut().commit(
                EventKind::TabNote,
                Some(id),
                serde_json::json!({
                    "previous_note": previous_note,
                    "note": note,
                }),
            );
            host.request_ui_redraw();
            Some(IpcResponse::success(""))
        }
        "show-tab-note" => {
            let Some(position) =
                resolve_target_position(host.tabs(), host.active_id(), option_value(args, "-t"))
            else {
                return Some(IpcResponse::failure("can't find tab"));
            };
            Some(IpcResponse::success(host.tabs()[position].note.clone()))
        }
        "set-tab-parent" => {
            let Some(child_position) =
                resolve_target_position(host.tabs(), host.active_id(), option_value(args, "-t"))
            else {
                return Some(IpcResponse::failure("can't find child tab"));
            };
            let Some(parent_target) = option_value(args, "--parent") else {
                return Some(IpcResponse::failure(
                    "usage: set-tab-parent -t child --parent parent|root",
                ));
            };
            let parent_id = match host.resolve_parent_id(parent_target) {
                Ok(parent_id) => parent_id,
                Err(error) => return Some(IpcResponse::failure(error)),
            };
            let child_id = host.tabs()[child_position].id;
            match set_tab_parent_on_host(host, child_id, parent_id) {
                Ok(()) => {
                    host.request_ui_redraw();
                    Some(IpcResponse::success(""))
                }
                Err(error) => Some(IpcResponse::failure(error)),
            }
        }
        "show-tab-parent" => {
            let Some(position) =
                resolve_target_position(host.tabs(), host.active_id(), option_value(args, "-t"))
            else {
                return Some(IpcResponse::failure("can't find tab"));
            };
            Some(IpcResponse::success(
                host.tabs()[position]
                    .parent_id
                    .map(|id| format!("@{id}"))
                    .unwrap_or_else(|| "root".to_owned()),
            ))
        }
        "next" | "next-window" | "prev" | "previous-window" => {
            let direction = if matches!(command, "next" | "next-window") {
                1
            } else {
                -1
            };
            let Some(position) = host.adjacent_tab_position(direction) else {
                return Some(IpcResponse::failure("no windows"));
            };
            match host.select_tab_at(position) {
                Ok(()) => Some(IpcResponse::success("")),
                Err(error) => Some(IpcResponse::failure(error)),
            }
        }
        "ui-snapshot" => host.ui_snapshot_json().map(IpcResponse::success),
        "show-composer" => {
            host.sync_composer_from_ui();
            let Some(position) =
                resolve_target_position(host.tabs(), host.active_id(), option_value(args, "-t"))
            else {
                return Some(IpcResponse::failure("can't find window"));
            };
            if host.tabs()[position].sensitive_composer.is_some() {
                return Some(IpcResponse::failure(
                    "Composer contains a sensitive proxy draft; content is redacted",
                ));
            }
            Some(IpcResponse::success(host.tabs()[position].composer.clone()))
        }
        "set-composer" => {
            let Some(position) =
                resolve_target_position(host.tabs(), host.active_id(), option_value(args, "-t"))
            else {
                return Some(IpcResponse::failure("can't find window"));
            };
            if host.tabs()[position].sensitive_composer.is_some() {
                return Some(IpcResponse::failure(
                    "Composer contains a sensitive proxy draft; send or discard it first",
                ));
            }
            let text = positional_values(args, &["-t"], &[]).join(" ");
            match host.apply_set_composer(position, text) {
                Ok(()) => Some(IpcResponse::success("")),
                Err(error) => Some(IpcResponse::failure(error)),
            }
        }
        "send-composer" => {
            host.sync_composer_from_ui();
            let Some(position) =
                resolve_target_position(host.tabs(), host.active_id(), option_value(args, "-t"))
            else {
                return Some(IpcResponse::failure("can't find window"));
            };
            Some(send_composer_at_position(host, position))
        }
        "set-setting" => {
            let Some(key) = args.get(1).map(String::as_str) else {
                return Some(IpcResponse::failure("set-setting requires a key and value"));
            };
            let value = args.get(2..).unwrap_or_default().join(" ");
            match host.apply_setting(key, &value) {
                Ok(()) => Some(ui_snapshot_response(host)),
                Err(error) => Some(IpcResponse::failure(error)),
            }
        }
        "ui-action" => dispatch_shared_ui_action(host, args),
        "get-settings" => Some(IpcResponse::success(host.settings_json())),
        "focus" => {
            let surface = args.get(1).map(String::as_str).unwrap_or("terminal");
            if let Some(position) =
                resolve_target_position(host.tabs(), host.active_id(), option_value(args, "-t"))
            {
                host.sync_composer_from_ui();
                if let Err(error) = host.select_tab_at(position) {
                    return Some(IpcResponse::failure(error));
                }
            }
            if let Err(error) = host.set_ipc_focus_surface(surface) {
                return Some(IpcResponse::failure(error));
            }
            match host.ui_snapshot_json() {
                Some(json) => Some(IpcResponse::success(json)),
                None => Some(IpcResponse::failure(format!(
                    "focus surface is unavailable: {surface}"
                ))),
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
