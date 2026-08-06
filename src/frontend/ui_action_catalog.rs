//! Product `ui-action` surface catalogs for Win remote vs Unix embedded hosts.
//!
//! Both hosts still dual-write large match arms (ARCHITECTURE debt L2). This
//! module is the machine-checked **set** of action ids each host claims, plus
//! explicit host-only allowlists so a one-sided add fails the unit test until
//! the peer host or the allowlist is updated intentionally.
//!
//! **Shared-first rule** (see `AGENTS.md` § Platform crate vs product UI):
//! new product UI semantics go in `src/frontend/*` / `ui_geometry` first; host
//! adapters only present, wake, IME, and native IPC. Prefer adding to
//! [`SHARED_UI_ACTIONS`] and both host inventories in the same change.
//! Host-only entries require a short reason comment and a `parity-gap:` note
//! when the peer is still planned.
//!
//! **Discipline**: add or rename a product gesture → update this catalog
//! **before** (or in the same commit as) adapter match arms. A one-host arm
//! without an allowlist entry fails the set-diff tests below.
//!
//! **Where SHARED is implemented**:
//! - Authority / Unix embedded: `src/control_dispatch.rs` (`dispatch_shared_ui_action`)
//!   runs **before** host-local matches (see `unix/frontend` `handle_ipc`).
//! - Windows replaceable client: also listed in `remote_frontend::execute_client_command`
//!   for local UI chrome (often relays to the same server verbs).

/// Actions both hosts implement under the same public id.
///
/// Keep sorted. Includes verbs owned by the shared control dispatcher even when
/// a host also has a local match arm.
pub const SHARED_UI_ACTIONS: &[&str] = &[
    "cancel",
    "close-tab",
    "close-window",
    "composer-send",
    "confirm",
    "copy-selection",
    "cwd-prepare",
    "cwd-prepare-append",
    "cwd-prepare-replace",
    "cwd-send-now",
    "edit-tab",
    "font-decrease",
    "font-increase",
    "keep-server-running",
    "new-child",
    "new-tab",
    "open-control-center",
    "open-cwd-editor",
    "open-new-terminal",
    "open-settings",
    "select-tab",
    "settings-apply",
    "settings-preset-classic-day",
    "settings-preset-classic-night",
    "settings-preset-fancy-day",
    "settings-preset-fancy-night",
    "settings-theme-dark",
    "settings-theme-light",
    "stop-server-and-exit",
    "tab-editor-cancel",
    "tab-editor-save",
    "tabs-hide",
    "tabs-set-width",
    "tabs-show",
    "tabs-toggle",
    "terminal-paste",
    "toggle-locale",
    "toggle-tabs",
    "toggle-tree",
    "window-activate",
    "window-maximize",
    "window-minimize",
    "window-resize",
    "window-restore",
];

/// Windows remote frontend only (`remote_frontend::execute_client_command`).
///
/// Not implemented in `control_dispatch::dispatch_shared_ui_action`. Grouped
/// `parity-gap:` reasons (product workbench, **not** OS-crate leaks):
/// - **instance / server strip**: multi-instance strip + picker (S′); Unix still
///   returns an honest gap for picker depth.
/// - **settings scope chrome**: defaults/current/inherit toggles beyond the
///   shared preset/apply path already in SHARED.
pub const WINDOWS_ONLY_UI_ACTIONS: &[&str] = &[
    // parity-gap: instance picker + strip (S′); Unix product surface incomplete.
    "instance-picker-cancel",
    "instance-picker-confirm",
    "instance-picker-next",
    "instance-picker-prev",
    "instance-picker-select",
    "open-instance",
    "open-instance-picker",
    "select-server-tab",
    // parity-gap: settings scope/inheritance chrome beyond shared presets/apply.
    "settings-current",
    "settings-defaults",
    "settings-font-toggle",
    "settings-reset-overrides",
    "settings-size-toggle",
    "settings-theme-toggle",
];

/// Unix embedded frontend only (`unix/frontend` + `new_terminal::dispatch_ui_action`).
///
/// parity-gap: dialog field verbs / shell-* are first-class ui-actions on Unix;
/// Windows still drives the same shared `NewTerminalDialog` state via native
/// controls after `open-new-terminal` (now SHARED). Prefer promoting create /
/// shell-* only when Windows exposes matching ui-action ids for automation.
pub const UNIX_ONLY_UI_ACTIONS: &[&str] = &[
    "create",
    "new-terminal-set-http-proxy",
    "new-terminal-set-https-proxy",
    "new-terminal-set-initial-command",
    "shell-bash",
    "shell-cmd",
    "shell-default",
    "shell-powershell",
    "shell-primary",
    "shell-sh",
    "shell-zsh",
];

/// Full Windows host inventory = shared ∪ windows-only.
#[allow(dead_code)] // used by unit tests and future parity tooling
pub fn windows_ui_actions() -> Vec<&'static str> {
    merge_sorted(SHARED_UI_ACTIONS, WINDOWS_ONLY_UI_ACTIONS)
}

/// Full Unix host inventory = shared ∪ unix-only.
#[allow(dead_code)] // used by unit tests and future parity tooling
pub fn unix_ui_actions() -> Vec<&'static str> {
    merge_sorted(SHARED_UI_ACTIONS, UNIX_ONLY_UI_ACTIONS)
}

#[allow(dead_code)]
fn merge_sorted(a: &[&'static str], b: &[&'static str]) -> Vec<&'static str> {
    let mut out = Vec::with_capacity(a.len() + b.len());
    out.extend_from_slice(a);
    out.extend_from_slice(b);
    out.sort_unstable();
    out.dedup();
    out
}

#[allow(dead_code)]
fn is_strictly_sorted_unique(items: &[&str]) -> bool {
    items.windows(2).all(|pair| pair[0] < pair[1])
}

#[allow(dead_code)]
fn set_diff<'a>(left: &[&'a str], right: &[&str]) -> Vec<&'a str> {
    left.iter()
        .copied()
        .filter(|item| !right.contains(item))
        .collect()
}

/// Shared dispatcher + Unix local surfaces that can own a `ui-action` id.
fn unix_ui_action_haystack() -> String {
    let unix_mod = include_str!("../platform/adapters/unix/frontend/mod.rs");
    let window_state = include_str!("../platform/adapters/unix/frontend/window_state.rs");
    let new_terminal = include_str!("new_terminal.rs");
    let control_dispatch = include_str!("../control_dispatch.rs");
    format!("{control_dispatch}\n{unix_mod}\n{window_state}\n{new_terminal}")
}

/// Windows replaceable client + shared dispatcher (server authority path).
fn windows_ui_action_haystack() -> String {
    let remote = include_str!("../platform/adapters/windows/remote_frontend.rs");
    let control_dispatch = include_str!("../control_dispatch.rs");
    format!("{remote}\n{control_dispatch}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalogs_are_sorted_unique_and_disjoint() {
        assert!(
            is_strictly_sorted_unique(SHARED_UI_ACTIONS),
            "SHARED_UI_ACTIONS must be sorted unique"
        );
        assert!(
            is_strictly_sorted_unique(WINDOWS_ONLY_UI_ACTIONS),
            "WINDOWS_ONLY_UI_ACTIONS must be sorted unique"
        );
        assert!(
            is_strictly_sorted_unique(UNIX_ONLY_UI_ACTIONS),
            "UNIX_ONLY_UI_ACTIONS must be sorted unique"
        );

        for action in WINDOWS_ONLY_UI_ACTIONS {
            assert!(
                !SHARED_UI_ACTIONS.contains(action),
                "windows-only {action} collides with SHARED"
            );
            assert!(
                !UNIX_ONLY_UI_ACTIONS.contains(action),
                "windows-only {action} collides with UNIX_ONLY"
            );
        }
        for action in UNIX_ONLY_UI_ACTIONS {
            assert!(
                !SHARED_UI_ACTIONS.contains(action),
                "unix-only {action} collides with SHARED"
            );
        }
    }

    #[test]
    fn host_inventories_equal_shared_plus_allowlist() {
        let windows = windows_ui_actions();
        let unix = unix_ui_actions();

        let windows_shared = set_diff(&windows, WINDOWS_ONLY_UI_ACTIONS);
        let unix_shared = set_diff(&unix, UNIX_ONLY_UI_ACTIONS);

        assert_eq!(
            windows_shared, SHARED_UI_ACTIONS,
            "windows inventory minus windows-only must equal SHARED"
        );
        assert_eq!(
            unix_shared, SHARED_UI_ACTIONS,
            "unix inventory minus unix-only must equal SHARED"
        );
        assert_eq!(
            windows_shared, unix_shared,
            "shared sets must match after host-only allowlists \
             (add both hosts, or extend WINDOWS_ONLY / UNIX_ONLY with parity-gap)"
        );
    }

    #[test]
    fn windows_only_is_not_implemented_in_shared_dispatcher() {
        // Bookkeeping gate: if an id lands in control_dispatch, it is already
        // shared on Unix authority and must not sit in WINDOWS_ONLY.
        let shared_dispatcher = include_str!("../control_dispatch.rs");
        let misfiled: Vec<&str> = WINDOWS_ONLY_UI_ACTIONS
            .iter()
            .copied()
            .filter(|action| shared_dispatcher.contains(&format!("\"{action}\"")))
            .collect();
        assert!(
            misfiled.is_empty(),
            "WINDOWS_ONLY entries are implemented in control_dispatch and must \
             be promoted to SHARED_UI_ACTIONS: {misfiled:?}"
        );
    }

    #[test]
    fn windows_catalog_literals_exist_in_remote_or_shared_dispatch() {
        let haystack = windows_ui_action_haystack();
        for action in windows_ui_actions() {
            let needle = format!("\"{action}\"");
            assert!(
                haystack.contains(&needle),
                "windows catalog action {action} missing as string literal in \
                 remote_frontend.rs or control_dispatch.rs"
            );
        }
    }

    #[test]
    fn unix_catalog_literals_exist_in_unix_surface_or_shared_dispatch() {
        let haystack = unix_ui_action_haystack();
        for action in unix_ui_actions() {
            let needle = format!("\"{action}\"");
            assert!(
                haystack.contains(&needle),
                "unix catalog action {action} missing as string literal in \
                 control_dispatch / unix frontend / new_terminal helpers"
            );
        }
    }

    #[test]
    fn shared_catalog_literals_exist_on_both_host_surfaces() {
        let windows = windows_ui_action_haystack();
        let unix = unix_ui_action_haystack();
        for action in SHARED_UI_ACTIONS {
            let needle = format!("\"{action}\"");
            assert!(
                windows.contains(&needle),
                "SHARED action {action} missing on Windows surface \
                 (remote_frontend or control_dispatch)"
            );
            assert!(
                unix.contains(&needle),
                "SHARED action {action} missing on Unix surface \
                 (control_dispatch or unix frontend helpers)"
            );
        }
    }

    #[test]
    fn catalog_discipline_docs_point_at_agents_shared_first() {
        let agents = include_str!("../../AGENTS.md");
        assert!(
            agents.contains("Platform crate vs product UI"),
            "AGENTS.md must document platform vs product boundary"
        );
        assert!(
            agents.contains("ui_action_catalog"),
            "AGENTS.md must name the ui-action catalog gate"
        );
        assert!(
            agents.contains("shared-first") || agents.contains("Shared-first"),
            "AGENTS.md must state shared-first UI discipline"
        );
    }
}
