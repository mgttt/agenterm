//! Script execution backend selection.
//!
//! Today every live invocation uses Rhai. The parallel `rh` track (`crates/agenterm-rh`)
//! validates pack subsets and transpiles to Rust for AOT; flip
//! `AGENTERM_SCRIPT_BACKEND=rh` once native pack loading ships.

/// Active script backend for pack execution.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScriptBackend {
    Rhai,
    Rh,
}

impl ScriptBackend {
    pub fn from_env() -> Self {
        match std::env::var("AGENTERM_SCRIPT_BACKEND")
            .ok()
            .as_deref()
            .map(str::trim)
            .map(str::to_ascii_lowercase)
        {
            Some(value) if value == "rh" => Self::Rh,
            _ => Self::Rhai,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Rhai => "rhai",
            Self::Rh => "rh",
        }
    }
}

pub fn rh_check(source: &str) -> Result<(), agenterm_rh::RhError> {
    agenterm_rh::check(source)
}

pub fn rh_transpile(source: &str) -> Result<String, agenterm_rh::RhError> {
    agenterm_rh::transpile(source)
}

#[cfg(test)]
mod tests {
    use super::ScriptBackend;

    #[test]
    fn default_backend_is_rhai() {
        assert_eq!(ScriptBackend::from_env(), ScriptBackend::Rhai);
    }
}
