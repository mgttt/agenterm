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

/// Actions both hosts currently implement under the same public id.
///
/// Keep sorted. When promoting a host-only action, move it here and remove it
/// from the corresponding `*_ONLY` list.
pub const SHARED_UI_ACTIONS: &[&str] = &[
    "cancel",
    "close-window",
    "cwd-prepare",
    "cwd-prepare-append",
    "cwd-prepare-replace",
    "cwd-send-now",
    "keep-server-running",
    "open-control-center",
    "open-cwd-editor",
    "stop-server-and-exit",
    "terminal-paste",
    "window-activate",
    "window-maximize",
    "window-minimize",
    "window-resize",
    "window-restore",
];

/// Windows remote frontend only (`remote_frontend::execute_client_command`).
///
/// parity-gap: most of these are product workbench actions still dual-write
/// debt on Unix (tabs, settings, instance strip, selection, confirm). Prefer
/// promoting to [`SHARED_UI_ACTIONS`] when Unix gains the surface, not growing
/// this list casually.
pub const WINDOWS_ONLY_UI_ACTIONS: &[&str] = &[
    "close-tab",
    "composer-send",
    "confirm",
    "copy-selection",
    "edit-tab",
    "font-decrease",
    "font-increase",
    "instance-picker-cancel",
    "instance-picker-confirm",
    "instance-picker-next",
    "instance-picker-prev",
    "instance-picker-select",
    "new-child",
    "new-tab",
    "open-instance",
    "open-instance-picker",
    "open-settings",
    "select-server-tab",
    "select-tab",
    "settings-apply",
    "settings-current",
    "settings-defaults",
    "settings-font-toggle",
    "settings-preset-classic-day",
    "settings-preset-classic-night",
    "settings-preset-fancy-day",
    "settings-preset-fancy-night",
    "settings-reset-overrides",
    "settings-size-toggle",
    "settings-theme-dark",
    "settings-theme-light",
    "settings-theme-toggle",
    "tab-editor-cancel",
    "tab-editor-save",
    "tabs-hide",
    "tabs-set-width",
    "tabs-show",
    "tabs-toggle",
    "toggle-locale",
    "toggle-tabs",
    "toggle-tree",
];

/// Unix embedded frontend only (`unix/frontend` + `new_terminal::dispatch_ui_action`).
///
/// parity-gap: Windows opens tabs via `new-tab` / toolbar rather than this
/// dialog verb set; shell-* and create are shared dialog helpers that Unix
/// exposes as first-class `ui-action` ids.
pub const UNIX_ONLY_UI_ACTIONS: &[&str] = &[
    "create",
    "new-terminal-set-http-proxy",
    "new-terminal-set-https-proxy",
    "new-terminal-set-initial-command",
    "open-new-terminal",
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
    fn windows_catalog_literals_exist_in_remote_frontend() {
        let src = include_str!("../platform/adapters/windows/remote_frontend.rs");
        for action in windows_ui_actions() {
            let needle = format!("\"{action}\"");
            assert!(
                src.contains(&needle),
                "windows catalog action {action} missing as string literal in remote_frontend.rs \
                 (update catalog or implement the match arm)"
            );
        }
    }

    #[test]
    fn unix_catalog_literals_exist_in_unix_frontend_or_shared_helpers() {
        // Unix ui-action surface is split: top-level match, window_state apply,
        // and shared new_terminal dispatch.
        let unix_mod = include_str!("../platform/adapters/unix/frontend/mod.rs");
        let window_state = include_str!("../platform/adapters/unix/frontend/window_state.rs");
        let new_terminal = include_str!("new_terminal.rs");
        let haystack = format!("{unix_mod}\n{window_state}\n{new_terminal}");
        for action in unix_ui_actions() {
            let needle = format!("\"{action}\"");
            assert!(
                haystack.contains(&needle),
                "unix catalog action {action} missing as string literal in unix frontend / \
                 new_terminal helpers (update catalog or implement the match arm)"
            );
        }
    }

    #[test]
    fn shared_catalog_literals_exist_on_both_hosts() {
        let windows_src = include_str!("../platform/adapters/windows/remote_frontend.rs");
        let unix_mod = include_str!("../platform/adapters/unix/frontend/mod.rs");
        let window_state = include_str!("../platform/adapters/unix/frontend/window_state.rs");
        let new_terminal = include_str!("new_terminal.rs");
        let unix_haystack = format!("{unix_mod}\n{window_state}\n{new_terminal}");
        for action in SHARED_UI_ACTIONS {
            let needle = format!("\"{action}\"");
            assert!(
                windows_src.contains(&needle),
                "SHARED action {action} missing on Windows remote_frontend"
            );
            assert!(
                unix_haystack.contains(&needle),
                "SHARED action {action} missing on Unix frontend surface"
            );
        }
    }

    #[test]
    fn catalog_discipline_docs_point_at_agents_shared_first() {
        // Keep a durable pointer so refactors cannot drop the process rule.
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
            agents.contains("shared-first") || agents.contains("Shared-first") || agents.contains("shared-first"),
            "AGENTS.md must state shared-first UI discipline"
        );
    }
}
