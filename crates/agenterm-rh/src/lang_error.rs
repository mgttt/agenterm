//! The Language-1 public error type (`plan/design-rh-standalone-product.md`,
//! API / Interface Changes).
//!
//! Distinct from [`crate::RhError`], which is the AgenTerm AOT pipeline's error
//! (parse / subset / transpile / compile). This one is the embedder-facing
//! surface and is `#[non_exhaustive]` so adding a variant is not a break.

use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    Parse(String),
    Subset { code: &'static str, detail: String },
    Runtime(String),
    Host(String),
    Cancelled,
    OutOfFuel,
    Timeout,
    Unsupported { feature: String },
    Io(String),
}

impl Error {
    /// An unsupported frozen feature name (`"compile"`, `"print"`).
    pub fn unsupported(feature: impl Into<String>) -> Self {
        Self::Unsupported {
            feature: feature.into(),
        }
    }

    /// An unsupported *script* name (`std::fs::not_shipped`).
    pub fn unsupported_name(name: &str) -> Self {
        Self::Unsupported {
            feature: name.to_owned(),
        }
    }

    pub fn runtime(message: impl Into<String>) -> Self {
        Self::Runtime(message.into())
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(message) => write!(f, "rh parse error: {message}"),
            Self::Subset { code, detail } => write!(f, "rh subset [{code}]: {detail}"),
            Self::Runtime(message) => write!(f, "rh runtime: {message}"),
            Self::Host(message) => write!(f, "rh host: {message}"),
            Self::Cancelled => write!(f, "rh runtime: cancelled"),
            Self::OutOfFuel => write!(f, "rh runtime: out of fuel"),
            Self::Timeout => write!(f, "rh runtime: timeout"),
            Self::Unsupported { feature } => write!(f, "rh: host {feature} is unsupported"),
            Self::Io(message) => write!(f, "rh io: {message}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<crate::RhError> for Error {
    fn from(error: crate::RhError) -> Self {
        match error {
            crate::RhError::Parse(message) => Self::Parse(message),
            crate::RhError::Subset { code, detail } => Self::Subset { code, detail },
            crate::RhError::Transpile(message) | crate::RhError::Compile(message) => {
                Self::Runtime(message)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Error;

    #[test]
    fn subset_errors_keep_their_code_across_the_boundary() {
        let rh = crate::RhError::Subset {
            code: "RH_SUBSET_NO_EVAL",
            detail: "eval is forbidden in rh".to_owned(),
        };
        let error = Error::from(rh);
        assert!(matches!(
            &error,
            Error::Subset { code, .. } if *code == "RH_SUBSET_NO_EVAL"
        ));
        assert!(error.to_string().contains("RH_SUBSET_NO_EVAL"));
    }

    #[test]
    fn unsupported_renders_the_observability_line() {
        assert_eq!(
            Error::unsupported("compile").to_string(),
            "rh: host compile is unsupported"
        );
    }
}
