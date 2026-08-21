use std::cell::RefCell;

use rhai::{AST, ASTFlags, ASTNode, BinaryExpr, Expr, FnCallExpr, OpAssignment, Stmt, Token};

use crate::RhError;
use crate::expr_print::{is_pure_int_expr, uses_host_surface, uses_host_surface_lang};

/// A root-expression hook the caller installs to reject shapes this module
/// knows nothing about. AgenTerm passes the fleet-shape validator
/// (`fleet_subset::fleet_root_expr`); the product passes nothing.
pub type RootExprHook = fn(&Expr) -> Option<RhError>;

/// Which flavour of the subset to enforce.
///
/// `SubsetPolicy::LANGUAGE` is Language 1: no `fleet` host root, no
/// `RH_SUBSET_FLEET_SHAPE`. `SubsetPolicy::agenterm(..)` is the workbench
/// flavour that keeps both. This is the seam that stops `fleet.rs` from
/// following `subset.rs` into the product crate.
#[derive(Clone, Copy)]
pub struct SubsetPolicy {
    host_surface: fn(&Expr) -> bool,
    root_expr_hook: Option<RootExprHook>,
}

impl SubsetPolicy {
    /// Language 1: `std` / `rh` / `rhai` only, no fleet shape checking.
    pub const LANGUAGE: Self = Self {
        host_surface: uses_host_surface_lang,
        root_expr_hook: None,
    };

    /// AgenTerm workbench: language roots plus `fleet`, plus a fleet-shape hook.
    pub const fn agenterm(hook: RootExprHook) -> Self {
        Self {
            host_surface: uses_host_surface,
            root_expr_hook: Some(hook),
        }
    }

    fn uses_host_surface(self, expr: &Expr) -> bool {
        (self.host_surface)(expr)
    }
}

/// Language-1 subset validation: no `compat_validate`, no fleet.
pub fn validate_ast_lang(ast: &AST) -> Result<(), RhError> {
    validate_ast_with(ast, SubsetPolicy::LANGUAGE)
}

pub fn validate_ast_with(ast: &AST, policy: SubsetPolicy) -> Result<(), RhError> {
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
                if let Some(err) = validate_stmt_block(&def.body, policy) {
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

fn validate_assignment(
    assign: &(OpAssignment, BinaryExpr),
    policy: SubsetPolicy,
) -> Option<RhError> {
    if let Expr::Index(boxed, ..) = &assign.1.lhs {
        if matches!(&boxed.lhs, Expr::Dot(inner, ..) if is_json_access_path(&inner.lhs))
            && !matches!(&boxed.lhs, Expr::Variable(..))
        {
            return validate_json_assignment(assign, policy);
        }
        if matches!(&boxed.lhs, Expr::Variable(..))
            && assign.0.get_op_assignment_info().is_none()
            && !matches!(assign.1.rhs, Expr::BoolConstant(true, ..))
        {
            return validate_json_assignment(assign, policy);
        }
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
        if let Some(err) = validate_root_expr(&boxed.lhs, policy) {
            return Some(err);
        }
        return validate_root_expr(&boxed.rhs, policy);
    }
    if is_json_assign_lhs(&assign.1.lhs) {
        return validate_json_assignment(assign, policy);
    }
    if !matches!(assign.1.lhs, Expr::Variable(..)) {
        return Some(subset_error(
            "RH_SUBSET_ASSIGN_LHS",
            "assignment lhs must be a simple variable in rh-3",
        ));
    }
    if is_pure_int_expr(&assign.1.rhs)
        || policy.uses_host_surface(&assign.1.rhs)
        || is_string_concat_assign_rhs(&assign.1.rhs, policy)
        || is_string_method_assign_rhs(&assign.1.rhs)
        || is_int_method_assign_rhs(&assign.1.rhs)
        || matches!(
            assign.1.rhs,
            Expr::BoolConstant(..) | Expr::StringConstant(..)
        )
        // Local helper calls (`assert_bounded_bundle(...)`, `start_task_probe(...)`)
        // are first-class INT/.rh values once emit lands; allow them as assign RHS.
        || matches!(
            &assign.1.rhs,
            Expr::FnCall(call, ..)
                if call.namespace.is_empty()
                    && call.op_token.is_none()
                    && call.name != "throw"
        )
    {
        return None;
    }
    let pos = assign.1.rhs.position();
    Some(subset_error(
        "RH_SUBSET_ASSIGN_RHS",
        &format!(
            "assignment rhs must be a pure int, bool/string literal, or native host expression in rh-3 (at {}:{})",
            pos.line().unwrap_or(0),
            pos.position().unwrap_or(0)
        ),
    ))
}

fn validate_json_assignment(
    assign: &(OpAssignment, BinaryExpr),
    policy: SubsetPolicy,
) -> Option<RhError> {
    if let Some((_, _, _, syntax, _, _)) = assign.0.get_op_assignment_info() {
        if syntax == "+=" {
            if !is_json_assign_lhs(&assign.1.lhs) {
                return Some(subset_error(
                    "RH_SUBSET_ASSIGN_LHS",
                    "json += assignment lhs must be a JSON path",
                ));
            }
            if is_pure_int_expr(&assign.1.rhs) || matches!(assign.1.rhs, Expr::Variable(..)) {
                return None;
            }
            return Some(subset_error(
                "RH_SUBSET_ASSIGN_RHS",
                "json += assignment rhs must be an int expression in rh-3",
            ));
        }
        return Some(subset_error(
            "RH_SUBSET_ASSIGN_LHS",
            "json assignment must use plain `=` in rh-3",
        ));
    }
    if !is_json_assign_lhs(&assign.1.lhs) {
        return Some(subset_error(
            "RH_SUBSET_ASSIGN_LHS",
            "json assignment lhs must be a JSON path or index",
        ));
    }
    if is_json_assign_rhs(&assign.1.rhs, policy) {
        return None;
    }
    Some(subset_error(
        "RH_SUBSET_ASSIGN_RHS",
        "json assignment rhs must be a supported JSON value expression in rh-3",
    ))
}

fn is_json_assign_lhs(expr: &Expr) -> bool {
    match expr {
        Expr::Index(boxed, ..) => is_json_access_path(&boxed.lhs),
        Expr::Dot(..) => is_json_access_path(expr),
        _ => false,
    }
}

fn is_json_access_path(expr: &Expr) -> bool {
    match expr {
        Expr::Variable(..) => true,
        Expr::Dot(boxed, ..) => match &boxed.rhs {
            Expr::Property(..) | Expr::Dot(..) => is_json_access_path(&boxed.lhs),
            Expr::Index(index_box, ..) => {
                matches!(&index_box.lhs, Expr::Property(..)) && is_json_access_path(&boxed.lhs)
            }
            _ => false,
        },
        _ => false,
    }
}

fn is_json_assign_rhs(expr: &Expr, policy: SubsetPolicy) -> bool {
    if validate_root_expr(expr, policy).is_none() {
        return true;
    }
    matches!(expr, Expr::Variable(..))
        || is_json_access_path(expr)
        || matches!(expr, Expr::Index(boxed, ..) if is_json_access_path(&boxed.lhs))
}

fn validate_stmt(stmt: &Stmt, policy: SubsetPolicy) -> Option<RhError> {
    match stmt {
        Stmt::Expr(expr) => validate_root_expr(expr.as_ref(), policy),
        Stmt::Return(Some(expr), ..) => validate_root_expr(expr.as_ref(), policy),
        Stmt::Var(boxed, ..) => {
            let (_, expr, _) = boxed.as_ref();
            validate_root_expr(expr, policy)
        }
        Stmt::If(boxed, ..) => {
            let flow = boxed.as_ref();
            if let Some(err) = validate_root_expr(&flow.expr, policy) {
                return Some(err);
            }
            if let Some(err) = validate_stmt_block(&flow.body, policy) {
                return Some(err);
            }
            if !flow.branch.is_empty() {
                for stmt in flow.branch.iter() {
                    if let Some(err) = validate_stmt(stmt, policy) {
                        return Some(err);
                    }
                }
            }
            None
        }
        Stmt::For(boxed, ..) => {
            let (_, _, flow) = boxed.as_ref();
            if let Some(err) = validate_root_expr(&flow.expr, policy) {
                return Some(err);
            }
            validate_stmt_block(&flow.body, policy)
        }
        Stmt::While(boxed, ..) => {
            let flow = boxed.as_ref();
            if !is_pure_int_expr(&flow.expr) {
                let pos = flow.expr.position();
                return Some(subset_error(
                    "RH_SUBSET_WHILE_COND",
                    &format!(
                        "while condition must be a pure int expression in rh-3 (at {}:{})",
                        pos.line().unwrap_or(0),
                        pos.position().unwrap_or(0)
                    ),
                ));
            }
            validate_stmt_block(&flow.body, policy)
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
            if let Some(err) = validate_stmt_block(&flow.body, policy) {
                return Some(err);
            }
            validate_stmt_block(&flow.branch, policy)
        }
        Stmt::BreakLoop(expr, ..) if expr.is_some() => Some(subset_error(
            "RH_SUBSET_BREAK_VALUE",
            "break/continue with value is not in rh-3",
        )),
        Stmt::Block(block) => validate_stmt_block(block, policy),
        Stmt::Assignment(boxed, ..) => validate_assignment(boxed, policy),
        Stmt::FnCall(call, ..) => {
            if call.name == "throw" {
                if call.args.len() != 1 {
                    return Some(subset_error(
                        "RH_SUBSET_THROW_ARGS",
                        "throw expects one argument",
                    ));
                }
                validate_root_expr(&call.args[0], policy)
            } else {
                None
            }
        }
        _ => None,
    }
}

fn validate_stmt_block(block: &rhai::StmtBlock, policy: SubsetPolicy) -> Option<RhError> {
    for stmt in block.iter() {
        if let Some(err) = validate_stmt(stmt, policy) {
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

fn validate_root_expr(expr: &Expr, policy: SubsetPolicy) -> Option<RhError> {
    if let Some(hook) = policy.root_expr_hook
        && let Some(err) = hook(expr)
    {
        return Some(err);
    }
    if is_pure_int_expr(expr)
        || policy.uses_host_surface(expr)
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

/// Fallback validation for scripts that delegate to the full Rh worker runtime.
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
        if let Expr::FnCall(call, ..) | Expr::MethodCall(call, ..) = expr
            && call.name == "eval"
        {
            *found.borrow_mut() = true;
            return false;
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

fn is_int_method_assign_rhs(expr: &Expr) -> bool {
    match expr {
        Expr::Dot(boxed, ..) => matches!(
            &boxed.rhs,
            Expr::MethodCall(call, ..)
                if matches!(
                    call.name.as_str(),
                    "index_of" | "parse_int" | "len" | "unix_millis"
                )
        ),
        _ => false,
    }
}

fn is_string_method_assign_rhs(expr: &Expr) -> bool {
    match expr {
        Expr::Dot(boxed, ..) => matches!(
            &boxed.rhs,
            Expr::MethodCall(call, ..)
                if call.args.is_empty()
                    && matches!(call.name.as_str(), "to_string" | "to_lower" | "trim")
        ),
        _ => false,
    }
}

fn is_string_concat_assign_rhs(expr: &Expr, policy: SubsetPolicy) -> bool {
    match expr {
        Expr::StringConstant(..) | Expr::Variable(..) => true,
        Expr::FnCall(call, ..)
            if matches!(call.op_token.as_ref(), Some(Token::Plus)) && call.args.len() == 2 =>
        {
            call.args
                .iter()
                .all(|arg| is_string_concat_assign_rhs(arg, policy))
        }
        other => policy.uses_host_surface(other) || args_index_like(other),
    }
}

fn args_index_like(expr: &Expr) -> bool {
    matches!(expr, Expr::Index(boxed, ..) if matches!(&boxed.lhs, Expr::Variable(ident, ..) if ident.1.as_str() == "args"))
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

    use super::validate_ast_lang as validate_ast;

    #[test]
    fn accepts_simple_fn() {
        let ast = Engine::new()
            .compile("fn add(a, b) { a + b }")
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
    fn accepts_json_path_plus_assignment() {
        let mut engine = Engine::new();
        engine.set_optimization_level(rhai::OptimizationLevel::None);
        let ast = engine
            .compile(
                r#"fn entry() {
                    let context = rhai::json::parse("{\"process_observation\":{\"owned_commands\":0}}");
                    context.process_observation.owned_commands += 1;
                    context.process_observation.owned_commands
                }"#,
            )
            .expect("compile");
        validate_ast(&ast).expect("json path +=");
    }

    #[test]
    fn accepts_json_path_key_and_index_field_assignment() {
        let mut engine = Engine::new();
        engine.set_optimization_level(rhai::OptimizationLevel::None);
        let ast = engine
            .compile(
                r#"fn entry() {
                    let context = rhai::json::parse("{\"results\":{}}");
                    let timing = rhai::json::parse("{\"gates\":[{\"id\":\"a\",\"status\":\"not_run\",\"duration_ms\":0}]}");
                    let gate_key = "a";
                    let index = 0;
                    context.results[gate_key] = #{ id: gate_key, status: "passed" };
                    timing.gates[index].status = "passed";
                    timing.gates[index].duration_ms = 0;
                    0
                }"#,
            )
            .expect("compile");
        validate_ast(&ast).expect("json path assignment");
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

    #[test]
    fn owned_scripts_have_pure_int_while_conditions() {
        use rhai::{ASTNode, Engine, OptimizationLevel, Stmt};
        use std::path::PathBuf;

        use crate::expr_print::is_pure_int_expr;

        fn while_cond_errors(source: &str) -> Vec<String> {
            let mut engine = Engine::new();
            engine.set_optimization_level(OptimizationLevel::None);
            engine.set_max_expr_depths(
                crate::check::RH_MAX_EXPR_DEPTH,
                crate::check::RH_MAX_EXPR_DEPTH,
            );
            let ast = engine.compile(source).expect("compile");
            let mut errors = Vec::new();
            ast.walk(&mut |path| {
                if let Some(ASTNode::Stmt(Stmt::While(boxed, ..))) = path.last() {
                    let cond = &boxed.as_ref().expr;
                    if !is_pure_int_expr(cond) {
                        let pos = cond.position();
                        errors.push(format!(
                            "{}:{}",
                            pos.line().unwrap_or(0),
                            pos.position().unwrap_or(0)
                        ));
                    }
                }
                true
            });
            errors
        }

        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("repo root");
        for rel in [
            "scripts/rh/qualification-selftest.rh",
            "scripts/rh/check.rh",
            "scripts/rh/lib/qualification.rh",
            "scripts/rh/lib/test_harness.rh",
        ] {
            let source = std::fs::read_to_string(root.join(rel)).expect("read");
            assert!(
                while_cond_errors(&source).is_empty(),
                "{rel} standalone while errors"
            );
            let bundled = crate::bundle_project_source(&root, &source).expect("bundle");
            assert!(
                while_cond_errors(&bundled).is_empty(),
                "{rel} bundled while errors"
            );
        }
    }
}
