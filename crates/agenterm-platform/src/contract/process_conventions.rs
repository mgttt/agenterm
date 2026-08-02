//! Platform-neutral values for host process parameter conventions.

use std::fmt;

/// How an environment-block encoder handles malformed entries.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum InvalidEnvironmentEntryPolicy {
    Reject,
    Skip,
}

impl InvalidEnvironmentEntryPolicy {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Reject => "reject",
            Self::Skip => "skip",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum WindowsCommandLineError {
    EmptyCommand,
    ArgumentContainsNul { index: usize },
}

impl WindowsCommandLineError {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::EmptyCommand => "empty-command",
            Self::ArgumentContainsNul { .. } => "argument-contains-nul",
        }
    }

    #[must_use]
    pub const fn argument_index(self) -> Option<usize> {
        match self {
            Self::EmptyCommand => None,
            Self::ArgumentContainsNul { index } => Some(index),
        }
    }
}

impl fmt::Display for WindowsCommandLineError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyCommand => formatter.write_str("Windows command line has no arguments"),
            Self::ArgumentContainsNul { index } => {
                write!(
                    formatter,
                    "Windows command-line argument {index} contains NUL"
                )
            }
        }
    }
}

impl std::error::Error for WindowsCommandLineError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum WindowsEnvironmentBlockError {
    EmptyKey { index: usize },
    KeyContainsEquals { index: usize },
    KeyContainsNul { index: usize },
    ValueContainsNul { index: usize },
}

impl WindowsEnvironmentBlockError {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::EmptyKey { .. } => "empty-key",
            Self::KeyContainsEquals { .. } => "key-contains-equals",
            Self::KeyContainsNul { .. } => "key-contains-nul",
            Self::ValueContainsNul { .. } => "value-contains-nul",
        }
    }

    #[must_use]
    pub const fn entry_index(self) -> usize {
        match self {
            Self::EmptyKey { index }
            | Self::KeyContainsEquals { index }
            | Self::KeyContainsNul { index }
            | Self::ValueContainsNul { index } => index,
        }
    }
}

impl fmt::Display for WindowsEnvironmentBlockError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyKey { index } => {
                write!(
                    formatter,
                    "Windows environment entry {index} has an empty key"
                )
            }
            Self::KeyContainsEquals { index } => write!(
                formatter,
                "Windows environment entry {index} has '=' in its key"
            ),
            Self::KeyContainsNul { index } => write!(
                formatter,
                "Windows environment entry {index} has NUL in its key"
            ),
            Self::ValueContainsNul { index } => write!(
                formatter,
                "Windows environment entry {index} has NUL in its value"
            ),
        }
    }
}

impl std::error::Error for WindowsEnvironmentBlockError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_parameter_contracts_have_stable_evidence() {
        assert_eq!(InvalidEnvironmentEntryPolicy::Reject.as_str(), "reject");
        assert_eq!(InvalidEnvironmentEntryPolicy::Skip.as_str(), "skip");
        assert_eq!(
            WindowsCommandLineError::EmptyCommand.as_str(),
            "empty-command"
        );
        assert_eq!(
            WindowsCommandLineError::ArgumentContainsNul { index: 2 }.argument_index(),
            Some(2)
        );
        let environment = WindowsEnvironmentBlockError::KeyContainsEquals { index: 3 };
        assert_eq!(environment.as_str(), "key-contains-equals");
        assert_eq!(environment.entry_index(), 3);
        assert!(environment.to_string().contains("entry 3"));
    }
}
