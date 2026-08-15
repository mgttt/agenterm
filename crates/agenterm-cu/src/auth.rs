//! Per-target authorization for `agenterm-cu` (PRD_02_31).

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GrantParseErrorKind {
    EmptyToken,
    UnknownToken,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GrantParseError {
    pub kind: GrantParseErrorKind,
    pub token_index: usize,
    pub token: String,
}

impl std::fmt::Display for GrantParseError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.kind {
            GrantParseErrorKind::EmptyToken => {
                write!(formatter, "grant scope token {} is empty", self.token_index)
            }
            GrantParseErrorKind::UnknownToken => write!(
                formatter,
                "grant scope token {} is unknown: {:?}",
                self.token_index, self.token
            ),
        }
    }
}

impl std::error::Error for GrantParseError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GrantSource {
    /// Constructed directly by an internal caller. This is compatibility data,
    /// not a persistent or target-bound authority claim.
    Direct,
    Cli,
    Environment,
    None,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvaluatedGrants {
    pub source: GrantSource,
    pub grants: BTreeSet<Grant>,
}

/// Least-capability grants: observation and actuation are distinct.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Ord, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Grant {
    Observe,
    Actuate,
}

impl Grant {
    /// Strict scope parser for authorization-facing callers. Unlike the legacy
    /// compatibility helper, one malformed token rejects the complete value.
    pub fn parse_many_strict(raw: &str) -> Result<BTreeSet<Self>, GrantParseError> {
        let mut grants = BTreeSet::new();
        for (token_index, raw_token) in raw.split(',').enumerate() {
            let token = raw_token.trim();
            if token.is_empty() {
                return Err(GrantParseError {
                    kind: GrantParseErrorKind::EmptyToken,
                    token_index,
                    token: String::new(),
                });
            }
            let grant = match token.to_ascii_lowercase().as_str() {
                "observe" | "observation" => Self::Observe,
                "actuate" | "actuation" | "act" => Self::Actuate,
                _ => {
                    return Err(GrantParseError {
                        kind: GrantParseErrorKind::UnknownToken,
                        token_index,
                        token: token.to_owned(),
                    });
                }
            };
            grants.insert(grant);
        }
        Ok(grants)
    }

    /// Compatibility parser retained for existing callers. Malformed input
    /// fails closed to no grants instead of keeping a valid-looking subset.
    pub fn parse_many(raw: &str) -> BTreeSet<Self> {
        Self::parse_many_strict(raw).unwrap_or_default()
    }

    pub fn from_env() -> BTreeSet<Self> {
        std::env::var("AGENTERM_CU_GRANT")
            .map(|value| Self::parse_many(&value))
            .unwrap_or_default()
    }
}

/// Select and parse one grant source without escalating by union. A present
/// CLI value is authoritative even when it is invalid; environment is only a
/// fallback when the CLI omitted authorization entirely.
pub fn evaluate_grant_sources(
    cli_grant: Option<&str>,
    environment_grant: Option<&str>,
) -> Result<EvaluatedGrants, GrantParseError> {
    let (source, raw) = if let Some(raw) = cli_grant {
        (GrantSource::Cli, Some(raw))
    } else if let Some(raw) = environment_grant {
        (GrantSource::Environment, Some(raw))
    } else {
        (GrantSource::None, None)
    };
    let grants = match raw {
        Some(raw) => Grant::parse_many_strict(raw)?,
        None => BTreeSet::new(),
    };
    Ok(EvaluatedGrants { source, grants })
}

/// Authorization selectors and future provider material never cross a worker
/// boundary through caller-controlled environment forwarding.
pub(crate) fn is_reserved_authority_env(key: &str) -> bool {
    ["AGENTERM_CU_GRANT", "AGENTERM_CU_AUTH"]
        .iter()
        .any(|prefix| {
            key.get(..prefix.len())
                .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
        })
}

pub(crate) fn clear_reserved_authority_environment(command: &mut std::process::Command) {
    // Remove current and planned selectors even when they are absent from this
    // parent environment, so a caller-set Command override cannot retain them.
    for key in [
        "AGENTERM_CU_GRANT",
        "AGENTERM_CU_GRANT_ID",
        "AGENTERM_CU_AUTH",
        "AGENTERM_CU_AUTH_PROVIDER",
    ] {
        command.env_remove(key);
    }
    // Also remove every future suffix already present in the parent. This
    // prevents OpenSSH SendEnv and local session workers from inheriting it.
    for (key, _) in std::env::vars_os() {
        if key.to_str().is_some_and(is_reserved_authority_env) {
            command.env_remove(key);
        }
    }
}

pub struct Authorization {
    grants: BTreeSet<Grant>,
    source: GrantSource,
}

impl Authorization {
    pub fn new(grants: BTreeSet<Grant>) -> Self {
        Self {
            grants,
            source: GrantSource::Direct,
        }
    }

    pub fn try_from_sources(
        cli_grant: Option<&str>,
        environment_grant: Option<&str>,
    ) -> Result<Self, GrantParseError> {
        let evaluated = evaluate_grant_sources(cli_grant, environment_grant)?;
        Ok(Self {
            grants: evaluated.grants,
            source: evaluated.source,
        })
    }

    /// Compatibility entry point for the current CLI. CLI takes precedence;
    /// environment is read only when CLI authorization is absent. Until the
    /// binary adopts the strict Result API, malformed selected input fails
    /// closed to an empty authorization.
    pub fn from_cli_and_env(cli_grant: Option<&str>) -> Self {
        let environment = if cli_grant.is_none() {
            std::env::var("AGENTERM_CU_GRANT").ok()
        } else {
            None
        };
        match Self::try_from_sources(cli_grant, environment.as_deref()) {
            Ok(authorization) => authorization,
            Err(_) => Self {
                grants: BTreeSet::new(),
                source: if cli_grant.is_some() {
                    GrantSource::Cli
                } else {
                    GrantSource::Environment
                },
            },
        }
    }

    pub fn allows(&self, required: Grant) -> bool {
        self.grants.contains(&required)
    }

    pub fn source(&self) -> GrantSource {
        self.source
    }

    /// Reconstruct a `--grant` CLI value for session workers (ssh / vnc
    /// transport). Observe and actuate both forward so remote get-selection /
    /// get-extents / get-caret / tree / send-text / paste / copy /
    /// send-keys / select / set-caret / click / scroll / focus work.
    pub fn grant_cli_arg(&self) -> String {
        let mut parts = Vec::new();
        if self.grants.contains(&Grant::Observe) {
            parts.push("observe");
        }
        if self.grants.contains(&Grant::Actuate) {
            parts.push("actuate");
        }
        parts.join(",")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grant_cli_arg_forwards_actuate_for_session_write() {
        // ssh / vnc session workers need actuate for send-text / paste / copy / send-keys / select / set-caret / click / scroll / focus.
        let auth = Authorization::from_cli_and_env(Some("observe,actuate"));
        assert_eq!(auth.grant_cli_arg(), "observe,actuate");
        assert!(auth.allows(Grant::Observe));
        assert!(auth.allows(Grant::Actuate));
    }

    #[test]
    fn grant_cli_arg_observe_only() {
        let auth = Authorization::from_cli_and_env(Some("observe"));
        assert_eq!(auth.grant_cli_arg(), "observe");
        assert!(!auth.allows(Grant::Actuate));
    }

    #[test]
    fn strict_parser_rejects_unknown_and_empty_tokens() {
        assert_eq!(
            Grant::parse_many_strict("observe,screen").unwrap_err(),
            GrantParseError {
                kind: GrantParseErrorKind::UnknownToken,
                token_index: 1,
                token: "screen".into(),
            }
        );
        for raw in ["", " ", ",observe", "observe,", "observe,,actuate"] {
            let error = Grant::parse_many_strict(raw).unwrap_err();
            assert_eq!(error.kind, GrantParseErrorKind::EmptyToken, "raw={raw:?}");
        }
        assert!(
            Grant::parse_many("observe,unknown").is_empty(),
            "compatibility parsing must fail closed rather than retain observe"
        );
    }

    #[test]
    fn strict_parser_accepts_both_distinct_scopes() {
        let grants = Grant::parse_many_strict(" observe , ACTUATE ").unwrap();
        assert_eq!(grants, BTreeSet::from([Grant::Observe, Grant::Actuate]));
    }

    #[test]
    fn cli_source_takes_precedence_without_union_with_environment() {
        let evaluated = evaluate_grant_sources(Some("observe"), Some("actuate")).unwrap();
        assert_eq!(evaluated.source, GrantSource::Cli);
        assert_eq!(evaluated.grants, BTreeSet::from([Grant::Observe]));

        let auth = Authorization::try_from_sources(Some("observe"), Some("actuate")).unwrap();
        assert_eq!(auth.source(), GrantSource::Cli);
        assert!(auth.allows(Grant::Observe));
        assert!(!auth.allows(Grant::Actuate));
    }

    #[test]
    fn environment_is_used_only_when_cli_is_absent() {
        let auth = Authorization::try_from_sources(None, Some("actuate")).unwrap();
        assert_eq!(auth.source(), GrantSource::Environment);
        assert!(!auth.allows(Grant::Observe));
        assert!(auth.allows(Grant::Actuate));

        let none = Authorization::try_from_sources(None, None).unwrap();
        assert_eq!(none.source(), GrantSource::None);
        assert!(!none.allows(Grant::Observe));
        assert!(!none.allows(Grant::Actuate));
    }

    #[test]
    fn invalid_cli_does_not_fall_back_to_valid_environment() {
        let error = Authorization::try_from_sources(Some("unknown"), Some("actuate"))
            .err()
            .expect("CLI parse failure must win");
        assert_eq!(error.kind, GrantParseErrorKind::UnknownToken);
    }

    #[test]
    fn worker_environment_reserves_all_authorization_prefixes() {
        for key in [
            "AGENTERM_CU_GRANT",
            "agenterm_cu_grant_id",
            "AGENTERM_CU_AUTH",
            "AgEnTeRm_Cu_AuTh_Provider",
        ] {
            assert!(is_reserved_authority_env(key), "key={key}");
        }
        for key in ["AGENTERM_CU_AUDIT_PATH", "AGENTERM_ABI_LIB", "DISPLAY"] {
            assert!(!is_reserved_authority_env(key), "key={key}");
        }
    }

    #[test]
    fn worker_command_clears_authorization_but_retains_unrelated_environment() {
        let mut command = std::process::Command::new("fixture-worker");
        command
            .env("AGENTERM_CU_GRANT", "credential-seed")
            .env("AGENTERM_CU_AUTH_PROVIDER", "provider-seed")
            .env("AGENTERM_CU_AUDIT_PATH", "audit.jsonl");
        clear_reserved_authority_environment(&mut command);

        let configured: std::collections::BTreeMap<_, _> = command
            .get_envs()
            .map(|(key, value)| (key.to_owned(), value.map(ToOwned::to_owned)))
            .collect();
        assert_eq!(
            configured.get(std::ffi::OsStr::new("AGENTERM_CU_GRANT")),
            Some(&None)
        );
        assert_eq!(
            configured.get(std::ffi::OsStr::new("AGENTERM_CU_AUTH_PROVIDER")),
            Some(&None)
        );
        assert_eq!(
            configured.get(std::ffi::OsStr::new("AGENTERM_CU_AUDIT_PATH")),
            Some(&Some(std::ffi::OsString::from("audit.jsonl")))
        );
    }
}
