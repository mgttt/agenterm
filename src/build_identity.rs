use serde::Serialize;

pub(crate) const BUILD_IDENTITY_SCHEMA_VERSION: u32 = 1;
const UNKNOWN: &str = "unknown";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DirtyState {
    Clean,
    Dirty,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct BuildIdentity {
    pub(crate) schema_version: u32,
    pub(crate) git_commit: &'static str,
    pub(crate) git_dirty: DirtyState,
    pub(crate) cargo_lock_sha256: &'static str,
    pub(crate) artifact_manifest_sha256: &'static str,
    pub(crate) profile: &'static str,
}

#[derive(Clone, Copy)]
struct EmbeddedIdentity {
    schema_version: Option<&'static str>,
    git_commit: Option<&'static str>,
    git_dirty: Option<&'static str>,
    cargo_lock_sha256: Option<&'static str>,
    artifact_manifest_sha256: Option<&'static str>,
    profile: Option<&'static str>,
}

impl BuildIdentity {
    pub(crate) fn current() -> Self {
        Self::from_embedded(EmbeddedIdentity {
            schema_version: option_env!("AGENTERM_BUILD_IDENTITY_VERSION"),
            git_commit: option_env!("AGENTERM_BUILD_GIT_COMMIT"),
            git_dirty: option_env!("AGENTERM_BUILD_GIT_DIRTY"),
            cargo_lock_sha256: option_env!("AGENTERM_BUILD_CARGO_LOCK_SHA256"),
            artifact_manifest_sha256: option_env!("AGENTERM_BUILD_ARTIFACT_MANIFEST_SHA256"),
            profile: option_env!("AGENTERM_BUILD_PROFILE"),
        })
    }

    fn from_embedded(embedded: EmbeddedIdentity) -> Self {
        let schema_is_current = embedded.schema_version == Some(BUILD_IDENTITY_SCHEMA_VERSION_STR);
        Self {
            schema_version: BUILD_IDENTITY_SCHEMA_VERSION,
            git_commit: if schema_is_current {
                valid_hex(embedded.git_commit, &[40, 64])
            } else {
                UNKNOWN
            },
            git_dirty: if schema_is_current {
                match embedded.git_dirty {
                    Some("false") => DirtyState::Clean,
                    Some("true") => DirtyState::Dirty,
                    _ => DirtyState::Unknown,
                }
            } else {
                DirtyState::Unknown
            },
            cargo_lock_sha256: if schema_is_current {
                valid_hex(embedded.cargo_lock_sha256, &[64])
            } else {
                UNKNOWN
            },
            artifact_manifest_sha256: if schema_is_current {
                valid_hex(embedded.artifact_manifest_sha256, &[64])
            } else {
                UNKNOWN
            },
            profile: if schema_is_current {
                match embedded.profile {
                    Some(profile @ ("dev" | "release-fast" | "release")) => profile,
                    _ => UNKNOWN,
                }
            } else {
                UNKNOWN
            },
        }
    }

    pub(crate) fn is_complete(&self) -> bool {
        self.git_commit != UNKNOWN
            && self.git_dirty != DirtyState::Unknown
            && self.cargo_lock_sha256 != UNKNOWN
            && self.artifact_manifest_sha256 != UNKNOWN
            && self.profile != UNKNOWN
    }
}

const BUILD_IDENTITY_SCHEMA_VERSION_STR: &str = "1";

fn valid_hex(value: Option<&'static str>, lengths: &[usize]) -> &'static str {
    value
        .filter(|value| {
            lengths.contains(&value.len())
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
        .unwrap_or(UNKNOWN)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn complete_identity() -> EmbeddedIdentity {
        EmbeddedIdentity {
            schema_version: Some("1"),
            git_commit: Some("0123456789abcdef0123456789abcdef01234567"),
            git_dirty: Some("true"),
            cargo_lock_sha256: Some(
                "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
            ),
            artifact_manifest_sha256: Some(
                "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789",
            ),
            profile: Some("release-fast"),
        }
    }

    #[test]
    fn complete_embedded_identity_is_truthful_and_serializable() {
        let identity = BuildIdentity::from_embedded(complete_identity());
        assert!(identity.is_complete());
        assert_eq!(identity.git_dirty, DirtyState::Dirty);
        assert_eq!(identity.profile, "release-fast");

        let value = serde_json::to_value(identity).unwrap();
        assert_eq!(value["schema_version"], 1);
        assert_eq!(value["git_dirty"], "dirty");
        assert_eq!(
            value["git_commit"],
            "0123456789abcdef0123456789abcdef01234567"
        );
    }

    #[test]
    fn missing_values_are_unknown_instead_of_fabricated() {
        let identity = BuildIdentity::from_embedded(EmbeddedIdentity {
            schema_version: None,
            git_commit: None,
            git_dirty: None,
            cargo_lock_sha256: None,
            artifact_manifest_sha256: None,
            profile: None,
        });
        assert!(!identity.is_complete());
        assert_eq!(identity.git_commit, UNKNOWN);
        assert_eq!(identity.git_dirty, DirtyState::Unknown);
        assert_eq!(identity.cargo_lock_sha256, UNKNOWN);
        assert_eq!(identity.artifact_manifest_sha256, UNKNOWN);
        assert_eq!(identity.profile, UNKNOWN);
    }

    #[test]
    fn malformed_or_partial_values_are_individually_unknown() {
        let mut embedded = complete_identity();
        embedded.git_commit = Some("short");
        embedded.git_dirty = Some("maybe");
        embedded.cargo_lock_sha256 =
            Some("ABCDEF0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF0123456789");
        embedded.profile = Some("custom");

        let identity = BuildIdentity::from_embedded(embedded);
        assert_eq!(identity.git_commit, UNKNOWN);
        assert_eq!(identity.git_dirty, DirtyState::Unknown);
        assert_eq!(identity.cargo_lock_sha256, UNKNOWN);
        assert_ne!(identity.artifact_manifest_sha256, UNKNOWN);
        assert_eq!(identity.profile, UNKNOWN);
    }

    #[test]
    fn unknown_schema_does_not_relabel_values_as_current() {
        let mut embedded = complete_identity();
        embedded.schema_version = Some("2");
        let identity = BuildIdentity::from_embedded(embedded);
        assert!(!identity.is_complete());
        assert_eq!(identity.git_commit, UNKNOWN);
        assert_eq!(identity.git_dirty, DirtyState::Unknown);
    }

    #[test]
    fn clean_state_is_distinct_from_unknown() {
        let mut embedded = complete_identity();
        embedded.git_dirty = Some("false");
        let identity = BuildIdentity::from_embedded(embedded);
        assert_eq!(identity.git_dirty, DirtyState::Clean);
        assert!(identity.is_complete());
    }
}
