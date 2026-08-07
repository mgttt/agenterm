use std::cell::RefCell;

use rhai::{AST, ASTFlags, ASTNode, BinaryExpr, Expr, FnCallExpr, OpAssignment, Stmt};

use crate::RhError;
use crate::expr_print::{is_pure_int_expr, uses_host_surface};
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
        Stmt::Do(..) | Stmt::Switch(..) => Some(subset_error(
            "RH_SUBSET_NO_LOOP",
            "do/switch are not in rh-3",
        )),
        Stmt::Share(..) => Some(subset_error(
            "RH_SUBSET_NO_CLOSURE",
            "closure capture is not in rh-2",
        )),
        Stmt::FnCall(call, ..) => reject_call(call),
        _ => None,
    }
}

fn reject_expr(expr: &Expr) -> Option<RhError> {
    match expr {
        Expr::FnCall(call, ..) | Expr::MethodCall(call, ..) => reject_call(call),
        Expr::InterpolatedString(..) => Some(subset_error(
            "RH_SUBSET_NO_INTERPOLATION",
            "string interpolation is not in rh-2",
        )),
        _ => None,
    }
}

fn validate_assignment(assign: &(OpAssignment, BinaryExpr)) -> Option<RhError> {
    if let Expr::Index(boxed, ..) = &assign.1.lhs {
        if assign.0.get_op_assignment_info().is_some() {
            return Some(subset_error(
                "RH_SUBSET_ASSIGN_LHS",
                "set index assignment must use plain `=` in rh-3",
            ));
        }
        if !matches!(assign.1.rhs, Expr::BoolConstant(true, ..)) {
            return Some(subset_error(
                "RH_SUBSET_ASSIGN_RHS",
                "set index assignment rhs must be `true` in rh-3",
            ));
        }
        if let Some(err) = validate_root_expr(&boxed.lhs) {
            return Some(err);
        }
        return validate_root_expr(&boxed.rhs);
    }
    if !matches!(assign.1.lhs, Expr::Variable(..)) {
        return Some(subset_error(
            "RH_SUBSET_ASSIGN_LHS",
            "assignment lhs must be a simple variable in rh-3",
        ));
    }
    if !is_pure_int_expr(&assign.1.rhs) && !uses_host_surface(&assign.1.rhs) {
        return Some(subset_error(
            "RH_SUBSET_ASSIGN_RHS",
            "assignment rhs must be a pure int or native host expression in rh-3",
        ));
    }
    None
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
            if let Some(err) = validate_root_expr(&flow.expr) {
                return Some(err);
            }
            if let Some(err) = validate_stmt_block(&flow.body) {
                return Some(err);
            }
            if !flow.branch.is_empty() {
                for stmt in flow.branch.iter() {
                    if let Some(err) = validate_stmt(stmt) {
                        return Some(err);
                    }
                }
            }
            None
        }
        Stmt::For(boxed, ..) => {
            let (_, _, flow) = boxed.as_ref();
            if let Some(err) = validate_root_expr(&flow.expr) {
                return Some(err);
            }
            validate_stmt_block(&flow.body)
        }
        Stmt::While(boxed, ..) => {
            let flow = boxed.as_ref();
            if !is_pure_int_expr(&flow.expr) {
                return Some(subset_error(
                    "RH_SUBSET_WHILE_COND",
                    "while condition must be a pure int expression in rh-3",
                ));
            }
            validate_stmt_block(&flow.body)
        }
        Stmt::TryCatch(boxed, ..) => {
            let flow = boxed.as_ref();
            if block_has_invalid_return(&flow.body) || block_has_invalid_return(&flow.branch) {
                return Some(subset_error(
                    "RH_SUBSET_TRY_RETURN",
                    "return is not allowed inside try/catch in rh-3",
                ));
            }
            if block_has_break_continue(&flow.body) || block_has_break_continue(&flow.branch) {
                return Some(subset_error(
                    "RH_SUBSET_TRY_BREAK",
                    "break/continue is not allowed inside try/catch in rh-3",
                ));
            }
            if let Some(err) = validate_stmt_block(&flow.body) {
                return Some(err);
            }
            validate_stmt_block(&flow.branch)
        }
        Stmt::BreakLoop(expr, ..) if expr.is_some() => Some(subset_error(
            "RH_SUBSET_BREAK_VALUE",
            "break/continue with value is not in rh-3",
        )),
        Stmt::Block(block) => validate_stmt_block(block),
        Stmt::Assignment(boxed, ..) => validate_assignment(boxed),
        Stmt::FnCall(call, ..) => {
            if call.name == "throw" {
                if call.args.len() != 1 {
                    return Some(subset_error(
                        "RH_SUBSET_THROW_ARGS",
                        "throw expects one argument",
                    ));
                }
                validate_root_expr(&call.args[0])
            } else {
                None
            }
        }
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

fn block_has_invalid_return(block: &rhai::StmtBlock) -> bool {
    block.iter().any(|stmt| match stmt {
        Stmt::Return(_, flags, ..) => !flags.contains(ASTFlags::BREAK),
        _ => false,
    })
}

fn block_has_break_continue(block: &rhai::StmtBlock) -> bool {
    block.iter().any(|stmt| matches!(stmt, Stmt::BreakLoop(..)))
}

fn validate_root_expr(expr: &Expr) -> Option<RhError> {
    if let Some(call) = parse_fleet_call(expr) {
        return validate_fleet_call(&call).err();
    }
    if expr_uses_fleet(expr) && parse_fleet_call(expr).is_none() {
        return Some(subset_error(
            "RH_SUBSET_FLEET_SHAPE",
            "fleet expression must be a supported fleet.* call",
        ));
    }
    if is_pure_int_expr(expr)
        || uses_host_surface(expr)
        || matches!(
            expr,
            Expr::StringConstant(..)
                | Expr::BoolConstant(..)
                | Expr::Unit(..)
                | Expr::Array(..)
                | Expr::Map(..)
        )
    {
        return None;
    }
    None
}

/// Fallback validation for scripts that delegate to the full Rhai worker runtime.
pub fn compat_validate(source: &str, ast: &AST) -> Result<(), RhError> {
    let _ = source;
    if ast_contains_eval(ast) {
        return Err(subset_error("RH_SUBSET_NO_EVAL", "eval is forbidden in rh"));
    }
    Ok(())
}

pub fn ast_contains_eval(ast: &AST) -> bool {
    let found = RefCell::new(false);
    ast.walk(&mut |path| {
        if *found.borrow() {
            return false;
        }
        let Some(ASTNode::Expr(expr)) = path.last() else {
            return true;
        };
        if let Expr::FnCall(call, ..) | Expr::MethodCall(call, ..) = expr {
            if call.name == "eval" {
                *found.borrow_mut() = true;
                return false;
            }
        }
        true
    });
    found.into_inner()
}

fn reject_call(call: &FnCallExpr) -> Option<RhError> {
    if call.name == "eval" {
        return Some(subset_error("RH_SUBSET_NO_EVAL", "eval is forbidden in rh"));
    }
    if call.capture_parent_scope {
        return Some(subset_error(
            "RH_SUBSET_NO_CLOSURE",
            "closure capture is not in rh-2",
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
    fn accepts_std_fs_exists() {
        let ast = Engine::new()
            .compile("fn entry() { if std::fs::exists(`/tmp`) { 1 } else { 0 } }")
            .expect("compile");
        validate_ast(&ast).expect("subset");
    }

    #[test]
    fn accepts_for_dyn_int_range() {
        let ast = Engine::new()
            .compile(
                "fn entry() { let count = 5; for x in 1..count { if x == 4 { return 7; } } 0 }",
            )
            .expect("compile");
        validate_ast(&ast).expect("subset");
    }

    #[test]
    fn accepts_for_var_len_range() {
        let ast = Engine::new()
            .compile("fn entry() { for x in 1..args.len { if x == 2 { return 9; } } 0 }")
            .expect("compile");
        validate_ast(&ast).expect("subset");
    }

    #[test]
    fn accepts_for_int_range() {
        let ast = Engine::new()
            .compile("fn entry() { for x in 1..5 { if x == 4 { return 7; } } 0 }")
            .expect("compile");
        validate_ast(&ast).expect("subset");
    }

    #[test]
    fn accepts_for_loop() {
        let ast = Engine::new()
            .compile("fn entry() { for x in [1, 2] { if x == 2 { return 9; } } 0 }")
            .expect("compile");
        validate_ast(&ast).expect("subset");
    }

    #[test]
    fn accepts_while_with_pure_int_condition() {
        let ast = Engine::new()
            .compile("fn entry() { while 0 == 1 { 0 } 42 }")
            .expect("compile");
        validate_ast(&ast).expect("subset");
    }

    #[test]
    fn rejects_while_with_host_condition() {
        let ast = Engine::new()
            .compile("fn entry() { while std::fs::exists(`/tmp`) { 0 } 0 }")
            .expect("compile");
        assert!(validate_ast(&ast).is_err());
    }

    #[test]
    fn accepts_try_catch_with_native_throw() {
        let mut engine = rhai::Engine::new();
        engine.set_optimization_level(rhai::OptimizationLevel::None);
        let ast = engine
            .compile("fn entry() { try { throw 1; 0 } catch (e) { 99 } }")
            .expect("compile");
        validate_ast(&ast).expect("subset");
    }

    #[test]
    fn rejects_return_inside_try() {
        let mut engine = rhai::Engine::new();
        engine.set_optimization_level(rhai::OptimizationLevel::None);
        let ast = engine
            .compile("fn entry() { try { return 1; } catch (e) { 0 } }")
            .expect("compile");
        assert!(validate_ast(&ast).is_err());
    }

    #[test]
    fn accepts_while_with_assignment() {
        let mut engine = rhai::Engine::new();
        engine.set_optimization_level(rhai::OptimizationLevel::None);
        let ast = engine
            .compile("fn entry() { let x = 3; while x != 0 { x -= 1; } x }")
            .expect("compile");
        validate_ast(&ast).expect("subset");
    }

    #[test]
    fn accepts_break_continue_in_for_loop() {
        let mut engine = rhai::Engine::new();
        engine.set_optimization_level(rhai::OptimizationLevel::None);
        let ast = engine
            .compile(
                "fn entry() { let sum = 0; for i in 1..10 { if i == 3 { continue; } if i == 8 { break; } sum += i; } sum }",
            )
            .expect("compile");
        validate_ast(&ast).expect("subset");
    }

    #[test]
    fn rejects_eval() {
        let ast = Engine::new().compile("eval(\"1\");").expect("compile");
        assert!(validate_ast(&ast).is_err());
    }

    #[test]
    fn validates_for_body_after_valid_iterable() {
        let mut engine = Engine::new();
        engine.set_optimization_level(rhai::OptimizationLevel::None);
        let ast = engine
            .compile("fn entry() { for x in [1, 2] { eval(\"1\"); } 0 }")
            .expect("compile");
        let error = validate_ast(&ast).expect_err("for body must be validated");
        assert!(error.to_string().contains("RH_SUBSET_NO_EVAL"), "{error}");
    }

    #[test]
    fn accepts_json_integer_property_assignment() {
        let mut engine = Engine::new();
        engine.set_optimization_level(rhai::OptimizationLevel::None);
        let ast = engine
            .compile(
                r#"fn entry() {
                    let document = rhai::json::parse("{\"n\":1}");
                    let total = 0;
                    total += document.n;
                    total
                }"#,
            )
            .expect("compile");
        validate_ast(&ast).expect("json integer assignment");
    }

    #[test]
    fn accepts_map_set_membership_assignment() {
        let mut engine = Engine::new();
        engine.set_optimization_level(rhai::OptimizationLevel::None);
        let ast = engine
            .compile(
                r#"fn entry() {
                    let names = #{};
                    let name = "agenterm";
                    names[name] = true;
                    names.contains(name)
                }"#,
            )
            .expect("compile");
        validate_ast(&ast).expect("map set membership");
    }
}
