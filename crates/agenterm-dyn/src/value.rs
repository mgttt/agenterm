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
            Self::Int(n) => {
                let value = u64::try_from(n)
                    .map_err(|_| format!("integer {n} does not fit in a pointer"))?;
                usize::try_from(value).map_err(|_| format!("integer {n} does not fit in a pointer"))
            }
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

#[cfg(test)]
mod tests {
    use super::Value;

    #[test]
    fn integer_pointer_conversion_keeps_representable_values() {
        assert_eq!(Value::Int(42).as_ptr(), Ok(42));
    }

    #[cfg(target_pointer_width = "32")]
    #[test]
    fn integer_pointer_conversion_rejects_values_wider_than_usize() {
        assert_eq!(
            Value::Int(4_294_967_297).as_ptr(),
            Err("integer 4294967297 does not fit in a pointer".to_owned())
        );
    }

    #[cfg(target_pointer_width = "64")]
    #[test]
    fn integer_pointer_conversion_accepts_u32_plus_one_on_64_bit_targets() {
        assert_eq!(Value::Int(4_294_967_297).as_ptr(), Ok(4_294_967_297));
    }
}
