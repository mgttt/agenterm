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
