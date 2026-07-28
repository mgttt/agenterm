use serde::{Deserialize, Serialize};

use crate::build_identity::BuildIdentity;

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct UpgradeIdentity {
    #[serde(
        rename = "protocol_version",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub protocol_version: Option<u32>,
    #[serde(rename = "version", default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(
        rename = "git_commit",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub git_commit: Option<String>,
    #[serde(rename = "profile", default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    #[serde(
        rename = "cargo_lock_sha256",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub cargo_lock_sha256: Option<String>,
    #[serde(
        rename = "artifact_manifest_sha256",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub artifact_manifest_sha256: Option<String>,
}

impl UpgradeIdentity {
    pub(crate) fn current(protocol_version: u32) -> Self {
        let build = BuildIdentity::current();
        let known = |value: &str| {
            (value != "unknown" && !value.trim().is_empty()).then(|| value.to_owned())
        };
        Self {
            protocol_version: Some(protocol_version),
            version: Some(env!("CARGO_PKG_VERSION").to_owned()),
            git_commit: known(build.git_commit),
            profile: known(build.profile),
            cargo_lock_sha256: known(build.cargo_lock_sha256),
            artifact_manifest_sha256: known(build.artifact_manifest_sha256),
        }
    }

    pub(crate) fn compare_staged(&self, staged: &Self) -> UpgradeIdentityComparison {
        let running_missing = self.missing_fields();
        let staged_missing = staged.missing_fields();
        if !running_missing.is_empty() || !staged_missing.is_empty() {
            return UpgradeIdentityComparison {
                status: UpgradeIdentityStatus::Unknown,
                reasons: vec![UpgradeIdentityReason::MissingFields {
                    running: running_missing,
                    staged: staged_missing,
                }],
            };
        }

        let running_protocol = self
            .protocol_version
            .expect("complete identity has a protocol version");
        let staged_protocol = staged
            .protocol_version
            .expect("complete identity has a protocol version");
        if running_protocol != staged_protocol {
            return UpgradeIdentityComparison {
                status: UpgradeIdentityStatus::Incompatible,
                reasons: vec![UpgradeIdentityReason::ProtocolIncompatible {
                    running: running_protocol,
                    staged: staged_protocol,
                }],
            };
        }

        let mut fields = Vec::new();
        if self.version != staged.version {
            fields.push(UpgradeIdentityField::Version);
        }
        if self.git_commit != staged.git_commit {
            fields.push(UpgradeIdentityField::GitCommit);
        }
        if self.profile != staged.profile {
            fields.push(UpgradeIdentityField::Profile);
        }
        if self.cargo_lock_sha256 != staged.cargo_lock_sha256 {
            fields.push(UpgradeIdentityField::CargoLockSha256);
        }
        if self.artifact_manifest_sha256 != staged.artifact_manifest_sha256 {
            fields.push(UpgradeIdentityField::ArtifactManifestSha256);
        }

        if fields.is_empty() {
            UpgradeIdentityComparison {
                status: UpgradeIdentityStatus::Same,
                reasons: vec![UpgradeIdentityReason::Identical],
            }
        } else {
            UpgradeIdentityComparison {
                status: UpgradeIdentityStatus::Stale,
                reasons: vec![UpgradeIdentityReason::FieldsChanged { fields }],
            }
        }
    }

    fn missing_fields(&self) -> Vec<UpgradeIdentityField> {
        let mut fields = Vec::new();
        if self.protocol_version.is_none() {
            fields.push(UpgradeIdentityField::ProtocolVersion);
        }
        if is_missing_text(&self.version) {
            fields.push(UpgradeIdentityField::Version);
        }
        if is_missing_text(&self.git_commit) {
            fields.push(UpgradeIdentityField::GitCommit);
        }
        if is_missing_text(&self.profile) {
            fields.push(UpgradeIdentityField::Profile);
        }
        if is_missing_text(&self.cargo_lock_sha256) {
            fields.push(UpgradeIdentityField::CargoLockSha256);
        }
        if is_missing_text(&self.artifact_manifest_sha256) {
            fields.push(UpgradeIdentityField::ArtifactManifestSha256);
        }
        fields
    }
}

fn is_missing_text(value: &Option<String>) -> bool {
    value.as_deref().is_none_or(|value| value.trim().is_empty())
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) enum UpgradeIdentityStatus {
    #[serde(rename = "same")]
    Same,
    #[serde(rename = "stale")]
    Stale,
    #[serde(rename = "incompatible")]
    Incompatible,
    #[serde(rename = "unknown")]
    Unknown,
}

impl UpgradeIdentityStatus {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Same => "same",
            Self::Stale => "stale",
            Self::Incompatible => "incompatible",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct UpgradeIdentityComparison {
    #[serde(rename = "status")]
    pub(crate) status: UpgradeIdentityStatus,
    #[serde(rename = "reasons")]
    pub(crate) reasons: Vec<UpgradeIdentityReason>,
}

impl UpgradeIdentityComparison {
    pub(crate) fn explanation(&self) -> String {
        match self.reasons.as_slice() {
            [UpgradeIdentityReason::Identical] => {
                "running and staged build identities match".to_owned()
            }
            [UpgradeIdentityReason::MissingFields { running, staged }] => {
                let mut sides = Vec::new();
                if !running.is_empty() {
                    sides.push(format!("running is missing {}", field_names(running)));
                }
                if !staged.is_empty() {
                    sides.push(format!("staged is missing {}", field_names(staged)));
                }
                format!("identity is incomplete: {}", sides.join("; "))
            }
            [UpgradeIdentityReason::ProtocolIncompatible { running, staged }] => {
                format!("protocol versions are incompatible: running {running}, staged {staged}")
            }
            [UpgradeIdentityReason::FieldsChanged { fields }] => {
                format!(
                    "staged build differs from running build in {}",
                    field_names(fields)
                )
            }
            _ => format!("upgrade identity status is {}", self.status.as_str()),
        }
    }
}

fn field_names(fields: &[UpgradeIdentityField]) -> String {
    fields
        .iter()
        .map(|field| field.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum UpgradeIdentityReason {
    Identical,
    MissingFields {
        running: Vec<UpgradeIdentityField>,
        staged: Vec<UpgradeIdentityField>,
    },
    ProtocolIncompatible {
        running: u32,
        staged: u32,
    },
    FieldsChanged {
        fields: Vec<UpgradeIdentityField>,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) enum UpgradeIdentityField {
    #[serde(rename = "protocol_version")]
    ProtocolVersion,
    #[serde(rename = "version")]
    Version,
    #[serde(rename = "git_commit")]
    GitCommit,
    #[serde(rename = "profile")]
    Profile,
    #[serde(rename = "cargo_lock_sha256")]
    CargoLockSha256,
    #[serde(rename = "artifact_manifest_sha256")]
    ArtifactManifestSha256,
}

impl UpgradeIdentityField {
    const fn as_str(self) -> &'static str {
        match self {
            Self::ProtocolVersion => "protocol_version",
            Self::Version => "version",
            Self::GitCommit => "git_commit",
            Self::Profile => "profile",
            Self::CargoLockSha256 => "cargo_lock_sha256",
            Self::ArtifactManifestSha256 => "artifact_manifest_sha256",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn complete_identity() -> UpgradeIdentity {
        UpgradeIdentity {
            protocol_version: Some(1),
            version: Some("0.1.7".to_owned()),
            git_commit: Some("a".repeat(40)),
            profile: Some("release".to_owned()),
            cargo_lock_sha256: Some("b".repeat(64)),
            artifact_manifest_sha256: Some("c".repeat(64)),
        }
    }

    #[test]
    fn complete_identical_identities_are_same() {
        let running = complete_identity();
        let comparison = running.compare_staged(&running);

        assert_eq!(comparison.status, UpgradeIdentityStatus::Same);
        assert_eq!(comparison.reasons, vec![UpgradeIdentityReason::Identical]);
        assert_eq!(
            comparison.explanation(),
            "running and staged build identities match"
        );
    }

    #[test]
    fn each_known_non_protocol_difference_is_stale() {
        let running = complete_identity();
        let cases = [
            (UpgradeIdentityField::Version, {
                let mut value = running.clone();
                value.version = Some("0.1.8".to_owned());
                value
            }),
            (UpgradeIdentityField::GitCommit, {
                let mut value = running.clone();
                value.git_commit = Some("d".repeat(40));
                value
            }),
            (UpgradeIdentityField::Profile, {
                let mut value = running.clone();
                value.profile = Some("release-fast".to_owned());
                value
            }),
            (UpgradeIdentityField::CargoLockSha256, {
                let mut value = running.clone();
                value.cargo_lock_sha256 = Some("e".repeat(64));
                value
            }),
            (UpgradeIdentityField::ArtifactManifestSha256, {
                let mut value = running.clone();
                value.artifact_manifest_sha256 = Some("f".repeat(64));
                value
            }),
        ];

        for (field, staged) in cases {
            let comparison = running.compare_staged(&staged);
            assert_eq!(comparison.status, UpgradeIdentityStatus::Stale);
            assert_eq!(
                comparison.reasons,
                vec![UpgradeIdentityReason::FieldsChanged {
                    fields: vec![field],
                }]
            );
        }
    }

    #[test]
    fn protocol_difference_is_incompatible_even_with_other_known_differences() {
        let running = complete_identity();
        let mut staged = running.clone();
        staged.protocol_version = Some(2);
        staged.version = Some("0.2.0".to_owned());

        let comparison = running.compare_staged(&staged);
        assert_eq!(comparison.status, UpgradeIdentityStatus::Incompatible);
        assert_eq!(
            comparison.reasons,
            vec![UpgradeIdentityReason::ProtocolIncompatible {
                running: 1,
                staged: 2,
            }]
        );
    }

    #[test]
    fn every_missing_field_on_either_side_is_unknown() {
        let complete = complete_identity();
        for field in [
            UpgradeIdentityField::ProtocolVersion,
            UpgradeIdentityField::Version,
            UpgradeIdentityField::GitCommit,
            UpgradeIdentityField::Profile,
            UpgradeIdentityField::CargoLockSha256,
            UpgradeIdentityField::ArtifactManifestSha256,
        ] {
            let mut running = complete.clone();
            clear_field(&mut running, field);
            let comparison = running.compare_staged(&complete);
            assert_unknown_on_side(&comparison, field, true);

            let mut staged = complete.clone();
            clear_field(&mut staged, field);
            let comparison = complete.compare_staged(&staged);
            assert_unknown_on_side(&comparison, field, false);
        }
    }

    #[test]
    fn missing_field_takes_precedence_over_protocol_difference() {
        let running = complete_identity();
        let mut staged = running.clone();
        staged.protocol_version = Some(2);
        staged.git_commit = None;

        let comparison = running.compare_staged(&staged);
        assert_eq!(comparison.status, UpgradeIdentityStatus::Unknown);
        assert!(matches!(
            comparison.reasons.as_slice(),
            [UpgradeIdentityReason::MissingFields { staged, .. }]
                if staged == &[UpgradeIdentityField::GitCommit]
        ));
    }

    #[test]
    fn empty_text_is_treated_as_unknown_identity_data() {
        let running = complete_identity();
        let mut staged = running.clone();
        staged.profile = Some(" \t".to_owned());

        let comparison = running.compare_staged(&staged);
        assert_eq!(comparison.status, UpgradeIdentityStatus::Unknown);
        assert!(
            comparison
                .explanation()
                .contains("staged is missing profile")
        );
    }

    #[test]
    fn serde_wire_names_are_stable_and_missing_fields_round_trip() {
        let identity_json = serde_json::to_value(complete_identity()).unwrap();
        assert_eq!(identity_json["protocol_version"], 1);
        assert_eq!(identity_json["version"], "0.1.7");
        assert!(identity_json.get("git_commit").is_some());
        assert!(identity_json.get("profile").is_some());
        assert!(identity_json.get("cargo_lock_sha256").is_some());
        assert!(identity_json.get("artifact_manifest_sha256").is_some());

        for (status, wire) in [
            (UpgradeIdentityStatus::Same, "\"same\""),
            (UpgradeIdentityStatus::Stale, "\"stale\""),
            (UpgradeIdentityStatus::Incompatible, "\"incompatible\""),
            (UpgradeIdentityStatus::Unknown, "\"unknown\""),
        ] {
            assert_eq!(serde_json::to_string(&status).unwrap(), wire);
        }

        let partial: UpgradeIdentity =
            serde_json::from_str(r#"{"protocol_version":1,"version":"0.1.7"}"#).unwrap();
        assert_eq!(partial.protocol_version, Some(1));
        assert!(partial.git_commit.is_none());
        assert_eq!(
            serde_json::to_value(partial).unwrap(),
            serde_json::json!({"protocol_version": 1, "version": "0.1.7"})
        );

        let changed = UpgradeIdentityReason::FieldsChanged {
            fields: vec![
                UpgradeIdentityField::GitCommit,
                UpgradeIdentityField::CargoLockSha256,
            ],
        };
        assert_eq!(
            serde_json::to_value(changed).unwrap(),
            serde_json::json!({
                "kind": "fields_changed",
                "fields": ["git_commit", "cargo_lock_sha256"],
            })
        );
    }

    fn clear_field(identity: &mut UpgradeIdentity, field: UpgradeIdentityField) {
        match field {
            UpgradeIdentityField::ProtocolVersion => identity.protocol_version = None,
            UpgradeIdentityField::Version => identity.version = None,
            UpgradeIdentityField::GitCommit => identity.git_commit = None,
            UpgradeIdentityField::Profile => identity.profile = None,
            UpgradeIdentityField::CargoLockSha256 => identity.cargo_lock_sha256 = None,
            UpgradeIdentityField::ArtifactManifestSha256 => {
                identity.artifact_manifest_sha256 = None;
            }
        }
    }

    fn assert_unknown_on_side(
        comparison: &UpgradeIdentityComparison,
        field: UpgradeIdentityField,
        running_side: bool,
    ) {
        assert_eq!(comparison.status, UpgradeIdentityStatus::Unknown);
        let [UpgradeIdentityReason::MissingFields { running, staged }] =
            comparison.reasons.as_slice()
        else {
            panic!("expected missing-fields reason");
        };
        if running_side {
            assert_eq!(running, &[field]);
            assert!(staged.is_empty());
        } else {
            assert!(running.is_empty());
            assert_eq!(staged, &[field]);
        }
    }
}
