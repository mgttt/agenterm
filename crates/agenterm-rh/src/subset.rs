use std::cell::RefCell;

use rhai::{AST, ASTNode, Expr, FnCallExpr, Stmt};

use crate::RhError;
use crate::fleet::{expr_uses_fleet, parse_fleet_call, validate_fleet_call};

pub fn validate_ast(ast: &AST) -> Result<(), RhError> {
    let error = RefCell::new(None);
    ast.walk(&mut |path| {
        let Some(node) = path.last() else {
            return true;
        };
        match node {
            ASTNode::Stmt(stmt) => {
                if let Some(err) = reject_stmt(stmt) {
                    *error.borrow_mut() = Some(err);
                    return false;
                }
            }
            ASTNode::Expr(expr) => {
                if let Some(err) = reject_expr(expr) {
                    *error.borrow_mut() = Some(err);
                    return false;
                }
            }
            _ => {}
        }
        true
    });
    match error.into_inner() {
        Some(err) => Err(err),
        None => {
            for def in ast.iter_fn_def() {
                if let Some(err) = validate_stmt_block(&def.body) {
                    return Err(err);
                }
            }
            Ok(())
        }
    }
}

fn reject_stmt(stmt: &Stmt) -> Option<RhError> {
    match stmt {
        Stmt::While(..) | Stmt::Do(..) | Stmt::For(..) | Stmt::Switch(..) | Stmt::TryCatch(..) => {
            Some(subset_error(
                "RH_SUBSET_NO_LOOP",
                "loops and try/catch are not in rh-0",
            ))
        }
        Stmt::Import(..) | Stmt::Export(..) => Some(subset_error(
            "RH_SUBSET_NO_MODULE",
            "import/export are not in rh-0",
        )),
        Stmt::Share(..) => Some(subset_error(
            "RH_SUBSET_NO_CLOSURE",
            "closure capture is not in rh-0",
        )),
        Stmt::FnCall(call, ..) => reject_call(call),
        _ => None,
    }
}

fn reject_expr(expr: &Expr) -> Option<RhError> {
    match expr {
        Expr::FnCall(call, ..) | Expr::MethodCall(call, ..) => reject_call(call),
        Expr::Map(..) | Expr::InterpolatedString(..) => Some(subset_error(
            "RH_SUBSET_NO_COLLECTION",
            "collections and interpolation are not in rh-0",
        )),
        _ => None,
    }
}

fn validate_stmt(stmt: &Stmt) -> Option<RhError> {
    match stmt {
        Stmt::Expr(expr) => validate_root_expr(expr.as_ref()),
        Stmt::Return(Some(expr), ..) => validate_root_expr(expr.as_ref()),
        Stmt::Var(boxed, ..) => {
            let (_, expr, _) = boxed.as_ref();
            validate_root_expr(expr)
        }
        Stmt::If(boxed, ..) => {
            let flow = boxed.as_ref();
            validate_root_expr(&flow.expr)?;
            validate_stmt_block(&flow.body)?;
            if !flow.branch.is_empty() {
                for stmt in flow.branch.iter() {
                    if let Some(err) = validate_stmt(stmt) {
                        return Some(err);
                    }
                }
            }
            None
        }
        Stmt::Block(block) => validate_stmt_block(block),
        _ => None,
    }
}

fn validate_stmt_block(block: &rhai::StmtBlock) -> Option<RhError> {
    for stmt in block.iter() {
        if let Some(err) = validate_stmt(stmt) {
            return Some(err);
        }
    }
    None
}

fn validate_root_expr(expr: &Expr) -> Option<RhError> {
    if let Some(call) = parse_fleet_call(expr) {
        return validate_fleet_call(&call).err();
    }
    if expr_uses_fleet(expr) {
        return Some(subset_error(
            "RH_SUBSET_FLEET_SHAPE",
            "fleet expression must be a supported fleet.* call",
        ));
    }
    match expr {
        Expr::Property(..) | Expr::ThisPtr(..) | Expr::Dot(..) | Expr::Index(..) => {
            Some(subset_error(
                "RH_SUBSET_NO_OBJECT",
                "object property access is not in rh-0",
            ))
        }
        _ => None,
    }
}

fn reject_call(call: &FnCallExpr) -> Option<RhError> {
    if call.name == "eval" {
        return Some(subset_error("RH_SUBSET_NO_EVAL", "eval is forbidden in rh"));
    }
    if call.capture_parent_scope {
        return Some(subset_error(
            "RH_SUBSET_NO_CLOSURE",
            "closure capture is not in rh-0",
        ));
    }
    None
}

fn subset_error(code: &'static str, detail: &str) -> RhError {
    RhError::Subset {
        code,
        detail: detail.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use rhai::Engine;

    use super::validate_ast;

    #[test]
    fn accepts_simple_fn() {
        let ast = Engine::new()
            .compile("fn add(a, b) { a + b }")
            .expect("compile");
        validate_ast(&ast).expect("subset");
    }

    #[test]
    fn accepts_fleet_protocol_info() {
        let ast = Engine::new()
            .compile("fn entry() { fleet.protocol.info(); 1 }")
            .expect("compile");
        validate_ast(&ast).expect("subset");
    }

    #[test]
    fn rejects_eval() {
        let ast = Engine::new().compile("eval(\"1\");").expect("compile");
        assert!(validate_ast(&ast).is_err());
    }
}
