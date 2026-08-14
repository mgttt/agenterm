use crate::Dyn;
use crate::error::DynError;
use crate::native;
use crate::parse::SExpr;
use crate::value::Value;

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
        other => Err(DynError::UnknownForm(other.to_owned())),
    }
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
