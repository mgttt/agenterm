use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RhError {
    Parse(String),
    Subset { code: &'static str, detail: String },
    Transpile(String),
    Compile(String),
}

impl fmt::Display for RhError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(message) => write!(f, "rh parse error: {message}"),
            Self::Subset { code, detail } => write!(f, "rh subset [{code}]: {detail}"),
            Self::Transpile(message) => write!(f, "rh transpile error: {message}"),
            Self::Compile(message) => write!(f, "rh compile error: {message}"),
        }
    }
}

impl std::error::Error for RhError {}
