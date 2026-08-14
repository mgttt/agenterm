use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Value {
    Nil,
    Int(i64),
    Ptr(usize),
}

impl Value {
    pub fn is_truthy(self) -> bool {
        match self {
            Self::Nil => false,
            Self::Int(n) => n != 0,
            Self::Ptr(p) => p != 0,
        }
    }

    pub fn as_int(self) -> Result<i64, String> {
        match self {
            Self::Int(n) => Ok(n),
            other => Err(format!("expected int, got {other:?}")),
        }
    }

    pub fn as_ptr(self) -> Result<usize, String> {
        match self {
            Self::Ptr(p) => Ok(p),
            Self::Int(n) => u64::try_from(n)
                .map(|v| v as usize)
                .map_err(|_| format!("integer {n} does not fit in a pointer")),
            Self::Nil => Err("expected pointer, got nil".to_owned()),
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Nil => write!(f, "nil"),
            Self::Int(n) => write!(f, "{n}"),
            Self::Ptr(p) => write!(f, "#x{p:x}"),
        }
    }
}
