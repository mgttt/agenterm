//! Server-strip chrome dialogs: new-server name, context menu, close confirm.

#[derive(Clone, Debug, Default)]
pub(crate) struct ServerNewDialog {
    open: bool,
    name: String,
    error: Option<String>,
}

impl ServerNewDialog {
    pub(crate) const fn new() -> Self {
        Self {
            open: false,
            name: String::new(),
            error: None,
        }
    }

    pub(crate) const fn is_open(&self) -> bool {
        self.open
    }

    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    pub(crate) fn set_name(&mut self, name: impl Into<String>) {
        self.name = name.into();
    }

    pub(crate) fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub(crate) fn open(&mut self) {
        self.open = true;
        self.name.clear();
        self.error = None;
    }

    pub(crate) fn close(&mut self) {
        self.open = false;
        self.name.clear();
        self.error = None;
    }

    pub(crate) fn set_error(&mut self, message: impl Into<String>) {
        self.error = Some(message.into());
    }

    /// Normalize user input to a bare instance token (no `custom:` prefix).
    pub(crate) fn take_validated_name(&mut self) -> Result<String, String> {
        let raw = self.name.trim();
        if raw.is_empty() {
            return Err("server name is required".to_owned());
        }
        let bare = raw.strip_prefix("custom:").unwrap_or(raw).trim();
        if bare.is_empty() {
            return Err("server name is required".to_owned());
        }
        if bare.eq_ignore_ascii_case("main") {
            return Err("use a custom name; `main` is the default instance".to_owned());
        }
        if bare.len() > 48 {
            return Err("server name is too long (max 48)".to_owned());
        }
        if !bare
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
        {
            return Err("use letters, digits, '-' or '_' only".to_owned());
        }
        Ok(bare.to_owned())
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ServerTabContextMenu {
    pub instance: String,
    pub endpoint: String,
    pub can_attach: bool,
    /// Menu top-left in client coordinates.
    pub origin_x: i32,
    pub origin_y: i32,
}

#[derive(Clone, Debug)]
pub(crate) struct ServerCloseConfirm {
    pub instance: String,
    pub endpoint: String,
    pub can_attach: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ServerContextAction {
    Close,
    NewWindow,
}
