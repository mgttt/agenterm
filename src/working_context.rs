use std::{env, path::Path};

use anyhow::{Result, bail};

pub(crate) const OSC7_MAX_BYTES: usize = 4096;
pub(crate) const CWD_MAX_CHARS: usize = 4096;
pub(crate) const PROXY_MAX_BYTES: usize = 8192;
const PROXY_CONFIRMATION_PREFIX: &str = "__AGENTERM_PROXY_APPLIED_";
const PROXY_CONFIRMATION_SUFFIX: &str = "__";
const PROXY_CONFIRMATION_NONCE_BYTES: usize = 32;

pub(crate) struct SecretValue(String);

impl SecretValue {
    pub(crate) fn new(value: String) -> Result<Self> {
        if value.len() > PROXY_MAX_BYTES {
            bail!("proxy value exceeds {PROXY_MAX_BYTES} bytes");
        }
        if value.contains('\0')
            || value
                .chars()
                .any(|character| matches!(character, '\r' | '\n'))
        {
            bail!("proxy value contains a forbidden control character");
        }
        Ok(Self(value))
    }

    pub(crate) fn expose(&self) -> &str {
        &self.0
    }

    pub(crate) fn expose_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }
}

impl Drop for SecretValue {
    fn drop(&mut self) {
        // This cannot erase copies made by Windows or the child process, but it
        // prevents AgenTerm's owned heap buffer from retaining the value after
        // the tab or sensitive draft is discarded.
        unsafe {
            self.0.as_mut_vec().fill(0);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ProxySource {
    Launch,
    UserRequested,
    Off,
}

impl ProxySource {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Launch => "launch",
            Self::UserRequested => "user_requested",
            Self::Off => "off",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ProxyApplicationState {
    Off,
    LaunchApplied,
    Prepared,
    Submitted,
    Applied,
    Failed,
}

impl ProxyApplicationState {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::LaunchApplied => "launch_applied",
            Self::Prepared => "prepared",
            Self::Submitted => "submitted",
            Self::Applied => "applied",
            Self::Failed => "failed",
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::LaunchApplied => "Launch applied",
            Self::Prepared => "Prepared",
            Self::Submitted => "Submitted",
            Self::Applied => "Applied",
            Self::Failed => "Failed",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ProxyFacts {
    pub(crate) configured: bool,
    pub(crate) source: ProxySource,
    pub(crate) application_state: ProxyApplicationState,
    pub(crate) request_pending: bool,
}

pub(crate) struct ProxyState {
    http: Option<SecretValue>,
    https: Option<SecretValue>,
    source: ProxySource,
    application_state: ProxyApplicationState,
}

impl ProxyState {
    pub(crate) fn from_environment(environment: &[(String, String)]) -> Result<Self> {
        let find = |name: &str| {
            environment
                .iter()
                .rev()
                .find(|(candidate, _)| candidate.eq_ignore_ascii_case(name))
                .map(|(_, value)| value.clone())
                .or_else(|| {
                    env::vars()
                        .find(|(candidate, _)| candidate.eq_ignore_ascii_case(name))
                        .map(|(_, value)| value)
                })
        };
        Self::new(
            find("HTTP_PROXY"),
            find("HTTPS_PROXY"),
            ProxySource::Launch,
            ProxyApplicationState::LaunchApplied,
        )
    }

    fn new(
        http: Option<String>,
        https: Option<String>,
        source: ProxySource,
        application_state: ProxyApplicationState,
    ) -> Result<Self> {
        for value in [http.as_deref(), https.as_deref()].into_iter().flatten() {
            if !value.is_empty() && parse_proxy_url(value).is_none() {
                bail!("proxy URL must be a valid http:// or https:// URL");
            }
        }
        let http = http
            .filter(|value| !value.is_empty())
            .map(SecretValue::new)
            .transpose()?;
        let https = https
            .filter(|value| !value.is_empty())
            .map(SecretValue::new)
            .transpose()?;
        let has_values = http.is_some() || https.is_some();
        let (source, application_state) =
            if !has_values && application_state == ProxyApplicationState::LaunchApplied {
                (ProxySource::Off, ProxyApplicationState::Off)
            } else {
                (source, application_state)
            };
        Ok(Self {
            http,
            https,
            source,
            application_state,
        })
    }

    pub(crate) fn requested(http: Option<String>, https: Option<String>) -> Result<Self> {
        Self::new(
            http,
            https,
            ProxySource::UserRequested,
            ProxyApplicationState::Prepared,
        )
    }

    /// Reports whether a proxy is known to be active, rather than merely
    /// prepared or submitted to a shell.
    pub(crate) fn configured(&self) -> bool {
        self.has_values()
            && matches!(
                self.application_state,
                ProxyApplicationState::LaunchApplied | ProxyApplicationState::Applied
            )
    }

    pub(crate) const fn source(&self) -> ProxySource {
        self.source
    }

    pub(crate) const fn request_pending(&self) -> bool {
        matches!(
            self.application_state,
            ProxyApplicationState::Prepared | ProxyApplicationState::Submitted
        )
    }

    pub(crate) const fn application_state(&self) -> ProxyApplicationState {
        self.application_state
    }

    pub(crate) fn facts(&self) -> ProxyFacts {
        ProxyFacts {
            configured: self.configured(),
            source: self.source,
            application_state: self.application_state,
            request_pending: self.request_pending(),
        }
    }

    pub(crate) fn mark_submitted(&mut self) -> Result<()> {
        self.transition(
            ProxyApplicationState::Prepared,
            ProxyApplicationState::Submitted,
        )
    }

    pub(crate) fn mark_applied(&mut self) -> Result<()> {
        self.transition(
            ProxyApplicationState::Submitted,
            ProxyApplicationState::Applied,
        )
    }

    pub(crate) fn mark_failed(&mut self) -> Result<()> {
        match self.application_state {
            ProxyApplicationState::Prepared | ProxyApplicationState::Submitted => {
                self.application_state = ProxyApplicationState::Failed;
                Ok(())
            }
            state => bail!(
                "proxy transition from {} to failed is invalid",
                state.as_str()
            ),
        }
    }

    pub(crate) fn http(&self) -> Option<&str> {
        self.http.as_ref().map(SecretValue::expose)
    }

    pub(crate) fn https(&self) -> Option<&str> {
        self.https.as_ref().map(SecretValue::expose)
    }

    pub(crate) fn sanitized_label(&self) -> String {
        let mut labels = Vec::new();
        if let Some(endpoint) = self.http().and_then(parse_proxy_url) {
            labels.push(format!("H {}", endpoint.display()));
        }
        if let Some(endpoint) = self.https().and_then(parse_proxy_url) {
            labels.push(format!("S {}", endpoint.display()));
        }
        if self.application_state == ProxyApplicationState::Off {
            return "Proxy · Off".to_owned();
        }
        if labels.is_empty() {
            labels.push("Clear".to_owned());
        }
        format!(
            "Proxy · {} · {}",
            self.application_state.label(),
            labels.join(" · ")
        )
    }

    pub(crate) fn compact_label(&self) -> String {
        format!("Proxy · {}", self.application_state.label())
    }

    pub(crate) fn editor_text(&self) -> String {
        format!(
            "HTTP_PROXY={}\r\nHTTPS_PROXY={}",
            self.http().unwrap_or(""),
            self.https().unwrap_or("")
        )
    }

    fn has_values(&self) -> bool {
        self.http.is_some() || self.https.is_some()
    }

    fn transition(
        &mut self,
        expected: ProxyApplicationState,
        next: ProxyApplicationState,
    ) -> Result<()> {
        if self.application_state != expected {
            bail!(
                "proxy transition from {} to {} is invalid",
                self.application_state.as_str(),
                next.as_str()
            );
        }
        self.application_state = next;
        Ok(())
    }
}

pub(crate) struct ProxyEndpoint {
    scheme: &'static str,
    host: String,
    port: u16,
}

impl ProxyEndpoint {
    pub(crate) fn display(&self) -> String {
        format!("{}://{}:{}", self.scheme, self.host, self.port)
    }
}

pub(crate) fn parse_proxy_url(value: &str) -> Option<ProxyEndpoint> {
    if value.is_empty()
        || value.len() > PROXY_MAX_BYTES
        || value.chars().any(char::is_control)
        || value.chars().any(char::is_whitespace)
    {
        return None;
    }
    let (raw_scheme, rest) = value.split_once("://")?;
    let (scheme, default_port) = if raw_scheme.eq_ignore_ascii_case("http") {
        ("http", 80)
    } else if raw_scheme.eq_ignore_ascii_case("https") {
        ("https", 443)
    } else {
        return None;
    };
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    if authority.is_empty() {
        return None;
    }
    let host_port = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    let (host, port) = if let Some(bracketed) = host_port.strip_prefix('[') {
        let close = bracketed.find(']')?;
        let host_body = &bracketed[..close];
        if host_body.is_empty()
            || !host_body
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() || matches!(byte, b':' | b'.' | b'%'))
        {
            return None;
        }
        let suffix = &bracketed[close + 1..];
        let port = if suffix.is_empty() {
            default_port
        } else {
            suffix
                .strip_prefix(':')?
                .parse::<u16>()
                .ok()
                .filter(|port| *port != 0)?
        };
        (format!("[{host_body}]"), port)
    } else {
        if host_port.matches(':').count() > 1 {
            return None;
        }
        let (host, port) =
            host_port
                .rsplit_once(':')
                .map_or((host_port, default_port), |(host, port)| {
                    (
                        host,
                        port.parse::<u16>()
                            .ok()
                            .filter(|port| *port != 0)
                            .unwrap_or(0),
                    )
                });
        if port == 0
            || host.is_empty()
            || !host
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
        {
            return None;
        }
        (host.to_owned(), port)
    };
    Some(ProxyEndpoint { scheme, host, port })
}

pub(crate) fn parse_proxy_editor(text: &str) -> Result<(Option<String>, Option<String>)> {
    let mut http = None;
    let mut https = None;
    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        if line.is_empty() {
            continue;
        }
        let (name, value) = line
            .split_once('=')
            .ok_or_else(|| anyhow::anyhow!("proxy editor requires NAME=value lines"))?;
        match name.trim() {
            name if name.eq_ignore_ascii_case("HTTP_PROXY") => http = Some(value.to_owned()),
            name if name.eq_ignore_ascii_case("HTTPS_PROXY") => https = Some(value.to_owned()),
            _ => bail!("proxy editor accepts only HTTP_PROXY and HTTPS_PROXY"),
        }
    }
    for value in [http.as_deref(), https.as_deref()].into_iter().flatten() {
        SecretValue::new(value.to_owned())?;
        if !value.is_empty() && parse_proxy_url(value).is_none() {
            bail!("proxy URL must be a valid http:// or https:// URL");
        }
    }
    Ok((
        http.filter(|value| !value.is_empty()),
        https.filter(|value| !value.is_empty()),
    ))
}

#[cfg(test)]
pub(crate) fn proxy_command(
    shell: ShellKind,
    http: Option<&str>,
    https: Option<&str>,
) -> Result<SecretValue> {
    let command = match shell {
        ShellKind::Cmd => {
            for value in [http, https].into_iter().flatten() {
                if value.contains(['"', '%', '!']) {
                    bail!("cmd proxy value contains a character that cannot be quoted safely");
                }
            }
            format!(
                "set \"HTTP_PROXY={}\" & set \"HTTPS_PROXY={}\"",
                http.unwrap_or(""),
                https.unwrap_or("")
            )
        }
        ShellKind::PowerShell => format!(
            "$env:HTTP_PROXY = '{}'; $env:HTTPS_PROXY = '{}'",
            http.unwrap_or("").replace('\'', "''"),
            https.unwrap_or("").replace('\'', "''")
        ),
        ShellKind::Bash => format!(
            "export HTTP_PROXY='{}' HTTPS_PROXY='{}'",
            http.unwrap_or("").replace('\'', "'\\''"),
            https.unwrap_or("").replace('\'', "'\\''")
        ),
        ShellKind::Unknown => bail!("the active shell is unknown"),
    };
    SecretValue::new(command)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ProxyConfirmationMarker(String);

impl ProxyConfirmationMarker {
    pub(crate) fn from_nonce(nonce: &str) -> Result<Self> {
        if nonce.len() != PROXY_CONFIRMATION_NONCE_BYTES
            || !nonce
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        {
            bail!("proxy confirmation nonce must be exactly 32 lowercase hexadecimal bytes");
        }
        Ok(Self(format!(
            "{PROXY_CONFIRMATION_PREFIX}{nonce}{PROXY_CONFIRMATION_SUFFIX}"
        )))
    }

    #[cfg(test)]
    pub(crate) fn parse(marker: &str) -> Result<Self> {
        let nonce = marker
            .strip_prefix(PROXY_CONFIRMATION_PREFIX)
            .and_then(|value| value.strip_suffix(PROXY_CONFIRMATION_SUFFIX))
            .ok_or_else(|| anyhow::anyhow!("proxy confirmation marker has an invalid format"))?;
        let parsed = Self::from_nonce(nonce)?;
        if parsed.0 != marker {
            bail!("proxy confirmation marker has an invalid format");
        }
        Ok(parsed)
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

pub(crate) struct ProxyCommandPlan {
    command: SecretValue,
    marker: ProxyConfirmationMarker,
}

impl ProxyCommandPlan {
    #[cfg(test)]
    pub(crate) fn command(&self) -> &SecretValue {
        &self.command
    }

    pub(crate) fn marker(&self) -> &ProxyConfirmationMarker {
        &self.marker
    }

    pub(crate) fn into_command(self) -> SecretValue {
        self.command
    }
}

/// Builds a shell command whose marker is written only after every requested
/// environment change succeeds. The marker is caller-supplied, nonsecret, and
/// deliberately independent from the proxy values.
pub(crate) fn proxy_command_with_confirmation(
    shell: ShellKind,
    http: Option<&str>,
    https: Option<&str>,
    marker: ProxyConfirmationMarker,
) -> Result<ProxyCommandPlan> {
    for value in [http, https].into_iter().flatten() {
        if parse_proxy_url(value).is_none() {
            bail!("proxy URL must be a valid http:// or https:// URL");
        }
        if value.contains(marker.as_str()) || marker.as_str().contains(value) {
            bail!("proxy confirmation marker overlaps a proxy value");
        }
    }

    let command = match shell {
        ShellKind::Cmd => {
            for value in [http, https].into_iter().flatten() {
                if value.contains(['"', '%', '!']) {
                    bail!("cmd proxy value contains a character that cannot be quoted safely");
                }
            }
            let nonce = &marker.as_str()[PROXY_CONFIRMATION_PREFIX.len()
                ..marker.as_str().len() - PROXY_CONFIRMATION_SUFFIX.len()];
            format!(
                "set \"HTTP_PROXY={}\" && set \"HTTPS_PROXY={}\" && echo {}^{}{}",
                http.unwrap_or(""),
                https.unwrap_or(""),
                PROXY_CONFIRMATION_PREFIX,
                nonce,
                PROXY_CONFIRMATION_SUFFIX,
            )
        }
        ShellKind::PowerShell => {
            let assignment = |name: &str, value: Option<&str>| {
                let value = value.unwrap_or("").replace('\'', "''");
                format!("[Environment]::SetEnvironmentVariable('{name}', '{value}', 'Process')")
            };
            format!(
                "& {{ $ErrorActionPreference = 'Stop'; {}; {}; \
                 [Console]::WriteLine('{}' + '{}' + '{}') }}",
                assignment("HTTP_PROXY", http),
                assignment("HTTPS_PROXY", https),
                PROXY_CONFIRMATION_PREFIX,
                &marker.as_str()[PROXY_CONFIRMATION_PREFIX.len()
                    ..marker.as_str().len() - PROXY_CONFIRMATION_SUFFIX.len()],
                PROXY_CONFIRMATION_SUFFIX,
            )
        }
        ShellKind::Bash => {
            let change = |upper: &str, lower: &str, value: Option<&str>| match value {
                Some(value) => {
                    let value = value.replace('\'', "'\\''");
                    format!("export {upper}='{value}' {lower}='{value}'")
                }
                None => format!("unset {upper} {lower}"),
            };
            format!(
                "{} && {} && printf '%s%s%s\\n' '{}' '{}' '{}'",
                change("HTTP_PROXY", "http_proxy", http),
                change("HTTPS_PROXY", "https_proxy", https),
                PROXY_CONFIRMATION_PREFIX,
                &marker.as_str()[PROXY_CONFIRMATION_PREFIX.len()
                    ..marker.as_str().len() - PROXY_CONFIRMATION_SUFFIX.len()],
                PROXY_CONFIRMATION_SUFFIX,
            )
        }
        ShellKind::Unknown => bail!("the active shell is unknown"),
    };

    Ok(ProxyCommandPlan {
        command: SecretValue::new(command)?,
        marker,
    })
}

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
        assert_eq!(ShellKind::from_program("/usr/bin/bash"), ShellKind::Bash);
        assert_eq!(ShellKind::from_program("nu.exe"), ShellKind::Unknown);
        #[cfg(windows)]
        {
            assert_eq!(
                ShellKind::from_program(r"C:\Windows\System32\cmd.exe"),
                ShellKind::Cmd
            );
            assert_eq!(ShellKind::from_program("pwsh.exe"), ShellKind::PowerShell);
        }
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

    #[test]
    fn proxy_parser_redacts_everything_except_scheme_host_and_port() {
        let parsed = parse_proxy_url(
            "https://alice:super-secret@proxy.example:8443/private?token=hidden#fragment",
        )
        .unwrap();
        assert_eq!(parsed.display(), "https://proxy.example:8443");
        let ipv6 = parse_proxy_url("http://user:pass@[2001:db8::1]/ignored").unwrap();
        assert_eq!(ipv6.display(), "http://[2001:db8::1]:80");
        for invalid in [
            "socks5://proxy.example:1080",
            "http://",
            "http://2001:db8::1",
            "http://proxy.example:0",
            "http://proxy.example:70000",
            "http://proxy example",
        ] {
            assert!(parse_proxy_url(invalid).is_none(), "{invalid}");
        }
    }

    #[test]
    fn proxy_environment_lookup_is_case_insensitive_and_snapshot_safe_by_type() {
        let state = ProxyState::from_environment(&[
            ("http_proxy".into(), "http://one.example".into()),
            (
                "HTTPS_PROXY".into(),
                "https://user:secret@two.example:9443/query?secret".into(),
            ),
        ])
        .unwrap();
        assert!(state.configured());
        assert_eq!(state.source(), ProxySource::Launch);
        assert!(!state.request_pending());
        assert_eq!(
            state.application_state(),
            ProxyApplicationState::LaunchApplied
        );
        assert_eq!(
            state.sanitized_label(),
            "Proxy · Launch applied · H http://one.example:80 · S https://two.example:9443"
        );
    }

    #[test]
    fn proxy_state_distinguishes_prepared_submitted_applied_and_failed() {
        let mut state =
            ProxyState::requested(Some("http://proxy.example:8080".into()), None).unwrap();
        assert!(!state.configured());
        assert!(state.request_pending());
        assert_eq!(state.application_state(), ProxyApplicationState::Prepared);
        assert_eq!(
            state.sanitized_label(),
            "Proxy · Prepared · H http://proxy.example:8080"
        );
        assert_eq!(
            state.facts(),
            ProxyFacts {
                configured: false,
                source: ProxySource::UserRequested,
                application_state: ProxyApplicationState::Prepared,
                request_pending: true,
            }
        );

        state.mark_submitted().unwrap();
        assert_eq!(state.application_state(), ProxyApplicationState::Submitted);
        assert!(state.mark_submitted().is_err());
        state.mark_applied().unwrap();
        assert!(state.configured());
        assert!(!state.request_pending());
        assert_eq!(state.application_state(), ProxyApplicationState::Applied);
        assert!(state.mark_applied().is_err());

        let mut failed =
            ProxyState::requested(None, Some("https://secure.example".into())).unwrap();
        failed.mark_submitted().unwrap();
        failed.mark_failed().unwrap();
        assert_eq!(failed.application_state(), ProxyApplicationState::Failed);
        assert!(!failed.configured());
        assert!(!failed.request_pending());
        assert!(failed.mark_failed().is_err());
    }

    #[test]
    fn proxy_clear_request_remains_observable_until_confirmation() {
        let mut state = ProxyState::requested(None, None).unwrap();
        assert_eq!(state.source(), ProxySource::UserRequested);
        assert_eq!(state.application_state(), ProxyApplicationState::Prepared);
        assert_eq!(state.sanitized_label(), "Proxy · Prepared · Clear");
        state.mark_submitted().unwrap();
        state.mark_applied().unwrap();
        assert_eq!(state.sanitized_label(), "Proxy · Applied · Clear");
        assert!(!state.configured());
    }

    #[test]
    fn proxy_editor_accepts_only_two_strict_http_values() {
        let (http, https) = parse_proxy_editor(
            "HTTP_PROXY=http://proxy.example:8080\r\n\
             HTTPS_PROXY=https://user:pass@secure.example/path?q=x",
        )
        .unwrap();
        assert_eq!(http.as_deref(), Some("http://proxy.example:8080"));
        assert!(https.as_deref().unwrap().contains("user:pass"));
        assert!(parse_proxy_editor("NO_PROXY=localhost").is_err());
        assert!(parse_proxy_editor("HTTP_PROXY=socks5://proxy").is_err());
        assert!(parse_proxy_editor("HTTP_PROXY=http://proxy\r\ninjected").is_err());
        assert_eq!(
            parse_proxy_editor("HTTP_PROXY=\r\nHTTPS_PROXY=").unwrap(),
            (None, None)
        );
    }

    #[test]
    fn proxy_commands_quote_known_shells_and_reject_unsafe_cmd_expansion() {
        assert_eq!(
            proxy_command(ShellKind::PowerShell, Some("http://user:p'ass@host"), None)
                .unwrap()
                .expose(),
            "$env:HTTP_PROXY = 'http://user:p''ass@host'; $env:HTTPS_PROXY = ''"
        );
        assert_eq!(
            proxy_command(ShellKind::Bash, Some("http://host/a'b"), None)
                .unwrap()
                .expose(),
            "export HTTP_PROXY='http://host/a'\\''b' HTTPS_PROXY=''"
        );
        assert!(proxy_command(ShellKind::Cmd, Some("http://host/%USER%"), None).is_err());
        assert!(proxy_command(ShellKind::Unknown, None, None).is_err());
    }

    #[test]
    fn confirmed_proxy_commands_emit_a_strict_marker_after_success() {
        let marker =
            ProxyConfirmationMarker::from_nonce("0123456789abcdef0123456789abcdef").unwrap();
        let marker_text = marker.as_str().to_owned();

        let cmd = proxy_command_with_confirmation(
            ShellKind::Cmd,
            Some("http://proxy.example:8080"),
            None,
            marker.clone(),
        )
        .unwrap();
        assert_eq!(cmd.marker().as_str(), marker_text);
        assert_eq!(
            cmd.command().expose(),
            "set \"HTTP_PROXY=http://proxy.example:8080\" && set \"HTTPS_PROXY=\" && \
             echo __AGENTERM_PROXY_APPLIED_^0123456789abcdef0123456789abcdef__"
        );
        assert!(!cmd.command().expose().contains(&marker_text));

        let powershell = proxy_command_with_confirmation(
            ShellKind::PowerShell,
            Some("http://user:p'ass@proxy.example"),
            None,
            marker.clone(),
        )
        .unwrap()
        .into_command();
        assert!(
            powershell
                .expose()
                .contains("$ErrorActionPreference = 'Stop'")
        );
        assert!(powershell.expose().ends_with(
            "[Console]::WriteLine('__AGENTERM_PROXY_APPLIED_' + \
             '0123456789abcdef0123456789abcdef' + '__') }"
        ));
        assert!(!powershell.expose().contains(&marker_text));

        let bash = proxy_command_with_confirmation(
            ShellKind::Bash,
            Some("http://proxy.example"),
            Some("https://secure.example"),
            marker,
        )
        .unwrap()
        .into_command();
        assert!(bash.expose().contains(
            "export HTTP_PROXY='http://proxy.example' http_proxy='http://proxy.example'"
        ));
        assert!(bash.expose().contains(
            "export HTTPS_PROXY='https://secure.example' https_proxy='https://secure.example'"
        ));
        assert!(bash.expose().ends_with(
            "&& printf '%s%s%s\\n' '__AGENTERM_PROXY_APPLIED_' \
             '0123456789abcdef0123456789abcdef' '__'"
        ));
        assert!(!bash.expose().contains(&marker_text));
    }

    #[test]
    fn confirmed_proxy_clear_unsets_all_bash_proxy_variables() {
        let marker =
            ProxyConfirmationMarker::from_nonce("abcdef0123456789abcdef0123456789").unwrap();
        let plan = proxy_command_with_confirmation(ShellKind::Bash, None, None, marker).unwrap();
        assert!(
            plan.command().expose().starts_with(
                "unset HTTP_PROXY http_proxy && unset HTTPS_PROXY https_proxy && printf"
            )
        );
    }

    #[test]
    fn proxy_confirmation_rejects_marker_and_command_injection_without_leaking_values() {
        for invalid in [
            "0123456789abcdef0123456789abcdeF",
            "0123456789abcdef0123456789abcde",
            "0123456789abcdef&whoami;2345678",
        ] {
            let error = ProxyConfirmationMarker::from_nonce(invalid)
                .unwrap_err()
                .to_string();
            assert!(!error.contains(invalid));
        }
        assert!(
            ProxyConfirmationMarker::parse(
                "__AGENTERM_PROXY_APPLIED_0123456789abcdef0123456789abcdef__ & whoami"
            )
            .is_err()
        );

        let secret = "http://user:do-not-leak@proxy.example/%USER%";
        let marker =
            ProxyConfirmationMarker::from_nonce("0123456789abcdef0123456789abcdef").unwrap();
        let error = proxy_command_with_confirmation(ShellKind::Cmd, Some(secret), None, marker)
            .err()
            .expect("unsafe cmd proxy value must be rejected")
            .to_string();
        assert!(!error.contains("do-not-leak"));
        assert!(!error.contains(secret));
    }

    #[test]
    fn secret_value_zeroes_its_owned_buffer_before_release() {
        let mut secret = SecretValue::new("sentinel-value".to_owned()).unwrap();
        unsafe {
            secret.0.as_mut_vec().fill(0);
        }
        assert!(secret.0.bytes().all(|byte| byte == 0));
    }
}
