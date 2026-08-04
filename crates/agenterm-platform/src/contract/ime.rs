//! Platform-neutral IME composition contract.

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ImeEvent {
    Enabled,
    Preedit {
        text: String,
        cursor: Option<(usize, usize)>,
    },
    Commit(String),
    Disabled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ImeAction {
    None,
    UpdatePreedit {
        text: String,
        cursor: Option<(usize, usize)>,
    },
    ClearPreedit,
    CommitText(String),
}

/// A snapshot of the keyboard input method the focused surface is typing
/// through, for display in product chrome. Purely descriptive: hosts that
/// cannot report a given field leave it empty rather than guessing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct ImeStatus {
    /// Input method description reported by the host, e.g. "微软拼音".
    /// Empty when the active layout is a plain keyboard rather than an IME.
    pub name: String,
    /// Whether an IME is attached to the focused surface at all.
    pub available: bool,
    /// Whether the IME is open (accepting composition) rather than passing
    /// keystrokes straight through.
    pub open: bool,
    /// Whether conversion is in the IME's native script rather than
    /// alphanumeric — the "中/英" distinction users actually watch for.
    pub native_mode: bool,
    /// Whether punctuation/letters are full-width.
    pub full_shape: bool,
}

impl ImeStatus {
    /// Compact status-bar label. Kept host-independent so the wording stays
    /// identical across frontends and stays testable off-Windows.
    #[must_use]
    pub fn label(&self) -> String {
        if !self.available {
            return "IME: off".to_owned();
        }
        let name = if self.name.is_empty() {
            "keyboard"
        } else {
            self.name.as_str()
        };
        let mode = if self.open && self.native_mode {
            "native"
        } else {
            "latin"
        };
        let mut label = format!("IME: {name} · {mode}");
        if self.full_shape {
            label.push_str(" · full-width");
        }
        label
    }
}

#[cfg(test)]
mod tests {
    use super::ImeStatus;

    #[test]
    fn absent_input_method_reads_as_off() {
        assert_eq!(ImeStatus::default().label(), "IME: off");
    }

    #[test]
    fn native_and_latin_modes_are_distinguished() {
        let native = ImeStatus {
            name: "微软拼音".to_owned(),
            available: true,
            open: true,
            native_mode: true,
            full_shape: false,
        };
        assert_eq!(native.label(), "IME: 微软拼音 · native");

        // A closed IME still names itself, but types latin.
        let closed = ImeStatus {
            open: false,
            ..native.clone()
        };
        assert_eq!(closed.label(), "IME: 微软拼音 · latin");

        // Open but converting alphanumerically is latin too.
        let alphanumeric = ImeStatus {
            native_mode: false,
            ..native
        };
        assert_eq!(alphanumeric.label(), "IME: 微软拼音 · latin");
    }

    #[test]
    fn unnamed_layout_falls_back_without_claiming_a_name() {
        let status = ImeStatus {
            available: true,
            open: true,
            native_mode: true,
            ..ImeStatus::default()
        };
        assert_eq!(status.label(), "IME: keyboard · native");
    }

    #[test]
    fn full_shape_is_appended_only_when_set() {
        let status = ImeStatus {
            name: "微软拼音".to_owned(),
            available: true,
            open: true,
            native_mode: true,
            full_shape: true,
        };
        assert_eq!(status.label(), "IME: 微软拼音 · native · full-width");
    }
}
