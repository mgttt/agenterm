//! macOS system-font discovery used by the Unix renderer.
//! Adapter-private native mechanism selected only by platform::selected.

#![cfg(target_os = "macos")]

use std::path::PathBuf;

use crate::platform::CapabilityStatus;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct MacosFontCandidate {
    pub name: &'static str,
    pub components: &'static [&'static str],
}

impl MacosFontCandidate {
    pub(crate) fn absolute_path(self) -> PathBuf {
        self.components.iter().fold(
            PathBuf::from(std::path::MAIN_SEPARATOR_STR),
            |path, part| path.join(part),
        )
    }

    pub(crate) fn exists(self) -> bool {
        self.absolute_path().is_file()
    }
}

pub(crate) fn candidates() -> &'static [MacosFontCandidate] {
    &[
        MacosFontCandidate {
            name: "SF Mono",
            components: &["System", "Library", "Fonts", "SFNSMono.ttf"],
        },
        MacosFontCandidate {
            name: "Hiragino Sans GB",
            components: &["System", "Library", "Fonts", "Hiragino Sans GB.ttc"],
        },
        MacosFontCandidate {
            name: "Apple Symbols",
            components: &["System", "Library", "Fonts", "Apple Symbols.ttf"],
        },
    ]
}

pub(crate) fn primary_family_name() -> Option<&'static str> {
    candidates()
        .iter()
        .copied()
        .find(|candidate| candidate.exists())
        .map(|candidate| candidate.name)
}

pub(crate) fn capability_status() -> CapabilityStatus {
    if primary_family_name().is_some() {
        CapabilityStatus::Available
    } else {
        CapabilityStatus::Failed {
            code: "font_unavailable",
            message: "no macOS system font candidate found".to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidate_paths_are_absolute_and_ordered() {
        let candidates = candidates();
        assert_eq!(candidates[0].name, "SF Mono");
        assert!(candidates.iter().all(|candidate| {
            candidate.absolute_path().is_absolute() && !candidate.components.is_empty()
        }));
    }

    #[test]
    fn native_macos_has_a_typed_font_result() {
        assert!(matches!(
            capability_status(),
            CapabilityStatus::Available
                | CapabilityStatus::Failed {
                    code: "font_unavailable",
                    ..
                }
        ));
    }
}
