use rhai::{Expr, Stmt};

use crate::RhError;

#[derive(Clone, Debug)]
pub struct FleetCall<'a> {
    pub operation_id: String,
    pub args: &'a [Expr],
}

pub fn parse_fleet_call(expr: &Expr) -> Option<FleetCall<'_>> {
    let Expr::Dot(boxed, ..) = expr else {
        return None;
    };
    if !matches!(&boxed.lhs, Expr::Variable(ident, ..) if ident.1 == "fleet") {
        return None;
    }
    parse_fleet_tail(&boxed.rhs)
}

pub fn expr_uses_fleet(expr: &Expr) -> bool {
    parse_fleet_call(expr).is_some() || expr_contains_fleet(expr)
}

fn expr_contains_fleet(expr: &Expr) -> bool {
    match expr {
        Expr::Variable(ident, ..) => ident.1 == "fleet",
        Expr::Dot(boxed, ..) => expr_contains_fleet(&boxed.lhs) || expr_contains_fleet(&boxed.rhs),
        Expr::MethodCall(call, ..) | Expr::FnCall(call, ..) => {
            call.args.iter().any(expr_uses_fleet)
        }
        Expr::Stmt(block) => block.iter().any(stmt_uses_fleet),
        _ => false,
    }
}

fn stmt_uses_fleet(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Expr(expr) => expr_uses_fleet(expr.as_ref()),
        Stmt::FnCall(call, ..) => call.args.iter().any(expr_uses_fleet),
        Stmt::Block(block) => block.iter().any(stmt_uses_fleet),
        _ => false,
    }
}

fn parse_fleet_tail(expr: &Expr) -> Option<FleetCall<'_>> {
    match expr {
        Expr::MethodCall(call, ..) | Expr::FnCall(call, ..) => {
            let operation_id = fleet_method_to_operation_id(&call.name)?;
            Some(FleetCall {
                operation_id,
                args: &call.args,
            })
        }
        Expr::Dot(boxed, ..) => {
            let mut segments = Vec::new();
            collect_property_segments(&boxed.lhs, &mut segments)?;
            let FleetCall {
                mut operation_id,
                args,
            } = parse_fleet_tail(&boxed.rhs)?;
            if let Some(prefix) = segments.pop() {
                operation_id = format!("{prefix}.{operation_id}");
            }
            for segment in segments.into_iter().rev() {
                operation_id = format!("{segment}.{operation_id}");
            }
            Some(FleetCall { operation_id, args })
        }
        _ => None,
    }
}

fn collect_property_segments(expr: &Expr, segments: &mut Vec<String>) -> Option<()> {
    match expr {
        Expr::Property(prop, ..) => {
            segments.push(prop.2.to_string());
            Some(())
        }
        Expr::Dot(boxed, ..) => {
            collect_property_segments(&boxed.lhs, segments)?;
            collect_property_segments(&boxed.rhs, segments)
        }
        _ => None,
    }
}

fn fleet_method_to_operation_id(name: &str) -> Option<String> {
    Some(match name {
        "info" => "info".to_owned(),
        "set_width" => "set-width".to_owned(),
        other if other.contains('_') => other.replace('_', "-"),
        other => other.to_owned(),
    })
}

pub fn validate_fleet_call(call: &FleetCall<'_>) -> Result<(), RhError> {
    if call.args.is_empty() {
        return Ok(());
    }
    if call.args.len() == 1 && matches!(call.args[0], Expr::IntegerConstant(..)) {
        return Ok(());
    }
    Err(RhError::Subset {
        code: "RH_SUBSET_FLEET_ARGS",
        detail: "rh-1 fleet calls support zero args or one integer arg".into(),
    })
}

pub fn fleet_params_json(call: &FleetCall<'_>) -> Result<String, RhError> {
    validate_fleet_call(call)?;
    if call.args.is_empty() {
        return Ok("{}".to_owned());
    }
    let Expr::IntegerConstant(value, ..) = call.args[0] else {
        return Err(RhError::Subset {
            code: "RH_SUBSET_FLEET_ARGS",
            detail: "rh-1 fleet calls support zero args or one integer arg".into(),
        });
    };
    Ok(format!("{{\"width\":{value}}}"))
}

#[cfg(test)]
mod tests {
    use super::{fleet_params_json, parse_fleet_call};
    use rhai::{Engine, Stmt};

    fn parse(source: &str) -> rhai::AST {
        Engine::new().compile(source).expect("compile")
    }

    fn entry_expr(source: &str) -> rhai::Expr {
        let ast = parse(source);
        let stmt = ast
            .iter_fn_def()
            .next()
            .expect("fn")
            .body
            .iter()
            .next()
            .expect("stmt");
        match stmt {
            Stmt::Expr(expr) => expr.as_ref().clone(),
            other => panic!("unexpected stmt: {other:?}"),
        }
    }

    #[test]
    fn parses_protocol_info() {
        let expr = entry_expr("fn entry() { fleet.protocol.info() }");
        let call = parse_fleet_call(&expr).expect("fleet call");
        assert_eq!(call.operation_id, "protocol.info");
        assert_eq!(fleet_params_json(&call).expect("json"), "{}");
    }

    #[test]
    fn parses_ui_tabs_set_width() {
        let expr = entry_expr("fn entry() { fleet.ui.tabs.set_width(240) }");
        let call = parse_fleet_call(&expr).expect("fleet call");
        assert_eq!(call.operation_id, "ui.tabs.set-width");
        assert_eq!(fleet_params_json(&call).expect("json"), "{\"width\":240}");
    }
}
