use crate::Dyn;
use crate::error::DynError;
use crate::native;
use crate::parse::SExpr;
use crate::value::Value;

/// Hard cap for `(repeat N …)` — unbounded loops are intentionally unsupported.
pub const REPEAT_MAX: i64 = 1_000_000;

pub(crate) fn eval_expr(env: &mut Dyn, expr: &SExpr) -> Result<Value, DynError> {
    match expr {
        SExpr::Int(n) => Ok(Value::Int(*n)),
        SExpr::Str(s) => Err(DynError::Type(format!(
            "bare string literal `{s}` is not a value; use it inside dlcall"
        ))),
        SExpr::Sym(name) => env
            .bindings
            .get(name)
            .copied()
            .ok_or_else(|| DynError::UnknownVar(name.clone())),
        SExpr::List(items) => eval_list(env, items),
    }
}

fn eval_list(env: &mut Dyn, items: &[SExpr]) -> Result<Value, DynError> {
    let head = items
        .first()
        .ok_or_else(|| DynError::Parse("empty list".into()))?;
    let SExpr::Sym(form) = head else {
        return Err(DynError::Parse(
            "application of non-symbol head is not supported".into(),
        ));
    };
    match form.as_str() {
        "do" => eval_do(env, &items[1..]),
        "set" => eval_set(env, &items[1..]),
        "if" => eval_if(env, &items[1..]),
        "dlcall" => native::eval_dlcall(env, &items[1..]),
        "=" => eval_cmp(env, "=", &items[1..], |a, b| a == b),
        "<" => eval_cmp(env, "<", &items[1..], |a, b| a < b),
        ">" => eval_cmp(env, ">", &items[1..], |a, b| a > b),
        "<=" => eval_cmp(env, "<=", &items[1..], |a, b| a <= b),
        ">=" => eval_cmp(env, ">=", &items[1..], |a, b| a >= b),
        "not" => eval_not(env, &items[1..]),
        "and" => eval_and(env, &items[1..]),
        "or" => eval_or(env, &items[1..]),
        "+" => eval_add(env, &items[1..]),
        "-" => eval_sub(env, &items[1..]),
        "repeat" => eval_repeat(env, &items[1..]),
        other => Err(DynError::UnknownForm(other.to_owned())),
    }
}

fn expect_int(value: Value, context: &str) -> Result<i64, DynError> {
    value
        .as_int()
        .map_err(|e| DynError::Type(format!("{context}: {e}")))
}

fn bool_value(cond: bool) -> Value {
    Value::Int(i64::from(cond))
}

fn eval_do(env: &mut Dyn, body: &[SExpr]) -> Result<Value, DynError> {
    let mut last = Value::Nil;
    for expr in body {
        last = eval_expr(env, expr)?;
    }
    Ok(last)
}

fn eval_set(env: &mut Dyn, args: &[SExpr]) -> Result<Value, DynError> {
    if args.len() != 2 {
        return Err(DynError::Arity {
            form: "set",
            expected: 2,
            got: args.len(),
        });
    }
    let SExpr::Sym(name) = &args[0] else {
        return Err(DynError::Type("set target must be a symbol".into()));
    };
    // Do this before evaluating the RHS: a full environment must not permit
    // an expression's side effects merely because its target is a new name.
    Dyn::ensure_name(name)?;
    env.ensure_binding_capacity(name)?;
    let value = eval_expr(env, &args[1])?;
    env.bindings.insert(name.clone(), value);
    Ok(value)
}

fn eval_if(env: &mut Dyn, args: &[SExpr]) -> Result<Value, DynError> {
    if args.len() != 3 {
        return Err(DynError::Arity {
            form: "if",
            expected: 3,
            got: args.len(),
        });
    }
    let cond = eval_expr(env, &args[0])?;
    if cond.is_truthy() {
        eval_expr(env, &args[1])
    } else {
        eval_expr(env, &args[2])
    }
}

fn eval_cmp(
    env: &mut Dyn,
    form: &'static str,
    args: &[SExpr],
    op: impl FnOnce(i64, i64) -> bool,
) -> Result<Value, DynError> {
    if args.len() != 2 {
        return Err(DynError::Arity {
            form,
            expected: 2,
            got: args.len(),
        });
    }
    let left = expect_int(eval_expr(env, &args[0])?, "comparison left operand")?;
    let right = expect_int(eval_expr(env, &args[1])?, "comparison right operand")?;
    Ok(bool_value(op(left, right)))
}

fn eval_and(env: &mut Dyn, args: &[SExpr]) -> Result<Value, DynError> {
    if args.is_empty() {
        return Err(DynError::Arity {
            form: "and",
            expected: 1,
            got: 0,
        });
    }
    let mut last = Value::Nil;
    for arg in args {
        last = eval_expr(env, arg)?;
        if !last.is_truthy() {
            return Ok(last);
        }
    }
    Ok(last)
}

fn eval_or(env: &mut Dyn, args: &[SExpr]) -> Result<Value, DynError> {
    if args.is_empty() {
        return Err(DynError::Arity {
            form: "or",
            expected: 1,
            got: 0,
        });
    }
    let mut last = Value::Nil;
    for arg in args {
        last = eval_expr(env, arg)?;
        if last.is_truthy() {
            return Ok(last);
        }
    }
    Ok(last)
}

fn eval_not(env: &mut Dyn, args: &[SExpr]) -> Result<Value, DynError> {
    if args.len() != 1 {
        return Err(DynError::Arity {
            form: "not",
            expected: 1,
            got: args.len(),
        });
    }
    Ok(bool_value(!eval_expr(env, &args[0])?.is_truthy()))
}

fn eval_add(env: &mut Dyn, args: &[SExpr]) -> Result<Value, DynError> {
    if args.is_empty() {
        return Err(DynError::Arity {
            form: "+",
            expected: 1,
            got: 0,
        });
    }
    let mut sum = 0_i64;
    for arg in args {
        let n = expect_int(eval_expr(env, arg)?, "+ operand")?;
        sum = sum
            .checked_add(n)
            .ok_or_else(|| DynError::Type("integer overflow in +".into()))?;
    }
    Ok(Value::Int(sum))
}

fn eval_sub(env: &mut Dyn, args: &[SExpr]) -> Result<Value, DynError> {
    if args.is_empty() {
        return Err(DynError::Arity {
            form: "-",
            expected: 1,
            got: 0,
        });
    }
    let first = expect_int(eval_expr(env, &args[0])?, "- operand")?;
    if args.len() == 1 {
        return first
            .checked_neg()
            .map(Value::Int)
            .ok_or_else(|| DynError::Type("integer overflow in unary -".into()));
    }
    let mut acc = first;
    for arg in &args[1..] {
        let n = expect_int(eval_expr(env, arg)?, "- operand")?;
        acc = acc
            .checked_sub(n)
            .ok_or_else(|| DynError::Type("integer overflow in -".into()))?;
    }
    Ok(Value::Int(acc))
}

fn eval_repeat(env: &mut Dyn, args: &[SExpr]) -> Result<Value, DynError> {
    if args.len() != 2 {
        return Err(DynError::Arity {
            form: "repeat",
            expected: 2,
            got: args.len(),
        });
    }
    let count = expect_int(eval_expr(env, &args[0])?, "repeat count")?;
    if count < 0 {
        return Err(DynError::Type(
            "repeat count must be a non-negative integer".into(),
        ));
    }
    if count > REPEAT_MAX {
        return Err(DynError::Type(format!(
            "repeat count {count} exceeds hard cap {REPEAT_MAX}"
        )));
    }
    let count_usize = usize::try_from(count)
        .map_err(|_| DynError::Type("repeat count does not fit in usize".into()))?;
    let mut last = Value::Nil;
    for _ in 0..count_usize {
        last = eval_expr(env, &args[1])?;
    }
    Ok(last)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeat_zero_is_nil() {
        let mut env = Dyn::new();
        let v = env.eval("(repeat 0 99)").expect("repeat 0");
        assert_eq!(v, Value::Nil);
    }
}
