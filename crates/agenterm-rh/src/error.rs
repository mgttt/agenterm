use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RhError {
    Parse(String),
    Subset {
        code: &'static str,
        detail: String,
    },
    Transpile(String),
    Compile(String),
    /// A script failed while running, as opposed to while being checked or
    /// built. Reporting these as compile errors was misleading now that
    /// `eval` / `run` execute on the interpreter and never invoke a compiler.
    Runtime(String),
}

impl fmt::Display for RhError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(message) => write!(f, "rh parse error: {message}"),
            Self::Subset { code, detail } => write!(f, "rh subset [{code}]: {detail}"),
            Self::Transpile(message) => write!(f, "rh transpile error: {message}"),
            Self::Compile(message) => write!(f, "rh compile error: {message}"),
            Self::Runtime(message) => write!(f, "rh runtime: {message}"),
        }
    }
}

impl std::error::Error for RhError {}
