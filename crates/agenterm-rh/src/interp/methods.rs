//! Core-type operators and methods — **interpreter builtins, not
//! `Host::call`** (Language 1 §3).
//!
//! `String` / `Array` / `Map` / `Bytes` are core `Value` variants, so a
//! program that only manipulates values must run correctly on a host that
//! implements nothing. Routing `s.trim()` through `Host::call` would make the
//! value model unusable in a sandbox, so it is implemented here.
//!
//! Names are taken from the live language, not invented: `transpile.rs`
//! `is_stringish_method_name` / `is_json_method_name`, plus the `Bytes.*` rows
//! of `shipped_surfaces.rs`.

use crate::ir::{BinOp, UnOp};
use crate::lang_error::Error;
use crate::value::Value;

pub(crate) fn display(value: &Value) -> String {
    match value {
        Value::Unit => String::new(),
        Value::Bool(v) => v.to_string(),
        Value::Int(v) => v.to_string(),
        Value::String(v) => v.clone(),
        Value::Bytes(bytes) => String::from_utf8_lossy(bytes).into_owned(),
        Value::Array(items) => {
            let parts: Vec<String> = items.iter().map(display).collect();
            format!("[{}]", parts.join(", "))
        }
        Value::Map(entries) => {
            let parts: Vec<String> = entries
                .iter()
                .map(|(key, value)| format!("{key}: {}", display(value)))
                .collect();
            format!("#{{{}}}", parts.join(", "))
        }
        Value::Host(object) => format!("<{}>", object.type_id()),
    }
}

pub(crate) fn debug(value: &Value) -> String {
    match value {
        Value::String(v) => format!("{v:?}"),
        Value::Unit => "()".to_owned(),
        other => display(other),
    }
}

pub(crate) fn unary(op: UnOp, value: &Value) -> Result<Value, Error> {
    match (op, value) {
        (UnOp::Neg, Value::Int(v)) => Ok(Value::Int(-v)),
        (UnOp::Not, Value::Bool(v)) => Ok(Value::Bool(!v)),
        _ => Err(Error::runtime(format!(
            "cannot apply {op:?} to {}",
            value.type_name()
        ))),
    }
}

pub(crate) fn binary(op: BinOp, lhs: &Value, rhs: &Value) -> Result<Value, Error> {
    use BinOp::*;
    match op {
        Eq | Ne => {
            if let Some(other) = bool_against_something_else(lhs, rhs) {
                return Err(Error::runtime(format!(
                    "cannot compare bool with {other}; write `if condition` or \
                     `if !condition` rather than comparing it to a value of \
                     another type"
                )));
            }
            let same = equals(lhs, rhs);
            return Ok(Value::Bool(if op == Eq { same } else { !same }));
        }
        _ => {}
    }
    // String concatenation: `+` with a string on either side stringifies the
    // other operand, matching the live language's `"scale:" + n` shape.
    if matches!(op, Add) && matches!(lhs, Value::String(_)) {
        return Ok(Value::String(format!("{}{}", display(lhs), display(rhs))));
    }
    if matches!(op, Add) && matches!(rhs, Value::String(_)) {
        return Ok(Value::String(format!("{}{}", display(lhs), display(rhs))));
    }
    if matches!(op, Add)
        && let (Value::Array(a), Value::Array(b)) = (lhs, rhs)
    {
        let mut out = a.clone();
        out.extend(b.iter().cloned());
        return Ok(Value::Array(out));
    }
    let (a, b) = match (lhs.as_int(), rhs.as_int()) {
        (Some(a), Some(b)) => (a, b),
        _ => {
            return Err(Error::runtime(format!(
                "cannot apply {op:?} to {} and {}",
                lhs.type_name(),
                rhs.type_name()
            )));
        }
    };
    Ok(match op {
        Add => Value::Int(a.wrapping_add(b)),
        Sub => Value::Int(a.wrapping_sub(b)),
        Mul => Value::Int(a.wrapping_mul(b)),
        Div => {
            if b == 0 {
                return Err(Error::runtime("division by zero"));
            }
            Value::Int(a.wrapping_div(b))
        }
        Rem => {
            if b == 0 {
                return Err(Error::runtime("modulo by zero"));
            }
            Value::Int(a.wrapping_rem(b))
        }
        Lt => Value::Bool(a < b),
        Le => Value::Bool(a <= b),
        Gt => Value::Bool(a > b),
        Ge => Value::Bool(a >= b),
        Eq | Ne => unreachable!("handled above"),
    })
}

/// Structural equality. `Unit == Unit` is true, which is how the live
/// `json-null-eq-probe.rh` shape reads.
fn equals(lhs: &Value, rhs: &Value) -> bool {
    lhs == rhs
}

/// The type on the other side, when exactly one side is a bool.
///
/// `condition == 0` is how a guard was written when a bool was an int, and
/// answering `false` makes the guard never fire. `"a" == 1` is still false:
/// a string genuinely is not an int. Only bool has a truthy reading waiting
/// to be assumed.
fn bool_against_something_else<'a>(lhs: &'a Value, rhs: &'a Value) -> Option<&'static str> {
    match (lhs, rhs) {
        (Value::Bool(_), Value::Bool(_)) => None,
        (Value::Bool(_), other) | (other, Value::Bool(_)) => Some(other.type_name()),
        _ => None,
    }
}

pub(crate) fn index(base: &Value, index: &Value) -> Result<Value, Error> {
    match (base, index) {
        (Value::Array(items), Value::Int(i)) => Ok(items
            .get(usize::try_from(*i).map_err(|_| Error::runtime("negative array index"))?)
            .cloned()
            .unwrap_or(Value::Unit)),
        (Value::Map(entries), Value::String(key)) => Ok(entries
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
            .unwrap_or(Value::Unit)),
        (Value::Bytes(bytes), Value::Int(i)) => Ok(bytes
            .get(usize::try_from(*i).map_err(|_| Error::runtime("negative bytes index"))?)
            .map(|byte| Value::Int(i64::from(*byte)))
            .unwrap_or(Value::Unit)),
        (Value::String(text), Value::Int(i)) => {
            let i = usize::try_from(*i).map_err(|_| Error::runtime("negative string index"))?;
            Ok(text
                .chars()
                .nth(i)
                .map(|c| Value::String(c.to_string()))
                .unwrap_or(Value::Unit))
        }
        _ => Err(Error::runtime(format!(
            "cannot index {} with {}",
            base.type_name(),
            index.type_name()
        ))),
    }
}

/// `base.name` as a *property* read. `len` is reachable both as a property and
/// as a method, as it is today.
pub(crate) fn field(base: &Value, name: &str) -> Result<Value, Error> {
    if name == "len" {
        return length(base);
    }
    match base {
        Value::Map(entries) => Ok(entries
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.clone())
            .unwrap_or(Value::Unit)),
        _ => Err(Error::runtime(format!(
            "{} has no property `{name}`",
            base.type_name()
        ))),
    }
}

fn length(value: &Value) -> Result<Value, Error> {
    let len = match value {
        Value::Array(items) => items.len(),
        Value::Map(entries) => entries.len(),
        Value::Bytes(bytes) => bytes.len(),
        Value::String(text) => text.chars().count(),
        other => {
            return Err(Error::runtime(format!(
                "{} has no `len`",
                other.type_name()
            )));
        }
    };
    Ok(Value::Int(i64::try_from(len).unwrap_or(i64::MAX)))
}

/// Write through an index or field, returning the updated container.
pub(crate) fn set_index(
    base: Value,
    index: &Value,
    op: Option<BinOp>,
    value: Value,
) -> Result<Value, Error> {
    match base {
        Value::Array(mut items) => {
            let i = index
                .as_int()
                .ok_or_else(|| Error::runtime("array index must be an int"))?;
            let i = usize::try_from(i).map_err(|_| Error::runtime("negative array index"))?;
            if i >= items.len() {
                return Err(Error::runtime("array index out of bounds"));
            }
            items[i] = combine(op, &items[i], value)?;
            Ok(Value::Array(items))
        }
        Value::Map(mut entries) => {
            let key = match index {
                Value::String(key) => key.clone(),
                other => display(other),
            };
            match entries.iter_mut().find(|(k, _)| *k == key) {
                Some(slot) => slot.1 = combine(op, &slot.1.clone(), value)?,
                None => {
                    let seeded = combine(op, &Value::Unit, value)?;
                    entries.push((key, seeded));
                }
            }
            Ok(Value::Map(entries))
        }
        other => Err(Error::runtime(format!(
            "cannot assign into {}",
            other.type_name()
        ))),
    }
}

fn combine(op: Option<BinOp>, current: &Value, value: Value) -> Result<Value, Error> {
    match op {
        Some(op) => binary(op, current, &value),
        None => Ok(value),
    }
}

/// Methods that mutate their receiver in place rather than producing a new
/// value. `a.push(x)` as a statement must actually grow `a`.
pub(crate) fn is_mutating(name: &str) -> bool {
    matches!(name, "push" | "insert" | "append")
}

/// Apply a mutating method, returning `(result, updated_receiver)`.
pub(crate) fn call_mutating(
    receiver: &Value,
    name: &str,
    args: &[Value],
) -> Result<(Value, Value), Error> {
    match (receiver, name, args.len()) {
        (Value::Array(items), "push", 1) => {
            let mut out = items.clone();
            out.push(args[0].clone());
            Ok((Value::Unit, Value::Array(out)))
        }
        (Value::Array(items), "insert", 2) => {
            let at = args[0]
                .as_int()
                .and_then(|i| usize::try_from(i).ok())
                .ok_or_else(|| Error::runtime("`insert` expects a non-negative index"))?;
            let mut out = items.clone();
            if at > out.len() {
                return Err(Error::runtime("`insert` index out of bounds"));
            }
            out.insert(at, args[1].clone());
            Ok((Value::Unit, Value::Array(out)))
        }
        (Value::Map(_), "insert", 2) => {
            let updated = call_method(receiver, "insert", args)?;
            Ok((Value::Unit, updated))
        }
        (Value::Bytes(_), "append", 1) => {
            let updated = call_method(receiver, "append", args)?;
            Ok((Value::Unit, updated))
        }
        _ => Err(Error::unsupported_name(&format!(
            "{}.{name}",
            receiver.type_name()
        ))),
    }
}

/// The frozen core-type method surface.
pub(crate) fn call_method(receiver: &Value, name: &str, args: &[Value]) -> Result<Value, Error> {
    if name == "len" && args.is_empty() {
        return length(receiver);
    }
    match receiver {
        Value::String(text) => string_method(text, name, args),
        Value::Array(items) => array_method(items, name, args),
        Value::Map(entries) => map_method(entries, name, args),
        Value::Bytes(bytes) => bytes_method(bytes, name, args),
        other => Err(Error::unsupported_name(&format!(
            "{}.{name}",
            other.type_name()
        ))),
    }
}

fn arg_str<'a>(args: &'a [Value], index: usize, method: &str) -> Result<&'a str, Error> {
    args.get(index)
        .and_then(Value::as_str)
        .ok_or_else(|| Error::runtime(format!("`{method}` expects a string argument")))
}

fn string_method(text: &str, name: &str, args: &[Value]) -> Result<Value, Error> {
    Ok(match (name, args.len()) {
        ("contains", 1) => Value::Bool(text.contains(arg_str(args, 0, name)?)),
        ("starts_with", 1) => Value::Bool(text.starts_with(arg_str(args, 0, name)?)),
        ("ends_with", 1) => Value::Bool(text.ends_with(arg_str(args, 0, name)?)),
        ("trim", 0) => Value::String(text.trim().to_owned()),
        ("to_lower", 0) => Value::String(text.to_lowercase()),
        ("to_upper", 0) => Value::String(text.to_uppercase()),
        ("to_string", 0) => Value::String(text.to_owned()),
        ("replace", 2) => {
            Value::String(text.replace(arg_str(args, 0, name)?, arg_str(args, 1, name)?))
        }
        ("split", 1) => Value::Array(
            text.split(arg_str(args, 0, name)?)
                .map(|part| Value::String(part.to_owned()))
                .collect(),
        ),
        ("sub_string", 2) => {
            let start = args[0]
                .as_int()
                .ok_or_else(|| Error::runtime("`sub_string` expects int bounds"))?;
            let count = args[1]
                .as_int()
                .ok_or_else(|| Error::runtime("`sub_string` expects int bounds"))?;
            let start = usize::try_from(start).unwrap_or(0);
            let count = usize::try_from(count).unwrap_or(0);
            Value::String(text.chars().skip(start).take(count).collect())
        }
        ("index_of", 1) => {
            let needle = arg_str(args, 0, name)?;
            match text.find(needle) {
                Some(byte_index) => {
                    Value::Int(i64::try_from(text[..byte_index].chars().count()).unwrap_or(-1))
                }
                None => Value::Int(-1),
            }
        }
        ("parse_int", 0) => match text.trim().parse::<i64>() {
            Ok(value) => Value::Int(value),
            Err(_) => return Err(Error::runtime(format!("`{text}` is not an int"))),
        },
        _ => return Err(Error::unsupported_name(&format!("String.{name}"))),
    })
}

fn array_method(items: &[Value], name: &str, args: &[Value]) -> Result<Value, Error> {
    Ok(match (name, args.len()) {
        ("get", 1) => index(&Value::Array(items.to_vec()), &args[0])?,
        ("contains", 1) => Value::Bool(items.contains(&args[0])),
        _ => return Err(Error::unsupported_name(&format!("Array.{name}"))),
    })
}

fn map_method(entries: &[(String, Value)], name: &str, args: &[Value]) -> Result<Value, Error> {
    Ok(match (name, args.len()) {
        ("keys", 0) => Value::Array(
            entries
                .iter()
                .map(|(key, _)| Value::String(key.clone()))
                .collect(),
        ),
        ("values", 0) => Value::Array(entries.iter().map(|(_, value)| value.clone()).collect()),
        ("contains", 1) => {
            let key = arg_str(args, 0, name)?;
            Value::Bool(entries.iter().any(|(k, _)| k == key))
        }
        ("get", 1) => index(&Value::Map(entries.to_vec()), &args[0])?,
        ("insert", 2) => {
            let key = arg_str(args, 0, name)?.to_owned();
            let mut out = entries.to_vec();
            match out.iter_mut().find(|(k, _)| *k == key) {
                Some(slot) => slot.1 = args[1].clone(),
                None => out.push((key, args[1].clone())),
            }
            Value::Map(out)
        }
        _ => return Err(Error::unsupported_name(&format!("Map.{name}"))),
    })
}

fn bytes_method(bytes: &[u8], name: &str, args: &[Value]) -> Result<Value, Error> {
    Ok(match (name, args.len()) {
        ("to_text", 0) => Value::String(String::from_utf8_lossy(bytes).into_owned()),
        ("get", 1) => index(&Value::Bytes(bytes.to_vec()), &args[0])?,
        ("slice", 2) => {
            let start = args[0].as_int().unwrap_or(0).max(0) as usize;
            let count = args[1].as_int().unwrap_or(0).max(0) as usize;
            Value::Bytes(bytes.iter().skip(start).take(count).copied().collect())
        }
        ("append", 1) => {
            let mut out = bytes.to_vec();
            match &args[0] {
                Value::Bytes(extra) => out.extend_from_slice(extra),
                Value::Int(byte) => out.push(*byte as u8),
                Value::String(text) => out.extend_from_slice(text.as_bytes()),
                other => {
                    return Err(Error::runtime(format!(
                        "cannot append {} to bytes",
                        other.type_name()
                    )));
                }
            }
            Value::Bytes(out)
        }
        _ => return Err(Error::unsupported_name(&format!("Bytes.{name}"))),
    })
}
