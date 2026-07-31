use std::time::Duration;
use std::{env, fmt, path::PathBuf, str::FromStr};

#[cfg(unix)]
use std::path::Path;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use sha2::{Digest, Sha256};

const INSTANCE_NAME_LIMIT: usize = 96;
const SCOPE_NAMESPACE: &str = "agenterm.server-scope.v1";

/// The user-facing role of one local server authority.
///
/// Instance names are deliberately separate from native endpoint names.  In
/// particular, custom/unicode labels are hashed before they participate in a
/// [`ServerScopeId`].
#[derive(Clone, Debug, Default, Eq, Hash, PartialEq)]
pub(crate) enum LogicalInstance {
    #[default]
    Main,
    Dev,
    Ephemeral(String),
    Custom(String),
}

impl LogicalInstance {
    pub(crate) fn canonical_name(&self) -> String {
        match self {
            Self::Main => "main".to_owned(),
            Self::Dev => "dev".to_owned(),
            Self::Ephemeral(value) => format!("ephemeral:{value}"),
            Self::Custom(value) => format!("custom:{value}"),
        }
    }

    pub(crate) fn display_label(&self, username: &str) -> String {
        let mut display_username = username
            .chars()
            .map(|character| {
                if character.is_control() {
                    '_'
                } else {
                    character
                }
            })
            .take(64)
            .collect::<String>();
        if display_username
            .chars()
            .all(|character| character == '_' || character.is_whitespace())
        {
            display_username = "user".to_owned();
        }
        format!("{display_username}_{}", self.canonical_name())
    }

    fn named(kind: &'static str, value: &str) -> Result<Self, ParseLogicalInstanceError> {
        let value = value.trim();
        if value.is_empty()
            || value.len() > INSTANCE_NAME_LIMIT
            || value.chars().any(char::is_control)
        {
            return Err(ParseLogicalInstanceError);
        }
        Ok(match kind {
            "ephemeral" => Self::Ephemeral(value.to_owned()),
            "custom" => Self::Custom(value.to_owned()),
            _ => unreachable!("logical instance kind is internal"),
        })
    }
}

impl fmt::Display for LogicalInstance {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.canonical_name())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ParseLogicalInstanceError;

impl fmt::Display for ParseLogicalInstanceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .write_str("logical instance must be main, dev, ephemeral:<name>, or custom:<name>")
    }
}

impl std::error::Error for ParseLogicalInstanceError {}

impl FromStr for LogicalInstance {
    type Err = ParseLogicalInstanceError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "main" => Ok(Self::Main),
            "dev" => Ok(Self::Dev),
            _ => value
                .strip_prefix("ephemeral:")
                .map(|name| Self::named("ephemeral", name))
                .or_else(|| {
                    value
                        .strip_prefix("custom:")
                        .map(|name| Self::named("custom", name))
                })
                .unwrap_or(Err(ParseLogicalInstanceError)),
        }
    }
}

impl Serialize for LogicalInstance {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.canonical_name())
    }
}

impl<'de> Deserialize<'de> for LogicalInstance {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(de::Error::custom)
    }
}

/// A transport-qualified IPC address.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) enum IpcEndpoint {
    UnixSocket(String),
    NamedPipe(String),
    Tcp { host: String, port: u16 },
}

impl IpcEndpoint {
    pub(crate) fn from_legacy_address(address: &str) -> Result<Self, ParseIpcEndpointError> {
        parse_tcp_authority(address).map(|(host, port)| Self::Tcp { host, port })
    }

    pub(crate) fn legacy_address(&self) -> Option<String> {
        match self {
            Self::Tcp { host, port } => Some(format_tcp_authority(host, *port)),
            Self::UnixSocket(_) | Self::NamedPipe(_) => None,
        }
    }

    #[cfg(unix)]
    pub(crate) fn default_unix_socket(verified_runtime_base: &Path, scope: &ServerScopeId) -> Self {
        let path = verified_runtime_base
            .join("agenterm")
            .join(format!("{}.sock", scope.as_str()));
        Self::UnixSocket(path.to_string_lossy().into_owned())
    }

    #[cfg(windows)]
    pub(crate) fn default_named_pipe(scope: &ServerScopeId) -> Self {
        Self::NamedPipe(format!(r"\\.\pipe\agenterm-{}", scope.as_str()))
    }

    #[cfg(unix)]
    pub(crate) fn unix_socket_path(&self) -> Option<PathBuf> {
        match self {
            Self::UnixSocket(path) => Some(PathBuf::from(path)),
            Self::NamedPipe(_) | Self::Tcp { .. } => None,
        }
    }

    pub(crate) fn transport_name(&self) -> &'static str {
        match self {
            Self::UnixSocket(_) => "unix",
            Self::NamedPipe(_) => "named_pipe",
            Self::Tcp { .. } => "tcp",
        }
    }

    pub(crate) fn validate_local(&self) -> Result<(), ParseIpcEndpointError> {
        match self {
            Self::Tcp { host, port } => {
                let authority = format_tcp_authority(host, *port);
                authority
                    .parse::<std::net::SocketAddr>()
                    .ok()
                    .filter(|address| address.ip().is_loopback())
                    .map(|_| ())
                    .ok_or(ParseIpcEndpointError)
            }
            Self::UnixSocket(path) => {
                nonempty_endpoint_value(path)?;
                if !PathBuf::from(path).is_absolute() {
                    return Err(ParseIpcEndpointError);
                }
                Ok(())
            }
            Self::NamedPipe(name) => nonempty_endpoint_value(name),
        }
    }
}

impl fmt::Display for IpcEndpoint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnixSocket(path) => write!(formatter, "unix:{path}"),
            Self::NamedPipe(name) => write!(formatter, "pipe:{name}"),
            Self::Tcp { host, port } => {
                write!(formatter, "tcp:{}", format_tcp_authority(host, *port))
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ParseIpcEndpointError;

impl fmt::Display for ParseIpcEndpointError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("IPC endpoint must be unix:<path>, pipe:<name>, or tcp:<host>:<port>")
    }
}

impl std::error::Error for ParseIpcEndpointError {}

impl FromStr for IpcEndpoint {
    type Err = ParseIpcEndpointError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if let Some(path) = value.strip_prefix("unix:") {
            return nonempty_endpoint_value(path).map(|()| Self::UnixSocket(path.to_owned()));
        }
        if let Some(name) = value.strip_prefix("pipe:") {
            return nonempty_endpoint_value(name).map(|()| Self::NamedPipe(name.to_owned()));
        }
        if let Some(authority) = value.strip_prefix("tcp:") {
            return parse_tcp_authority(authority).map(|(host, port)| Self::Tcp { host, port });
        }
        Err(ParseIpcEndpointError)
    }
}

impl Serialize for IpcEndpoint {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for IpcEndpoint {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(de::Error::custom)
    }
}

fn nonempty_endpoint_value(value: &str) -> Result<(), ParseIpcEndpointError> {
    if value.is_empty() || value.chars().any(char::is_control) {
        Err(ParseIpcEndpointError)
    } else {
        Ok(())
    }
}

fn parse_tcp_authority(authority: &str) -> Result<(String, u16), ParseIpcEndpointError> {
    let (host, port) = if let Some(rest) = authority.strip_prefix('[') {
        let (host, port) = rest.split_once("]:").ok_or(ParseIpcEndpointError)?;
        (host, port)
    } else {
        authority.rsplit_once(':').ok_or(ParseIpcEndpointError)?
    };
    if host.is_empty() || host.chars().any(char::is_whitespace) {
        return Err(ParseIpcEndpointError);
    }
    let port = port
        .parse::<u16>()
        .ok()
        .filter(|port| *port != 0)
        .ok_or(ParseIpcEndpointError)?;
    Ok((host.to_owned(), port))
}

fn format_tcp_authority(host: &str, port: u16) -> String {
    if host.contains(':') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    }
}

/// Opaque, stable authority identity. It intentionally contains no username,
/// UID, SID, endpoint, or workspace text.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) struct ServerScopeId(String);

impl ServerScopeId {
    pub(crate) fn derive(user_scope: &TrustedOsUserScope, instance: &LogicalInstance) -> Self {
        let mut digest = Sha256::new();
        digest.update(SCOPE_NAMESPACE.as_bytes());
        digest.update([0]);
        digest.update(user_scope.kind.as_bytes());
        digest.update([0]);
        digest.update(&user_scope.identity);
        digest.update([0]);
        digest.update(instance.canonical_name().as_bytes());
        let bytes = digest.finalize();
        let mut encoded = String::with_capacity(39);
        encoded.push_str("agt-v1-");
        for byte in &bytes[..16] {
            use fmt::Write as _;
            write!(encoded, "{byte:02x}").expect("writing to a String cannot fail");
        }
        Self(encoded)
    }

    pub(crate) fn current(instance: &LogicalInstance) -> std::io::Result<Self> {
        current_trusted_os_user_scope().map(|scope| Self::derive(&scope, instance))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ParseServerScopeIdError;

impl fmt::Display for ParseServerScopeIdError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("invalid AgenTerm server scope ID")
    }
}

impl std::error::Error for ParseServerScopeIdError {}

impl FromStr for ServerScopeId {
    type Err = ParseServerScopeIdError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let valid = value.strip_prefix("agt-v1-").is_some_and(|digest| {
            digest.len() == 32
                && digest
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        });
        valid
            .then(|| Self(value.to_owned()))
            .ok_or(ParseServerScopeIdError)
    }
}

impl Serialize for ServerScopeId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for ServerScopeId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(de::Error::custom)
    }
}

impl fmt::Display for ServerScopeId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct EndpointSelectorArgs {
    pub(crate) endpoint: Option<String>,
    pub(crate) address: Option<String>,
    pub(crate) instance: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ResolvedIpcEndpoint {
    pub(crate) endpoint: IpcEndpoint,
    pub(crate) logical_instance: LogicalInstance,
    pub(crate) server_scope_id: ServerScopeId,
    pub(crate) explicit: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum EndpointSelectionErrorCode {
    ConflictingCliSelectors,
    InvalidEndpoint,
    InvalidAddress,
    InvalidInstance,
    UserScopeUnavailable,
    LegacyDiscoveryUnavailable,
}

#[derive(Debug)]
pub(crate) struct EndpointSelectionError {
    pub(crate) code: EndpointSelectionErrorCode,
    message: String,
}

impl fmt::Display for EndpointSelectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} [{:?}]", self.message, self.code)
    }
}

impl std::error::Error for EndpointSelectionError {}

pub(crate) fn resolve_ipc_endpoint(
    cli: &EndpointSelectorArgs,
) -> Result<ResolvedIpcEndpoint, EndpointSelectionError> {
    let environment = EndpointSelectorArgs {
        endpoint: nonempty_env("AGENTERM_IPC_ENDPOINT"),
        address: nonempty_env("AGENTERM_IPC_ADDRESS"),
        instance: nonempty_env("AGENTERM_INSTANCE"),
    };
    let legacy_main = if selector_count(cli) == 0 && selector_count(&environment) == 0 {
        let expected = legacy_default_main_endpoint();
        crate::instances::discover_healthy_legacy_main_endpoint(
            &expected,
            Duration::from_millis(250),
        )
        .map_err(|error| EndpointSelectionError {
            code: EndpointSelectionErrorCode::LegacyDiscoveryUnavailable,
            message: format!("could not safely inspect legacy main registrations: {error:#}"),
        })?
    } else {
        None
    };
    resolve_ipc_endpoint_with_environment(cli, &environment, legacy_main)
}

fn resolve_ipc_endpoint_with_environment(
    cli: &EndpointSelectorArgs,
    environment: &EndpointSelectorArgs,
    legacy_main: Option<IpcEndpoint>,
) -> Result<ResolvedIpcEndpoint, EndpointSelectionError> {
    let cli_count = selector_count(cli);
    if cli_count > 1 {
        return Err(selection_error(
            EndpointSelectionErrorCode::ConflictingCliSelectors,
            "--endpoint, --address, and --instance are mutually exclusive",
        ));
    }

    // A CLI selector is one complete authority choice.  It must suppress the
    // entire environment selector group, rather than accidentally combining
    // (for example) CLI --instance dev with AGENTERM_IPC_ENDPOINT.
    let selected = if cli_count == 1 { cli } else { environment };
    let instance_text = if selected.endpoint.is_none() && selected.address.is_none() {
        selected
            .instance
            .clone()
            .unwrap_or_else(|| "main".to_owned())
    } else {
        "main".to_owned()
    };
    let logical_instance =
        instance_text
            .parse::<LogicalInstance>()
            .map_err(|error| EndpointSelectionError {
                code: EndpointSelectionErrorCode::InvalidInstance,
                message: format!("invalid AgenTerm instance {instance_text:?}: {error}"),
            })?;
    let server_scope_id =
        ServerScopeId::current(&logical_instance).map_err(|error| EndpointSelectionError {
            code: EndpointSelectionErrorCode::UserScopeUnavailable,
            message: format!("could not resolve current OS-user scope: {error}"),
        })?;

    // Environment selectors are ordered, not merged: endpoint > legacy
    // address > instance > compatibility main.  Lower-priority values cannot
    // change the identity associated with a higher-priority authority.
    let (endpoint, explicit) = if let Some(value) = selected.endpoint.clone() {
        let endpoint = value
            .parse::<IpcEndpoint>()
            .map_err(|error| EndpointSelectionError {
                code: EndpointSelectionErrorCode::InvalidEndpoint,
                message: format!("invalid AgenTerm IPC endpoint {value:?}: {error}"),
            })?;
        endpoint
            .validate_local()
            .map_err(|error| EndpointSelectionError {
                code: EndpointSelectionErrorCode::InvalidEndpoint,
                message: format!("invalid local AgenTerm IPC endpoint {value:?}: {error}"),
            })?;
        (endpoint, true)
    } else if let Some(value) = selected.address.clone() {
        let endpoint =
            IpcEndpoint::from_legacy_address(&value).map_err(|error| EndpointSelectionError {
                code: EndpointSelectionErrorCode::InvalidAddress,
                message: format!("invalid AgenTerm IPC address {value:?}: {error}"),
            })?;
        endpoint.validate_local().map_err(|_| {
            selection_error(
                EndpointSelectionErrorCode::InvalidAddress,
                "legacy AgenTerm IPC address must use a loopback IP",
            )
        })?;
        (endpoint, true)
    } else if selected.instance.is_some() {
        (default_native_endpoint(&server_scope_id), true)
    } else {
        (
            legacy_main.unwrap_or_else(|| default_native_endpoint(&server_scope_id)),
            false,
        )
    };
    Ok(ResolvedIpcEndpoint {
        endpoint,
        logical_instance,
        server_scope_id,
        explicit,
    })
}

fn selector_count(selectors: &EndpointSelectorArgs) -> usize {
    [
        selectors.endpoint.is_some(),
        selectors.address.is_some(),
        selectors.instance.is_some(),
    ]
    .into_iter()
    .filter(|selected| *selected)
    .count()
}

fn legacy_default_main_endpoint() -> IpcEndpoint {
    let user = env::var("USERNAME")
        .or_else(|_| env::var("USER"))
        .unwrap_or_else(|_| "default".to_owned());
    let hash = user.bytes().fold(2_166_136_261_u32, |hash, byte| {
        (hash ^ u32::from(byte)).wrapping_mul(16_777_619)
    });
    IpcEndpoint::Tcp {
        host: "127.0.0.1".to_owned(),
        port: (42_000 + hash % 10_000) as u16,
    }
}

pub(crate) fn default_workspace_path(scope: &ServerScopeId) -> PathBuf {
    #[cfg(windows)]
    let root = env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(env::temp_dir)
        .join("AgenTerm");
    #[cfg(not(windows))]
    let root = env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
        .unwrap_or_else(env::temp_dir)
        .join("agenterm");
    root.join("workspaces")
        .join(format!("{}.json", scope.as_str()))
}

pub(crate) fn default_workspace_path_for(resolved: &ResolvedIpcEndpoint) -> PathBuf {
    if resolved.logical_instance == LogicalInstance::Main {
        return env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."))
            .join("AgenTerm")
            .join("workspace.json");
    }
    default_workspace_path(&resolved.server_scope_id)
}

fn default_native_endpoint(scope: &ServerScopeId) -> IpcEndpoint {
    #[cfg(windows)]
    {
        IpcEndpoint::default_named_pipe(scope)
    }
    #[cfg(unix)]
    {
        let runtime = env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .unwrap_or_else(|| {
                fallback_unix_runtime_base()
                    .join(format!("agenterm-{}", unsafe { libc::geteuid() }))
            });
        IpcEndpoint::default_unix_socket(&runtime, scope)
    }
}

#[cfg(unix)]
pub(crate) fn fallback_unix_runtime_base() -> PathBuf {
    let short_system_temp = Path::new(std::path::MAIN_SEPARATOR_STR).join("tmp");
    std::fs::canonicalize(&short_system_temp)
        .or_else(|_| std::fs::canonicalize(env::temp_dir()))
        .unwrap_or(short_system_temp)
}

fn nonempty_env(name: &str) -> Option<String> {
    env::var(name).ok().filter(|value| !value.trim().is_empty())
}

fn selection_error(
    code: EndpointSelectionErrorCode,
    message: impl Into<String>,
) -> EndpointSelectionError {
    EndpointSelectionError {
        code,
        message: message.into(),
    }
}

/// Trusted binary identity read from the process security context.
///
/// It has no `Debug`, `Display`, or serialization implementation so callers
/// cannot accidentally expose the raw UID/SID in registration or diagnostics.
pub(crate) struct TrustedOsUserScope {
    kind: &'static str,
    identity: Vec<u8>,
}

impl TrustedOsUserScope {
    #[cfg(test)]
    fn for_test(kind: &'static str, identity: &[u8]) -> Self {
        Self {
            kind,
            identity: identity.to_vec(),
        }
    }
}

#[cfg(unix)]
fn current_trusted_os_user_scope() -> std::io::Result<TrustedOsUserScope> {
    let uid = unsafe { libc::geteuid() };
    Ok(TrustedOsUserScope {
        kind: "uid",
        identity: uid.to_le_bytes().to_vec(),
    })
}

#[cfg(windows)]
fn current_trusted_os_user_scope() -> std::io::Result<TrustedOsUserScope> {
    use std::{ffi::c_void, ptr};

    use windows_sys::Win32::{
        Foundation::{CloseHandle, HANDLE},
        System::Threading::GetCurrentProcess,
    };

    const TOKEN_QUERY: u32 = 0x0008;
    const TOKEN_USER_CLASS: u32 = 1;

    #[repr(C)]
    struct SidAndAttributes {
        sid: *mut c_void,
        _attributes: u32,
    }

    #[repr(C)]
    struct TokenUser {
        user: SidAndAttributes,
    }

    #[link(name = "advapi32")]
    unsafe extern "system" {
        fn OpenProcessToken(
            process_handle: HANDLE,
            desired_access: u32,
            token_handle: *mut HANDLE,
        ) -> i32;
        fn GetTokenInformation(
            token_handle: HANDLE,
            token_information_class: u32,
            token_information: *mut c_void,
            token_information_length: u32,
            return_length: *mut u32,
        ) -> i32;
        fn GetLengthSid(sid: *mut c_void) -> u32;
    }

    let mut token = ptr::null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(std::io::Error::last_os_error());
    }
    let result = (|| {
        let mut required = 0;
        unsafe {
            GetTokenInformation(token, TOKEN_USER_CLASS, ptr::null_mut(), 0, &mut required);
        }
        if required < size_of::<TokenUser>() as u32 {
            return Err(std::io::Error::last_os_error());
        }
        let word_count = (required as usize).div_ceil(size_of::<usize>());
        let mut buffer = vec![0usize; word_count];
        if unsafe {
            GetTokenInformation(
                token,
                TOKEN_USER_CLASS,
                buffer.as_mut_ptr().cast(),
                required,
                &mut required,
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error());
        }
        let user = unsafe { &*buffer.as_ptr().cast::<TokenUser>() };
        if user.user.sid.is_null() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "process token returned a null user SID",
            ));
        }
        let sid_length = unsafe { GetLengthSid(user.user.sid) } as usize;
        let buffer_start = buffer.as_ptr() as usize;
        let buffer_end = buffer_start
            .checked_add(required as usize)
            .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::InvalidData))?;
        let sid_start = user.user.sid as usize;
        let sid_end = sid_start
            .checked_add(sid_length)
            .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::InvalidData))?;
        if sid_length == 0 || sid_start < buffer_start || sid_end > buffer_end {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "process token returned an invalid user SID",
            ));
        }
        let sid = unsafe { std::slice::from_raw_parts(user.user.sid.cast::<u8>(), sid_length) };
        Ok(TrustedOsUserScope {
            kind: "sid",
            identity: sid.to_vec(),
        })
    })();
    unsafe {
        CloseHandle(token);
    }
    result
}

#[cfg(windows)]
pub(crate) fn current_user_sid_bytes() -> std::io::Result<Vec<u8>> {
    current_trusted_os_user_scope().map(|scope| scope.identity)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logical_instances_round_trip_without_restricting_unicode_labels() {
        for value in ["main", "dev", "ephemeral:测试-1", "custom:über team"] {
            let parsed = value.parse::<LogicalInstance>().unwrap();
            assert_eq!(parsed.to_string(), value);
            assert_eq!(
                serde_json::from_str::<LogicalInstance>(&serde_json::to_string(&parsed).unwrap())
                    .unwrap(),
                parsed
            );
        }
        assert!("custom:".parse::<LogicalInstance>().is_err());
        assert!("custom:bad\nname".parse::<LogicalInstance>().is_err());
    }

    #[test]
    fn endpoints_parse_display_and_serialize_as_one_typed_value() {
        for value in [
            "unix:/run/user/1000/agenterm/main.sock",
            r"pipe:\\.\pipe\agenterm-agt-v1-deadbeef",
            "tcp:127.0.0.1:48815",
            "tcp:[::1]:48815",
        ] {
            let parsed = value.parse::<IpcEndpoint>().unwrap();
            assert_eq!(parsed.to_string(), value);
            assert_eq!(
                serde_json::from_str::<IpcEndpoint>(&serde_json::to_string(&parsed).unwrap())
                    .unwrap(),
                parsed
            );
        }
        assert!("tcp:127.0.0.1:0".parse::<IpcEndpoint>().is_err());
        assert!("unix:".parse::<IpcEndpoint>().is_err());
        assert!(
            "unix:relative/server.sock"
                .parse::<IpcEndpoint>()
                .unwrap()
                .validate_local()
                .is_err()
        );
    }

    #[test]
    fn scope_is_stable_instance_specific_and_does_not_leak_identity() {
        let identity = TrustedOsUserScope::for_test("sid", b"S-1-5-21-private-user");
        let main = ServerScopeId::derive(&identity, &LogicalInstance::Main);
        let repeated = ServerScopeId::derive(&identity, &LogicalInstance::Main);
        let dev = ServerScopeId::derive(&identity, &LogicalInstance::Dev);
        assert_eq!(main, repeated);
        assert_ne!(main, dev);
        assert_eq!(main.as_str().len(), 39);
        assert!(!main.as_str().contains("private"));
        assert!(!main.as_str().contains("S-1-5"));
        assert_eq!(
            serde_json::from_str::<ServerScopeId>(&serde_json::to_string(&main).unwrap()).unwrap(),
            main
        );
        assert!(serde_json::from_str::<ServerScopeId>(r#""agt-v1-../../escape""#).is_err());
    }

    #[test]
    fn username_is_only_present_in_the_display_label() {
        let instance = LogicalInstance::Custom("舰队".to_owned());
        assert_eq!(instance.display_label("用户"), "用户_custom:舰队");
        let scope = ServerScopeId::derive(
            &TrustedOsUserScope::for_test("uid", &1000_u32.to_le_bytes()),
            &instance,
        );
        assert!(!scope.as_str().contains("用户"));
        assert!(!scope.as_str().contains("舰队"));
    }

    #[test]
    fn display_label_removes_record_separators_and_control_characters() {
        assert_eq!(
            LogicalInstance::Main.display_label("ali\tce\r\n"),
            "ali_ce___main"
        );
        assert_eq!(LogicalInstance::Dev.display_label("\t\r\n"), "user_dev");
    }

    #[test]
    fn native_endpoint_derivation_uses_only_the_opaque_scope_key() {
        let scope = ServerScopeId::derive(
            &TrustedOsUserScope::for_test("uid", &1000_u32.to_le_bytes()),
            &LogicalInstance::Main,
        );
        #[cfg(unix)]
        {
            let unix = IpcEndpoint::default_unix_socket(Path::new("/run/user/1000"), &scope);
            assert_eq!(
                unix.unix_socket_path().unwrap(),
                Path::new("/run/user/1000")
                    .join("agenterm")
                    .join(format!("{}.sock", scope.as_str()))
            );
        }
        #[cfg(windows)]
        let pipe = IpcEndpoint::default_named_pipe(&scope);
        #[cfg(windows)]
        assert_eq!(
            pipe.to_string(),
            format!(r"pipe:\\.\pipe\agenterm-{}", scope.as_str())
        );
    }

    #[test]
    fn selector_rejects_multiple_cli_authorities_and_non_loopback_tcp() {
        let conflict = resolve_ipc_endpoint(&EndpointSelectorArgs {
            endpoint: Some("tcp:127.0.0.1:48815".to_owned()),
            address: None,
            instance: Some("dev".to_owned()),
        })
        .unwrap_err();
        assert_eq!(
            conflict.code,
            EndpointSelectionErrorCode::ConflictingCliSelectors
        );

        let remote = resolve_ipc_endpoint(&EndpointSelectorArgs {
            endpoint: Some("tcp:192.0.2.1:48815".to_owned()),
            ..EndpointSelectorArgs::default()
        })
        .unwrap_err();
        assert_eq!(remote.code, EndpointSelectionErrorCode::InvalidEndpoint);
    }

    #[test]
    fn selector_matrix_keeps_cli_authority_independent_from_environment() {
        let environment = EndpointSelectorArgs {
            endpoint: Some("tcp:127.0.0.1:49001".to_owned()),
            address: Some("127.0.0.1:49002".to_owned()),
            instance: Some("dev".to_owned()),
        };
        let cases = [
            (
                EndpointSelectorArgs {
                    endpoint: Some("tcp:127.0.0.1:49101".to_owned()),
                    ..EndpointSelectorArgs::default()
                },
                IpcEndpoint::Tcp {
                    host: "127.0.0.1".to_owned(),
                    port: 49101,
                },
                LogicalInstance::Main,
            ),
            (
                EndpointSelectorArgs {
                    address: Some("127.0.0.1:49102".to_owned()),
                    ..EndpointSelectorArgs::default()
                },
                IpcEndpoint::Tcp {
                    host: "127.0.0.1".to_owned(),
                    port: 49102,
                },
                LogicalInstance::Main,
            ),
        ];
        for (cli, expected_endpoint, expected_instance) in cases {
            let resolved = resolve_ipc_endpoint_with_environment(&cli, &environment, None).unwrap();
            assert_eq!(resolved.endpoint, expected_endpoint);
            assert_eq!(resolved.logical_instance, expected_instance);
            assert!(resolved.explicit);
        }

        let dev = resolve_ipc_endpoint_with_environment(
            &EndpointSelectorArgs {
                instance: Some("dev".to_owned()),
                ..EndpointSelectorArgs::default()
            },
            &environment,
            None,
        )
        .unwrap();
        assert_eq!(dev.logical_instance, LogicalInstance::Dev);
        assert_eq!(dev.endpoint.transport_name(), native_transport_name());
    }

    #[test]
    fn environment_selector_priority_is_endpoint_then_address_then_instance() {
        let cases = [
            (
                EndpointSelectorArgs {
                    endpoint: Some("tcp:127.0.0.1:49201".to_owned()),
                    address: Some("127.0.0.1:49202".to_owned()),
                    instance: Some("dev".to_owned()),
                },
                "tcp:127.0.0.1:49201",
                LogicalInstance::Main,
            ),
            (
                EndpointSelectorArgs {
                    endpoint: None,
                    address: Some("127.0.0.1:49202".to_owned()),
                    instance: Some("dev".to_owned()),
                },
                "tcp:127.0.0.1:49202",
                LogicalInstance::Main,
            ),
        ];
        for (environment, expected, expected_instance) in cases {
            let resolved = resolve_ipc_endpoint_with_environment(
                &EndpointSelectorArgs::default(),
                &environment,
                None,
            )
            .unwrap();
            assert_eq!(resolved.endpoint.to_string(), expected);
            assert_eq!(resolved.logical_instance, expected_instance);
            assert!(resolved.explicit);
        }

        let instance_only = resolve_ipc_endpoint_with_environment(
            &EndpointSelectorArgs::default(),
            &EndpointSelectorArgs {
                instance: Some("dev".to_owned()),
                ..EndpointSelectorArgs::default()
            },
            None,
        )
        .unwrap();
        assert_eq!(instance_only.logical_instance, LogicalInstance::Dev);
        assert_eq!(
            instance_only.endpoint.transport_name(),
            native_transport_name()
        );
        assert!(instance_only.explicit);
    }

    #[test]
    fn selector_conflict_is_limited_to_cli_and_implicit_main_migrates_safely() {
        let environment_conflict = EndpointSelectorArgs {
            endpoint: Some("tcp:127.0.0.1:49301".to_owned()),
            address: Some("not-an-address".to_owned()),
            instance: Some("not-an-instance".to_owned()),
        };
        let resolved = resolve_ipc_endpoint_with_environment(
            &EndpointSelectorArgs::default(),
            &environment_conflict,
            None,
        )
        .unwrap();
        assert_eq!(resolved.endpoint.to_string(), "tcp:127.0.0.1:49301");

        let default = resolve_ipc_endpoint_with_environment(
            &EndpointSelectorArgs::default(),
            &EndpointSelectorArgs::default(),
            None,
        )
        .unwrap();
        assert_eq!(default.logical_instance, LogicalInstance::Main);
        assert_eq!(default.endpoint.transport_name(), native_transport_name());
        assert!(!default.explicit);
        assert!(
            default_workspace_path_for(&default)
                .ends_with(PathBuf::from("AgenTerm").join("workspace.json"))
        );

        let native_main = resolve_ipc_endpoint_with_environment(
            &EndpointSelectorArgs {
                instance: Some("main".to_owned()),
                ..EndpointSelectorArgs::default()
            },
            &EndpointSelectorArgs::default(),
            None,
        )
        .unwrap();
        assert_eq!(
            default_workspace_path_for(&native_main),
            default_workspace_path_for(&default)
        );
        assert_ne!(
            default_workspace_path_for(&native_main),
            default_workspace_path(&native_main.server_scope_id)
        );

        let legacy = legacy_default_main_endpoint();
        let migrated = resolve_ipc_endpoint_with_environment(
            &EndpointSelectorArgs::default(),
            &EndpointSelectorArgs::default(),
            Some(legacy.clone()),
        )
        .unwrap();
        assert_eq!(migrated.endpoint, legacy);
        assert_eq!(
            default_workspace_path_for(&migrated),
            default_workspace_path_for(&default)
        );
    }

    fn native_transport_name() -> &'static str {
        #[cfg(windows)]
        {
            "named_pipe"
        }
        #[cfg(unix)]
        {
            "unix"
        }
    }

    #[test]
    fn main_and_dev_have_distinct_native_endpoints() {
        let main = ServerScopeId::current(&LogicalInstance::Main).unwrap();
        let dev = ServerScopeId::current(&LogicalInstance::Dev).unwrap();
        let main_endpoint = default_native_endpoint(&main);
        let dev_endpoint = default_native_endpoint(&dev);
        assert_ne!(main_endpoint, dev_endpoint);
        #[cfg(windows)]
        assert!(matches!(main_endpoint, IpcEndpoint::NamedPipe(_)));
        #[cfg(unix)]
        assert!(matches!(main_endpoint, IpcEndpoint::UnixSocket(_)));
    }
}
