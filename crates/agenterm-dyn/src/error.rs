use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DynError {
    #[error("parse error: {0}")]
    Parse(String),
    #[error("unknown variable: {0}")]
    UnknownVar(String),
    #[error("binding name must not be empty")]
    InvalidBindingName,
    #[error("name contains interior NUL")]
    NameContainsNul,
    #[error("name exceeds {limit}-byte UTF-8 limit")]
    NameTooLong { limit: usize },
    #[error("native dlcall requires unsafe Dyn::eval_native")]
    NativeRequiresUnsafe,
    #[error("state limit reached for {resource}: {limit}")]
    StateLimit {
        resource: &'static str,
        limit: usize,
    },
    #[error("arity mismatch in special form `{form}`: expected {expected}, got {got}")]
    Arity {
        form: &'static str,
        expected: usize,
        got: usize,
    },
    #[error("unknown special form: {0}")]
    UnknownForm(String),
    #[error("type error: {0}")]
    Type(String),
    #[error("dlcall failed: {0}")]
    DlCall(String),
    #[error("library error: {0}")]
    Library(String),
}
