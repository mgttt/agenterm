//! New-terminal modal state, validation, and ui-action handlers (Win parity).

use crate::working_context::parse_proxy_url;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum NewShellChoice {
    #[default]
    Default,
    CommandPrompt,
    PowerShell,
}

impl NewShellChoice {
    pub(super) const fn id(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::CommandPrompt => "cmd",
            Self::PowerShell => "powershell",
        }
    }

    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::Default => "Default",
            Self::CommandPrompt => "Command Prompt",
            Self::PowerShell => "PowerShell",
        }
    }

    pub(super) fn from_action_id(action: &str) -> Option<Self> {
        match action {
            "shell-default" => Some(Self::Default),
            "shell-cmd" => Some(Self::CommandPrompt),
            "shell-powershell" => Some(Self::PowerShell),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct CreateParams {
    pub command_line: Vec<String>,
    pub tab_environment: Vec<(String, String)>,
}

#[derive(Clone, Debug, Default)]
pub(super) struct NewTerminalDialog {
    open: bool,
    shell_choice: NewShellChoice,
    initial_command_draft: String,
    http_proxy_draft: String,
    https_proxy_draft: String,
    last_error: Option<String>,
}

impl NewTerminalDialog {
    pub(super) const fn new() -> Self {
        Self {
            open: false,
            shell_choice: NewShellChoice::Default,
            initial_command_draft: String::new(),
            http_proxy_draft: String::new(),
            https_proxy_draft: String::new(),
            last_error: None,
        }
    }

    pub(super) const fn is_open(&self) -> bool {
        self.open
    }

    pub(super) fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    pub(super) fn shell_choice(&self) -> NewShellChoice {
        self.shell_choice
    }

    pub(super) fn initial_command_draft(&self) -> &str {
        &self.initial_command_draft
    }

    pub(super) fn initial_command_draft_mut(&mut self) -> &mut String {
        &mut self.initial_command_draft
    }

    pub(super) fn http_proxy_draft(&self) -> &str {
        &self.http_proxy_draft
    }

    pub(super) fn http_proxy_draft_mut(&mut self) -> &mut String {
        &mut self.http_proxy_draft
    }

    pub(super) fn https_proxy_draft(&self) -> &str {
        &self.https_proxy_draft
    }

    pub(super) fn https_proxy_draft_mut(&mut self) -> &mut String {
        &mut self.https_proxy_draft
    }

    pub(super) fn open(&mut self) {
        self.open = true;
        self.shell_choice = NewShellChoice::Default;
        self.initial_command_draft.clear();
        self.http_proxy_draft.clear();
        self.https_proxy_draft.clear();
        self.last_error = None;
    }

    pub(super) fn cancel(&mut self) {
        if self.open {
            self.close_without_create();
        }
    }

    pub(super) fn finish(&mut self, create: bool) -> Result<Option<CreateParams>, String> {
        if !self.open {
            return Ok(None);
        }
        if !create {
            self.close_without_create();
            return Ok(None);
        }

        let initial = self.initial_command_draft.trim();
        let http_proxy = self.http_proxy_draft.trim();
        let https_proxy = self.https_proxy_draft.trim();

        for (label, value) in [("HTTP proxy", http_proxy), ("HTTPS proxy", https_proxy)] {
            if !value.is_empty() && parse_proxy_url(value).is_none() {
                let message = format!("{label} must be a valid http:// or https:// URL");
                self.last_error = Some(message.clone());
                return Err(message);
            }
        }

        let command_line = build_command_line(self.shell_choice, initial);
        let mut tab_environment = Vec::new();
        for (name, value) in [("HTTP_PROXY", http_proxy), ("HTTPS_PROXY", https_proxy)] {
            if !value.is_empty() {
                tab_environment.push((name.to_owned(), value.to_owned()));
            }
        }

        self.close_without_create();
        Ok(Some(CreateParams {
            command_line,
            tab_environment,
        }))
    }

    pub(super) fn choose_shell(&mut self, choice: NewShellChoice) {
        if self.open {
            self.shell_choice = choice;
        }
    }

    pub(super) fn set_initial_command_draft(&mut self, value: String) {
        if self.open {
            self.initial_command_draft = value;
        }
    }

    pub(super) fn set_http_proxy_draft(&mut self, value: String) {
        if self.open {
            self.http_proxy_draft = value;
        }
    }

    pub(super) fn set_https_proxy_draft(&mut self, value: String) {
        if self.open {
            self.https_proxy_draft = value;
        }
    }

    pub(super) fn snapshot_modal(&self) -> serde_json::Value {
        serde_json::json!({
            "kind": "new-terminal",
            "shell": self.shell_choice.id(),
            "initial_command_configured":
                !self.initial_command_draft.trim().is_empty(),
            "http_proxy_configured": !self.http_proxy_draft.trim().is_empty(),
            "https_proxy_configured": !self.https_proxy_draft.trim().is_empty(),
            "proxy_values_exposed": false,
            "default_action": "create",
            "actions": ["create", "cancel"],
        })
    }

    fn close_without_create(&mut self) {
        self.open = false;
        self.last_error = None;
    }
}

fn build_command_line(choice: NewShellChoice, initial: &str) -> Vec<String> {
    match choice {
        NewShellChoice::Default if initial.is_empty() => Vec::new(),
        NewShellChoice::Default | NewShellChoice::CommandPrompt => {
            let mut child = vec!["/bin/sh".to_owned()];
            if !initial.is_empty() {
                child.extend(["-c".to_owned(), format!("{initial}; exec /bin/sh -i")]);
            }
            child
        }
        NewShellChoice::PowerShell => {
            let mut child = vec!["pwsh".to_owned(), "-NoLogo".to_owned()];
            if !initial.is_empty() {
                child.extend([
                    "-NoExit".to_owned(),
                    "-Command".to_owned(),
                    initial.to_owned(),
                ]);
            }
            child
        }
    }
}

pub(super) fn ui_action_open(dialog: &mut NewTerminalDialog) -> bool {
    if dialog.is_open() {
        return false;
    }
    dialog.open();
    true
}

pub(super) fn ui_action_create(
    dialog: &mut NewTerminalDialog,
) -> Result<Option<CreateParams>, String> {
    if !dialog.is_open() {
        return Ok(None);
    }
    dialog.finish(true)
}

pub(super) fn ui_action_cancel(dialog: &mut NewTerminalDialog) -> bool {
    if !dialog.is_open() {
        return false;
    }
    dialog.cancel();
    true
}

pub(super) fn ui_action_choose_shell(
    dialog: &mut NewTerminalDialog,
    choice: NewShellChoice,
) -> bool {
    if !dialog.is_open() {
        return false;
    }
    dialog.choose_shell(choice);
    true
}

pub(super) fn ui_action_set_initial_command(dialog: &mut NewTerminalDialog, text: &str) -> bool {
    if !dialog.is_open() {
        return false;
    }
    dialog.set_initial_command_draft(text.to_owned());
    true
}

pub(super) fn ui_action_set_http_proxy(dialog: &mut NewTerminalDialog, text: &str) -> bool {
    if !dialog.is_open() {
        return false;
    }
    dialog.set_http_proxy_draft(text.to_owned());
    true
}

pub(super) fn ui_action_set_https_proxy(dialog: &mut NewTerminalDialog, text: &str) -> bool {
    if !dialog.is_open() {
        return false;
    }
    dialog.set_https_proxy_draft(text.to_owned());
    true
}

/// Shared ui-action entry for new-terminal modal actions.
pub(super) fn dispatch_ui_action(
    dialog: &mut NewTerminalDialog,
    action: &str,
    text: Option<&str>,
) -> Result<Option<CreateParams>, String> {
    match action {
        "open-new-terminal" => {
            ui_action_open(dialog);
            Ok(None)
        }
        "create" => ui_action_create(dialog),
        "cancel" if dialog.is_open() => {
            ui_action_cancel(dialog);
            Ok(None)
        }
        "shell-default" => {
            ui_action_choose_shell(dialog, NewShellChoice::Default);
            Ok(None)
        }
        "shell-cmd" => {
            ui_action_choose_shell(dialog, NewShellChoice::CommandPrompt);
            Ok(None)
        }
        "shell-powershell" => {
            ui_action_choose_shell(dialog, NewShellChoice::PowerShell);
            Ok(None)
        }
        "new-terminal-set-initial-command" => {
            let text = text.unwrap_or("");
            ui_action_set_initial_command(dialog, text);
            Ok(None)
        }
        "new-terminal-set-http-proxy" => {
            let text = text.unwrap_or("");
            ui_action_set_http_proxy(dialog, text);
            Ok(None)
        }
        "new-terminal-set-https-proxy" => {
            let text = text.unwrap_or("");
            ui_action_set_https_proxy(dialog, text);
            Ok(None)
        }
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_resets_drafts() {
        let mut dialog = NewTerminalDialog::new();
        dialog.open();
        dialog.set_initial_command_draft("echo hi".to_owned());
        dialog.set_http_proxy_draft("http://proxy.test".to_owned());
        dialog.choose_shell(NewShellChoice::PowerShell);

        dialog.open();
        assert!(dialog.is_open());
        assert_eq!(dialog.shell_choice(), NewShellChoice::Default);
        assert!(dialog.initial_command_draft().is_empty());
        assert!(dialog.http_proxy_draft().is_empty());
        assert!(dialog.last_error().is_none());
    }

    #[test]
    fn finish_create_default_empty_command_line() {
        let mut dialog = NewTerminalDialog::new();
        dialog.open();
        let params = dialog.finish(true).unwrap().expect("create params");
        assert!(params.command_line.is_empty());
        assert!(params.tab_environment.is_empty());
        assert!(!dialog.is_open());
    }

    #[test]
    fn finish_create_cmd_with_initial() {
        let mut dialog = NewTerminalDialog::new();
        dialog.open();
        dialog.choose_shell(NewShellChoice::CommandPrompt);
        dialog.set_initial_command_draft("echo marker".to_owned());
        let params = dialog.finish(true).unwrap().expect("create params");
        assert_eq!(params.command_line[0], "/bin/sh");
        assert_eq!(params.command_line[1], "-c");
        assert!(params.command_line[2].contains("echo marker"));
    }

    #[test]
    fn finish_create_powershell() {
        let mut dialog = NewTerminalDialog::new();
        dialog.open();
        dialog.choose_shell(NewShellChoice::PowerShell);
        dialog.set_initial_command_draft("Write-Host hi".to_owned());
        let params = dialog.finish(true).unwrap().expect("create params");
        assert_eq!(
            params.command_line,
            vec![
                "pwsh".to_owned(),
                "-NoLogo".to_owned(),
                "-NoExit".to_owned(),
                "-Command".to_owned(),
                "Write-Host hi".to_owned(),
            ]
        );
    }

    #[test]
    fn finish_rejects_invalid_proxy() {
        let mut dialog = NewTerminalDialog::new();
        dialog.open();
        dialog.set_http_proxy_draft("not-a-url".to_owned());
        let error = dialog.finish(true).unwrap_err();
        assert!(error.contains("HTTP proxy"));
        assert!(dialog.is_open());
        assert!(dialog.last_error().is_some());
    }

    #[test]
    fn finish_accepts_proxy_environment() {
        let mut dialog = NewTerminalDialog::new();
        dialog.open();
        dialog.set_http_proxy_draft("http://127.0.0.1:8080".to_owned());
        dialog.set_https_proxy_draft("https://127.0.0.1:8443".to_owned());
        let params = dialog.finish(true).unwrap().expect("create params");
        assert_eq!(
            params.tab_environment,
            vec![
                ("HTTP_PROXY".to_owned(), "http://127.0.0.1:8080".to_owned()),
                (
                    "HTTPS_PROXY".to_owned(),
                    "https://127.0.0.1:8443".to_owned(),
                ),
            ]
        );
    }

    #[test]
    fn snapshot_redacts_secrets() {
        let mut dialog = NewTerminalDialog::new();
        dialog.open();
        let secret = "http://user:dialog-pass@127.0.0.1:48888";
        dialog.set_initial_command_draft("echo secret-cmd".to_owned());
        dialog.set_http_proxy_draft(secret.to_owned());
        dialog.set_https_proxy_draft(secret.to_owned());

        let snapshot = dialog.snapshot_modal();
        let json = serde_json::to_string(&snapshot).expect("snapshot json");
        assert_eq!(snapshot["kind"], "new-terminal");
        assert_eq!(snapshot["shell"], "default");
        assert!(snapshot["initial_command_configured"].as_bool() == Some(true));
        assert!(snapshot["http_proxy_configured"].as_bool() == Some(true));
        assert!(snapshot["https_proxy_configured"].as_bool() == Some(true));
        assert_eq!(snapshot["proxy_values_exposed"], false);
        assert!(!json.contains("dialog-pass"));
        assert!(!json.contains(secret));
        assert!(!json.contains("secret-cmd"));
    }

    #[test]
    fn dispatch_ui_action_create_path() {
        let mut dialog = NewTerminalDialog::new();
        dispatch_ui_action(&mut dialog, "open-new-terminal", None).expect("open");
        dispatch_ui_action(
            &mut dialog,
            "new-terminal-set-initial-command",
            Some("echo ok"),
        )
        .expect("set initial");
        let params = dispatch_ui_action(&mut dialog, "create", None)
            .expect("create")
            .expect("params");
        assert_eq!(params.command_line[0], "/bin/sh");
        assert!(!dialog.is_open());
    }

    #[test]
    fn shell_action_ids() {
        assert_eq!(
            NewShellChoice::from_action_id("shell-default"),
            Some(NewShellChoice::Default)
        );
        assert_eq!(
            NewShellChoice::from_action_id("shell-cmd"),
            Some(NewShellChoice::CommandPrompt)
        );
        assert_eq!(
            NewShellChoice::from_action_id("shell-powershell"),
            Some(NewShellChoice::PowerShell)
        );
    }
}
