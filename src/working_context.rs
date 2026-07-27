use std::path::Path;

use anyhow::{Result, bail};

pub(crate) const OSC7_MAX_BYTES: usize = 4096;
pub(crate) const CWD_MAX_CHARS: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CwdSource {
    Launch,
    Osc7,
    UserRequested,
    Unknown,
}

impl CwdSource {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Launch => "launch",
            Self::Osc7 => "osc7",
            Self::UserRequested => "user_requested",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ShellKind {
    Cmd,
    PowerShell,
    Bash,
    Unknown,
}

impl ShellKind {
    pub(crate) fn from_program(program: &str) -> Self {
        let name = Path::new(program)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(program)
            .to_ascii_lowercase();
        match name.as_str() {
            "cmd" | "cmd.exe" => Self::Cmd,
            "powershell" | "powershell.exe" | "pwsh" | "pwsh.exe" => Self::PowerShell,
            "bash" | "bash.exe" | "sh" | "sh.exe" | "zsh" | "zsh.exe" => Self::Bash,
            _ => Self::Unknown,
        }
    }

    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Cmd => "cmd",
            Self::PowerShell => "powershell",
            Self::Bash => "bash",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CwdTracker {
    path: Option<String>,
    confirmed_path: Option<String>,
    source: CwdSource,
    pending: bool,
}

impl CwdTracker {
    pub(crate) fn launch(path: Option<String>) -> Self {
        match path.filter(|path| valid_display_path(path)) {
            Some(path) => Self {
                confirmed_path: Some(path.clone()),
                path: Some(path),
                source: CwdSource::Launch,
                pending: false,
            },
            None => Self::unknown(),
        }
    }

    pub(crate) const fn unknown() -> Self {
        Self {
            path: None,
            confirmed_path: None,
            source: CwdSource::Unknown,
            pending: false,
        }
    }

    pub(crate) fn path(&self) -> Option<&str> {
        self.path.as_deref()
    }

    pub(crate) fn confirmed_path(&self) -> Option<&str> {
        self.confirmed_path.as_deref()
    }

    pub(crate) const fn source(&self) -> CwdSource {
        self.source
    }

    pub(crate) const fn pending(&self) -> bool {
        self.pending
    }

    pub(crate) fn request(&mut self, path: String) -> Result<()> {
        validate_path(&path)?;
        self.path = Some(path);
        self.source = CwdSource::UserRequested;
        self.pending = true;
        Ok(())
    }

    pub(crate) fn confirm_osc7(&mut self, path: String) {
        debug_assert!(valid_display_path(&path));
        self.path = Some(path.clone());
        self.confirmed_path = Some(path);
        self.source = CwdSource::Osc7;
        self.pending = false;
    }
}

pub(crate) fn validate_path(path: &str) -> Result<()> {
    if path.is_empty() {
        bail!("working directory cannot be empty");
    }
    if path.chars().count() > CWD_MAX_CHARS {
        bail!("working directory exceeds {CWD_MAX_CHARS} characters");
    }
    if !valid_display_path(path) {
        bail!("working directory contains a control character");
    }
    Ok(())
}

fn valid_display_path(path: &str) -> bool {
    !path.is_empty() && path.chars().count() <= CWD_MAX_CHARS && !path.chars().any(char::is_control)
}

pub(crate) fn cwd_command(shell: ShellKind, path: &str) -> Result<String> {
    validate_path(path)?;
    match shell {
        ShellKind::Cmd => {
            // `%` and `!` expand even inside cmd.exe quotes, and an embedded
            // quote can escape the quoted argument. Refusing those bytes is
            // safer than pretending cmd has a universal literal quoting form.
            if path.contains(['"', '%', '!']) {
                bail!("cmd working directory contains a character that cannot be quoted safely");
            }
            Ok(format!("cd /d \"{path}\""))
        }
        ShellKind::PowerShell => Ok(format!(
            "Set-Location -LiteralPath '{}'",
            path.replace('\'', "''")
        )),
        ShellKind::Bash => Ok(format!("cd -- '{}'", path.replace('\'', "'\\''"))),
        ShellKind::Unknown => bail!("the active shell is unknown"),
    }
}

pub(crate) fn parse_osc7(params: &[&[u8]], local_hostname: Option<&str>) -> Option<String> {
    if params.len() != 2 || params[0] != b"7" || params[1].len() > OSC7_MAX_BYTES {
        return None;
    }
    parse_file_uri(params[1], local_hostname)
}

fn parse_file_uri(uri: &[u8], local_hostname: Option<&str>) -> Option<String> {
    if uri.contains(&b'?') || uri.contains(&b'#') {
        return None;
    }
    let rest = uri.strip_prefix(b"file://")?;
    if rest.iter().any(|byte| *byte >= 0x80) {
        return None;
    }
    let slash = rest.iter().position(|byte| *byte == b'/')?;
    let authority = std::str::from_utf8(&rest[..slash]).ok()?;
    if !authority
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
    {
        return None;
    }
    let local = authority.is_empty()
        || authority.eq_ignore_ascii_case("localhost")
        || local_hostname.is_some_and(|host| authority.eq_ignore_ascii_case(host));
    if !local {
        return None;
    }
    let decoded = percent_decode(&rest[slash..])?;
    let mut path = String::from_utf8(decoded).ok()?;
    if path.len() >= 3
        && path.as_bytes()[0] == b'/'
        && path.as_bytes()[1].is_ascii_alphabetic()
        && path.as_bytes()[2] == b':'
    {
        path.remove(0);
        path = path.replace('/', "\\");
    }
    valid_display_path(&path).then_some(path)
}

fn percent_decode(input: &[u8]) -> Option<Vec<u8>> {
    let mut output = Vec::with_capacity(input.len());
    let mut position = 0;
    while position < input.len() {
        if input[position] == b'%' {
            let high = *input.get(position + 1)?;
            let low = *input.get(position + 2)?;
            output.push(hex(high)? << 4 | hex(low)?);
            position += 3;
        } else {
            output.push(input[position]);
            position += 1;
        }
    }
    (!output.iter().any(|byte| byte.is_ascii_control())).then_some(output)
}

fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_detection_uses_the_executable_name() {
        assert_eq!(
            ShellKind::from_program(r"C:\Windows\System32\cmd.exe"),
            ShellKind::Cmd
        );
        assert_eq!(ShellKind::from_program("pwsh.exe"), ShellKind::PowerShell);
        assert_eq!(ShellKind::from_program("/usr/bin/bash"), ShellKind::Bash);
        assert_eq!(ShellKind::from_program("nu.exe"), ShellKind::Unknown);
    }

    #[test]
    fn quotes_cmd_powershell_and_bash_without_command_injection() {
        assert_eq!(
            cwd_command(ShellKind::Cmd, r"C:\safe folder").unwrap(),
            r#"cd /d "C:\safe folder""#
        );
        assert_eq!(
            cwd_command(ShellKind::PowerShell, r"C:\O'Brien").unwrap(),
            r#"Set-Location -LiteralPath 'C:\O''Brien'"#
        );
        assert_eq!(
            cwd_command(ShellKind::Bash, "/tmp/O'Brien").unwrap(),
            r#"cd -- '/tmp/O'\''Brien'"#
        );
    }

    #[test]
    fn rejects_cmd_expansion_and_control_character_attacks() {
        for path in [r"C:\%TEMP%", r"C:\wow!", "C:\\bad\" & whoami"] {
            assert!(cwd_command(ShellKind::Cmd, path).is_err(), "{path}");
        }
        for shell in [ShellKind::Cmd, ShellKind::PowerShell, ShellKind::Bash] {
            assert!(cwd_command(shell, "safe\r\nwhoami").is_err());
        }
        assert!(cwd_command(ShellKind::Unknown, r"C:\safe").is_err());
    }

    #[test]
    fn parses_local_osc7_file_uris_and_decodes_utf8() {
        assert_eq!(
            parse_osc7(&[b"7", b"file:///C:/work/a%20b"], None).as_deref(),
            Some(r"C:\work\a b")
        );
        assert_eq!(
            parse_osc7(
                &[b"7", b"file://buildbox/C:/src/%E4%B8%AD%E6%96%87"],
                Some("BUILDBOX")
            )
            .as_deref(),
            Some(r"C:\src\中文")
        );
        assert_eq!(
            parse_osc7(&[b"7", b"file:///home/user/project"], None).as_deref(),
            Some("/home/user/project")
        );
    }

    #[test]
    fn rejects_remote_malformed_oversized_and_control_osc7() {
        assert_eq!(parse_osc7(&[b"7", b"https://localhost/C:/x"], None), None);
        assert_eq!(
            parse_osc7(&[b"7", b"file://other/C:/x"], Some("local")),
            None
        );
        assert_eq!(parse_osc7(&[b"7", b"file:///C:/bad%0dname"], None), None);
        assert_eq!(parse_osc7(&[b"7", b"file:///C:/bad%ZZ"], None), None);
        assert_eq!(parse_osc7(&[b"7", b"file:///C:/work?secret"], None), None);
        assert_eq!(parse_osc7(&[b"7", b"file:///C:/work#fragment"], None), None);
        assert_eq!(parse_osc7(&[b"7"], None), None);
        let oversized = vec![b'x'; OSC7_MAX_BYTES + 1];
        assert_eq!(parse_osc7(&[b"7", &oversized], None), None);
    }

    #[test]
    fn requested_cwd_is_explicitly_pending_until_osc7_confirms() {
        let mut tracker = CwdTracker::launch(Some(r"C:\launch".into()));
        tracker.request(r"C:\requested".into()).unwrap();
        assert_eq!(tracker.path(), Some(r"C:\requested"));
        assert_eq!(tracker.confirmed_path(), Some(r"C:\launch"));
        assert_eq!(tracker.source(), CwdSource::UserRequested);
        assert!(tracker.pending());

        tracker.confirm_osc7(r"C:\actual".into());
        assert_eq!(tracker.path(), Some(r"C:\actual"));
        assert_eq!(tracker.confirmed_path(), Some(r"C:\actual"));
        assert_eq!(tracker.source(), CwdSource::Osc7);
        assert!(!tracker.pending());
    }
}
