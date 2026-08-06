//! Render Rhai AST fragments back to source for host-side evaluation.

use rhai::{Expr, FnCallExpr, Stmt, Token};

use crate::RhError;

pub fn expr_to_rhai(expr: &Expr) -> Result<String, RhError> {
    let mut out = String::new();
    print_expr(&mut out, expr)?;
    Ok(out)
}

pub fn stmt_to_rhai(stmt: &Stmt) -> Result<String, RhError> {
    let mut out = String::new();
    print_stmt(&mut out, stmt, false)?;
    Ok(out)
}

fn print_stmt(out: &mut String, stmt: &Stmt, trailing_semi: bool) -> Result<(), RhError> {
    match stmt {
        Stmt::Var(boxed, ..) => {
            let (ident, expr, _) = boxed.as_ref();
            out.push_str("let ");
            out.push_str(ident.name.as_str());
            out.push_str(" = ");
            print_expr(out, expr)?;
            if trailing_semi {
                out.push(';');
            }
        }
        Stmt::Expr(expr) => {
            print_expr(out, expr.as_ref())?;
            if trailing_semi {
                out.push(';');
            }
        }
        Stmt::Return(Some(expr), ..) => {
            out.push_str("return ");
            print_expr(out, expr.as_ref())?;
            if trailing_semi {
                out.push(';');
            }
        }
        Stmt::Return(None, ..) => {
            out.push_str("return");
            if trailing_semi {
                out.push(';');
            }
        }
        Stmt::If(boxed, ..) => {
            let flow = boxed.as_ref();
            out.push_str("if ");
            print_expr(out, &flow.expr)?;
            out.push_str(" { ");
            print_block(out, &flow.body, true)?;
            out.push('}');
            if !flow.branch.is_empty() {
                out.push_str(" else { ");
                print_block(out, &flow.branch, true)?;
                out.push('}');
            }
        }
        Stmt::For(boxed, ..) => {
            let (counter, index, flow) = boxed.as_ref();
            out.push_str("for ");
            out.push_str(counter.name.as_str());
            if let Some(index) = index {
                out.push_str(", ");
                out.push_str(index.name.as_str());
            }
            out.push_str(" in ");
            print_expr(out, &flow.expr)?;
            out.push_str(" { ");
            print_block(out, &flow.body, true)?;
            out.push('}');
        }
        Stmt::While(boxed, ..) => {
            let flow = boxed.as_ref();
            out.push_str("while ");
            print_expr(out, &flow.expr)?;
            out.push_str(" { ");
            print_block(out, &flow.body, true)?;
            out.push('}');
        }
        Stmt::Assignment(boxed, ..) => {
            let (op, bin) = boxed.as_ref();
            print_expr(out, &bin.lhs)?;
            if let Some((_, _, _, syntax, _, _)) = op.get_op_assignment_info() {
                out.push(' ');
                out.push_str(syntax);
                out.push(' ');
            } else {
                out.push_str(" = ");
            }
            print_expr(out, &bin.rhs)?;
            if trailing_semi {
                out.push(';');
            }
        }
        Stmt::Block(block) => {
            out.push_str("{ ");
            print_block(out, block, true)?;
            out.push('}');
        }
        Stmt::FnCall(call, ..) => {
            print_call(out, call.as_ref())?;
            if trailing_semi {
                out.push(';');
            }
        }
        other => {
            return Err(RhError::Transpile(format!(
                "expr_print unsupported statement: {other:?}"
            )));
        }
    }
    Ok(())
}

fn print_block(
    out: &mut String,
    block: &rhai::StmtBlock,
    implicit_return: bool,
) -> Result<(), RhError> {
    let stmts: Vec<_> = block.iter().collect();
    for (index, stmt) in stmts.iter().enumerate() {
        if index > 0 {
            out.push(' ');
        }
        let is_last = index + 1 == stmts.len();
        print_stmt(out, stmt, !(implicit_return && is_last))?;
    }
    Ok(())
}

fn print_expr(out: &mut String, expr: &Expr) -> Result<(), RhError> {
    match expr {
        Expr::IntegerConstant(value, ..) => out.push_str(&value.to_string()),
        Expr::BoolConstant(value, ..) => out.push_str(if *value { "true" } else { "false" }),
        Expr::StringConstant(value, ..) => {
            out.push('`');
            out.push_str(value);
            out.push('`');
        }
        Expr::CharConstant(value, ..) => {
            out.push('\'');
            out.push(*value);
            out.push('\'');
        }
        Expr::Unit(..) => out.push_str("()"),
        Expr::Variable(ident, ..) => out.push_str(ident.1.as_str()),
        Expr::Property(prop, ..) => out.push_str(prop.2.as_str()),
        Expr::Dot(boxed, ..) => {
            print_expr(out, &boxed.lhs)?;
            out.push('.');
            print_expr(out, &boxed.rhs)?;
        }
        Expr::FnCall(call, ..) | Expr::MethodCall(call, ..) => print_call(out, call.as_ref())?,
        Expr::Array(items, ..) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                print_expr(out, item)?;
            }
            out.push(']');
        }
        Expr::Map(map, ..) => {
            out.push_str("#{");
            for (index, (key, value)) in map.0.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                out.push('`');
                out.push_str(key.name.as_str());
                out.push('`');
                out.push_str(": ");
                print_expr(out, value)?;
            }
            out.push('}');
        }
        Expr::Index(boxed, ..) => {
            print_expr(out, &boxed.lhs)?;
            out.push('[');
            print_expr(out, &boxed.rhs)?;
            out.push(']');
        }
        Expr::Stmt(block) => {
            out.push_str("{ ");
            print_block(out, block, true)?;
            out.push_str(" }");
        }
        other => {
            return Err(RhError::Transpile(format!(
                "expr_print unsupported expression: {other:?}"
            )));
        }
    }
    Ok(())
}

fn print_call(out: &mut String, call: &FnCallExpr) -> Result<(), RhError> {
    if let Some(op) = &call.op_token {
        return print_op(out, op, &call.args);
    }
    if !call.namespace.is_empty() {
        out.push_str(call.namespace.to_string().as_str());
        out.push_str("::");
    }
    out.push_str(call.name.as_str());
    out.push('(');
    for (index, arg) in call.args.iter().enumerate() {
        if index > 0 {
            out.push_str(", ");
        }
        print_expr(out, arg)?;
    }
    out.push(')');
    Ok(())
}

fn print_op(out: &mut String, op: &Token, args: &[Expr]) -> Result<(), RhError> {
    match (op, args.len()) {
        (Token::Plus, 2) => binary(out, "+", &args[0], &args[1]),
        (Token::Minus, 2) => binary(out, "-", &args[0], &args[1]),
        (Token::Multiply, 2) => binary(out, "*", &args[0], &args[1]),
        (Token::Divide, 2) => binary(out, "/", &args[0], &args[1]),
        (Token::Modulo, 2) => binary(out, "%", &args[0], &args[1]),
        (Token::Equals, 2) | (Token::EqualsTo, 2) => binary(out, "==", &args[0], &args[1]),
        (Token::NotEqualsTo, 2) => binary(out, "!=", &args[0], &args[1]),
        (Token::GreaterThan, 2) => binary(out, ">", &args[0], &args[1]),
        (Token::GreaterThanEqualsTo, 2) => binary(out, ">=", &args[0], &args[1]),
        (Token::LessThan, 2) => binary(out, "<", &args[0], &args[1]),
        (Token::LessThanEqualsTo, 2) => binary(out, "<=", &args[0], &args[1]),
        (Token::And, 2) => binary(out, "&&", &args[0], &args[1]),
        (Token::Or, 2) => binary(out, "||", &args[0], &args[1]),
        (Token::Minus, 1) => {
            out.push_str("(-(");
            print_expr(out, &args[0])?;
            out.push_str("))");
            Ok(())
        }
        (Token::Bang, 1) => {
            out.push_str("(!(");
            print_expr(out, &args[0])?;
            out.push_str("))");
            Ok(())
        }
        _ => Err(RhError::Transpile(format!(
            "expr_print unsupported operator `{op:?}`"
        ))),
    }
}

fn binary(out: &mut String, op: &str, lhs: &Expr, rhs: &Expr) -> Result<(), RhError> {
    out.push('(');
    print_expr(out, lhs)?;
    out.push(' ');
    out.push_str(op);
    out.push(' ');
    print_expr(out, rhs)?;
    out.push(')');
    Ok(())
}

pub fn uses_host_surface(expr: &Expr) -> bool {
    match expr {
        Expr::Variable(ident, ..) => matches!(ident.1.as_str(), "std" | "rhai" | "fleet"),
        Expr::Dot(boxed, ..) => uses_host_surface(&boxed.lhs) || uses_host_surface(&boxed.rhs),
        Expr::FnCall(call, ..) | Expr::MethodCall(call, ..) => {
            !call.namespace.is_empty()
                || call.name == "throw"
                || call.args.iter().any(uses_host_surface)
        }
        Expr::Property(..) | Expr::Index(..) => true,
        Expr::Array(items, ..) => items.iter().any(uses_host_surface),
        Expr::Map(map, ..) => map.0.iter().any(|(_, value)| uses_host_surface(value)),
        Expr::Stmt(block) => block.iter().any(stmt_uses_host),
        _ => false,
    }
}

fn stmt_uses_host(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Expr(expr) => uses_host_surface(expr.as_ref()),
        Stmt::Var(boxed, ..) => {
            let (_, expr, _) = boxed.as_ref();
            uses_host_surface(expr)
        }
        Stmt::Return(Some(expr), ..) => uses_host_surface(expr.as_ref()),
        Stmt::If(boxed, ..) => {
            let flow = boxed.as_ref();
            uses_host_surface(&flow.expr)
                || flow.body.iter().any(stmt_uses_host)
                || flow.branch.iter().any(stmt_uses_host)
        }
        Stmt::For(boxed, ..) => {
            let (_, _, flow) = boxed.as_ref();
            uses_host_surface(&flow.expr) || flow.body.iter().any(stmt_uses_host)
        }
        Stmt::While(boxed, ..) => {
            let flow = boxed.as_ref();
            uses_host_surface(&flow.expr) || flow.body.iter().any(stmt_uses_host)
        }
        Stmt::Assignment(boxed, ..) => {
            let (_, bin) = boxed.as_ref();
            !is_pure_int_expr(&bin.lhs) || !is_pure_int_expr(&bin.rhs)
        }
        Stmt::Block(block) => block.iter().any(stmt_uses_host),
        Stmt::FnCall(call, ..) => call.args.iter().any(uses_host_surface),
        _ => false,
    }
}

pub fn is_pure_int_expr(expr: &Expr) -> bool {
    match expr {
        Expr::IntegerConstant(..) => true,
        Expr::Variable(..) => true,
        Expr::FnCall(call, ..) if call.op_token.is_some() => call.args.iter().all(is_pure_int_expr),
        Expr::FnCall(call, ..) if call.name == "throw" => false,
        Expr::FnCall(..) | Expr::MethodCall(..) => false,
        Expr::Dot(..)
        | Expr::Property(..)
        | Expr::Index(..)
        | Expr::StringConstant(..)
        | Expr::BoolConstant(..)
        | Expr::Array(..)
        | Expr::Map(..)
        | Expr::Stmt(..) => false,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use rhai::Engine;

    use super::{expr_to_rhai, uses_host_surface};

    fn parse_expr(source: &str) -> rhai::Expr {
        let wrapped = format!("fn probe() {{ {source} }}");
        let ast = Engine::new().compile(wrapped).expect("compile");
        let def = ast.iter_fn_def().next().expect("fn");
        match def.body.iter().next().expect("stmt") {
            rhai::Stmt::Expr(expr) => expr.as_ref().clone(),
            rhai::Stmt::FnCall(call, ..) => rhai::Expr::FnCall(call.clone(), rhai::Position::NONE),
            other => panic!("unexpected stmt: {other:?}"),
        }
    }

    #[test]
    fn prints_std_path_join() {
        let expr = parse_expr("std::path::join(`a`, `b`)");
        assert_eq!(
            expr_to_rhai(&expr).expect("print"),
            "std::path::join(`a`, `b`)"
        );
        assert!(uses_host_surface(&expr));
    }
}
