//! Language-1 public value model (`plan/design-rh-standalone-product.md` D20).
//!
//! Frozen surface. Adding or removing a variant is a Language 2 / crate-major
//! change. There is deliberately no `f64`, no `Json`, no `Char`, no `Set`.

/// A Language-1 value.
///
/// `Map` is insertion-ordered. `Host` carries an opaque handle into the host's
/// object table; the handle is crate-private on purpose so embedders cannot
/// forge one.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Unit,
    Bool(bool),
    Int(i64),
    String(String),
    Array(Vec<Value>),
    Map(Vec<(String, Value)>),
    Bytes(Vec<u8>),
    Host(HostObject),
}

/// An opaque handle to a host-owned object (`std.fs.Metadata`,
/// `std.process.Command`, …). `type_id` is one of the `'static` strings from
/// the Language-1 value-model table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostObject {
    type_id: &'static str,
    handle: u64,
}

impl HostObject {
    // Constructed by `StdHost`'s object table in PR-A3; the type and its
    // accessors are frozen surface now so the value model does not move later.
    #[allow(dead_code)]
    pub(crate) fn new(type_id: &'static str, handle: u64) -> Self {
        Self { type_id, handle }
    }

    /// The frozen `type_id` string, e.g. `"std.fs.Metadata"`.
    pub fn type_id(&self) -> &'static str {
        self.type_id
    }

    #[allow(dead_code)]
    pub(crate) fn handle(&self) -> u64 {
        self.handle
    }
}

impl Value {
    /// The Language-1 type name, used in error messages and `type_of`.
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Unit => "()",
            Self::Bool(..) => "bool",
            Self::Int(..) => "int",
            Self::String(..) => "string",
            Self::Array(..) => "array",
            Self::Map(..) => "map",
            Self::Bytes(..) => "bytes",
            Self::Host(object) => object.type_id(),
        }
    }

    /// Truthiness as Language 1 defines it: only `Bool` is a condition.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_int(&self) -> Option<i64> {
        match self {
            Self::Int(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(value) => Some(value.as_str()),
            _ => None,
        }
    }
}

/// Map an entry value to a process exit code.
///
/// `i32::try_from` then `u8::try_from`, else 1 — the same shape as the live
/// `script_exit_code` (`src/script_rh_cli_main.rs`), with an explicit i64→i32
/// step because AOT `rh_entry` is `i64` (design D21).
pub fn exit_from_int(value: i64) -> u8 {
    i32::try_from(value)
        .ok()
        .and_then(|narrowed| u8::try_from(narrowed).ok())
        .unwrap_or(1)
}

#[cfg(test)]
mod tests {
    use super::{Value, exit_from_int};

    #[test]
    fn exit_code_matches_script_exit_code_shape() {
        assert_eq!(exit_from_int(0), 0);
        assert_eq!(exit_from_int(3), 3);
        assert_eq!(exit_from_int(255), 255);
        // Out of u8 range -> FAILURE (1), never a silent truncation to 0.
        assert_eq!(exit_from_int(256), 1);
        assert_eq!(exit_from_int(-1), 1);
        assert_eq!(exit_from_int(i64::from(i32::MAX) + 1), 1);
    }

    #[test]
    fn value_is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<Value>();
    }

    #[test]
    fn type_names_are_language_1() {
        assert_eq!(Value::Unit.type_name(), "()");
        assert_eq!(Value::Int(1).type_name(), "int");
        assert_eq!(Value::Bytes(vec![]).type_name(), "bytes");
    }
}
