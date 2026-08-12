//! macOS runtime defaults.

use std::{io, path::PathBuf};

use crate::contract::runtime::TerminalShellDescriptor;

pub fn application_arguments() -> io::Result<Vec<String>> {
    std::env::args_os()
        .skip(1)
        .map(|argument| {
            argument.into_string().map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "macOS supplied invalid UTF-8 arguments",
                )
            })
        })
        .collect()
}

pub fn user_config_directory() -> io::Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|home| home.join(".config"))
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "HOME is unavailable"))
}

pub fn default_terminal_shell() -> String {
    std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into())
}

pub const fn primary_terminal_shell() -> TerminalShellDescriptor {
    TerminalShellDescriptor {
        id: "zsh",
        label: "zsh",
        program: "/bin/zsh",
    }
}

/// The locale `LANG` value AgenTerm injects into terminal children when the
/// GUI process environment carries no locale at all (the Finder/Dock launch
/// default). Terminal.app and iTerm2 do the same, and without it shells fall
/// back to the C locale and UTF-8-requiring tools such as mosh refuse to run.
///
/// Returns `None` when the environment already declares `LANG` or `LC_ALL`,
/// or when no valid UTF-8 locale can be derived.
pub fn preferred_terminal_lang() -> Option<String> {
    if std::env::var_os("LANG").is_some() || std::env::var_os("LC_ALL").is_some() {
        return None;
    }
    static DERIVED: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    DERIVED
        .get_or_init(|| {
            apple_locale()
                .and_then(|locale| utf8_locale_candidate(&locale))
                .or_else(|| Some("en_US.UTF-8".to_owned()))
        })
        .clone()
}

fn apple_locale() -> Option<String> {
    let output = std::process::Command::new("defaults")
        .args(["read", "-g", "AppleLocale"])
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .filter(|value| !value.is_empty())
}

/// Maps an `AppleLocale` value such as `zh-Hans_HK` or `en_GB@rg=hkzzzz` to an
/// installed `<language>_<REGION>.UTF-8` locale name.
fn utf8_locale_candidate(apple_locale: &str) -> Option<String> {
    let base = apple_locale.split('@').next()?;
    let (language_part, region) = base.split_once('_').unwrap_or((base, ""));
    let language = language_part.split('-').next()?;
    let candidate = if region.is_empty() {
        format!("{language}.UTF-8")
    } else {
        format!("{language}_{region}.UTF-8")
    };
    std::path::Path::new("/usr/share/locale")
        .join(&candidate)
        .exists()
        .then_some(candidate)
}

#[cfg(test)]
mod locale_tests {
    use super::utf8_locale_candidate;

    #[test]
    fn apple_locales_map_to_installed_utf8_locales() {
        assert_eq!(
            utf8_locale_candidate("zh-Hans_HK").as_deref(),
            Some("zh_HK.UTF-8")
        );
        assert_eq!(
            utf8_locale_candidate("en_US@rg=hkzzzz").as_deref(),
            Some("en_US.UTF-8")
        );
        assert_eq!(utf8_locale_candidate("tlh_QO"), None);
    }
}
