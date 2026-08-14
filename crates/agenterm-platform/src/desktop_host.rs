//! Resident desktop action host facade.

use std::{collections::HashSet, time::Duration};

pub use crate::contract::desktop_host::{
    DesktopActionSpec, DesktopHostError, MAX_DESKTOP_ACTIONS, MAX_DESKTOP_LABEL_BYTES,
    MAX_DESKTOP_SHORTCUT_BYTES,
};

pub struct DesktopHost {
    inner: crate::selected::desktop_host::DesktopHost,
}

impl DesktopHost {
    pub fn open(actions: Vec<DesktopActionSpec>) -> Result<Self, DesktopHostError> {
        validate_actions(&actions)?;
        crate::selected::desktop_host::DesktopHost::open(actions).map(|inner| Self { inner })
    }

    pub fn poll_action(&mut self, timeout: Duration) -> Result<Option<u32>, DesktopHostError> {
        self.inner.poll_action(timeout)
    }

    pub fn close(&mut self) -> Result<(), DesktopHostError> {
        self.inner.close()
    }
}

pub fn capability_status() -> crate::CapabilityStatus {
    crate::capability_status(crate::Capability::DesktopHost)
}

fn validate_actions(actions: &[DesktopActionSpec]) -> Result<(), DesktopHostError> {
    if actions.is_empty() || actions.len() > MAX_DESKTOP_ACTIONS {
        return Err(DesktopHostError::failed(
            "desktop_host_bad_action_count",
            format!("action count must be in 1..={MAX_DESKTOP_ACTIONS}"),
        ));
    }
    let mut ids = HashSet::with_capacity(actions.len());
    let mut shortcuts = HashSet::with_capacity(actions.len());
    for action in actions {
        if action.action_id == 0 {
            return Err(DesktopHostError::failed(
                "desktop_host_bad_action_id",
                "action id 0 is reserved for poll timeout",
            ));
        }
        if !ids.insert(action.action_id) {
            return Err(DesktopHostError::failed(
                "desktop_host_duplicate_action_id",
                format!("duplicate action id {}", action.action_id),
            ));
        }
        if action.label.is_empty()
            || action.label.len() > MAX_DESKTOP_LABEL_BYTES
            || action.label.contains('\0')
        {
            return Err(DesktopHostError::failed(
                "desktop_host_bad_label",
                format!("label must contain 1..={MAX_DESKTOP_LABEL_BYTES} UTF-8 bytes and no NUL"),
            ));
        }
        if let Some(shortcut) = &action.shortcut {
            if shortcut.is_empty()
                || shortcut.len() > MAX_DESKTOP_SHORTCUT_BYTES
                || shortcut.contains('\0')
            {
                return Err(DesktopHostError::failed(
                    "desktop_host_bad_shortcut",
                    format!(
                        "shortcut must contain 1..={MAX_DESKTOP_SHORTCUT_BYTES} UTF-8 bytes and no NUL"
                    ),
                ));
            }
            let canonical = shortcut.to_ascii_lowercase().replace(' ', "");
            if !shortcuts.insert(canonical) {
                return Err(DesktopHostError::failed(
                    "desktop_host_duplicate_hotkey",
                    format!("duplicate shortcut {shortcut:?}"),
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_zero_and_duplicate_action_ids() {
        let zero = vec![DesktopActionSpec::new(0, "Quit")];
        assert!(matches!(
            validate_actions(&zero),
            Err(DesktopHostError::Failed { .. })
        ));
        let duplicate = vec![
            DesktopActionSpec::new(7, "Left"),
            DesktopActionSpec::new(7, "Quit"),
        ];
        assert!(matches!(
            validate_actions(&duplicate),
            Err(DesktopHostError::Failed { .. })
        ));
    }

    #[test]
    fn rejects_duplicate_hotkeys_case_insensitively() {
        let actions = vec![
            DesktopActionSpec::new(1, "Left").with_shortcut("Alt+Win+Left"),
            DesktopActionSpec::new(2, "Other").with_shortcut("alt + win + left"),
        ];
        assert!(matches!(
            validate_actions(&actions),
            Err(DesktopHostError::Failed { .. })
        ));
    }

    #[cfg(not(windows))]
    #[test]
    fn non_windows_open_is_typed_unsupported() {
        let error = DesktopHost::open(vec![DesktopActionSpec::new(1, "Quit")])
            .err()
            .expect("unsupported");
        assert!(matches!(error, DesktopHostError::Unsupported { .. }));
    }
}
