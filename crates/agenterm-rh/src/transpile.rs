use std::collections::BTreeMap;

use rhai::{AST, ASTFlags, Expr, ScriptFuncDef, Stmt, StmtBlock, Token};

use crate::{
    RhError,
    expr_print::{
        args_index_expr, expr_to_rhai, is_args_len_expr, is_pure_int_expr, is_var_len_expr,
        uses_host_surface, var_len_name,
    },
    fleet::{fleet_params_json, parse_fleet_call, validate_fleet_call},
    host_api::{emit_host_runtime, rust_raw_string_literal},
    subset::{compat_validate, validate_ast},
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum ValueKind {
    Int,
    Bool,
    String,
    Char,
    Json,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CdylibExecutionMode {
    Native,
    HostEval,
    CompatDelegating,
}

impl CdylibExecutionMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Native => "native",
            Self::HostEval => "host-eval",
            Self::CompatDelegating => "compat-delegating",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CdylibTranspileOutput {
    pub rust: String,
    pub execution_mode: CdylibExecutionMode,
}

#[derive(Clone)]
struct EmitCtx {
    cdylib: bool,
    scope: BTreeMap<String, ValueKind>,
    try_depth: u32,
}

impl EmitCtx {
    fn new(cdylib: bool) -> Self {
        Self {
            cdylib,
            scope: BTreeMap::new(),
            try_depth: 0,
        }
    }

    fn in_try(&self) -> bool {
        self.cdylib && self.try_depth > 0
    }

    fn enter_try(mut self) -> Self {
        self.try_depth += 1;
        self
    }

    fn value_type(&self) -> &'static str {
        if self.cdylib { "INT" } else { "Dynamic" }
    }

    fn unit_expr(&self) -> &'static str {
        if self.cdylib { "0" } else { "Dynamic::UNIT" }
    }

    fn emit_scope_json_expr(&self, out: &mut String) {
        if self.scope.is_empty() {
            out.push_str("\"{}\"");
            return;
        }
        out.push_str("&format!(\"{{\\\"vars\\\":{{");
        let mut first = true;
        for (name, kind) in &self.scope {
            if !first {
                out.push(',');
            }
            first = false;
            match kind {
                ValueKind::Int => {
                    out.push_str(&format!(
                        "\\\"{name}\\\":{{\\\"kind\\\":\\\"int\\\",\\\"value\\\":{{}}}}"
                    ));
                }
                ValueKind::Bool => {
                    out.push_str(&format!(
                        "\\\"{name}\\\":{{\\\"kind\\\":\\\"bool\\\",\\\"value\\\":{{}}}}"
                    ));
                }
                ValueKind::String | ValueKind::Char => {
                    out.push_str(&format!(
                        "\\\"{name}\\\":{{\\\"kind\\\":\\\"string\\\",\\\"value\\\":{{}}}}"
                    ));
                }
                ValueKind::Json => {
                    out.push_str(&format!(
                        "\\\"{name}\\\":{{\\\"kind\\\":\\\"json\\\",\\\"value\\\":{{}}}}"
                    ));
                }
            }
        }
        out.push_str("}}}}\"");
        for (name, kind) in &self.scope {
            out.push_str(", ");
            if matches!(kind, ValueKind::String | ValueKind::Json) {
                out.push_str("serde_json::to_string(&");
                out.push_str(name);
                out.push_str(").unwrap_or_else(|_| \"\\\"\\\"\".to_owned())");
            } else if matches!(kind, ValueKind::Char) {
                out.push_str(name);
                out.push_str(".to_string()");
            } else {
                out.push_str(name);
            }
        }
        out.push(')');
    }

    fn with_binding(mut self, name: &str, kind: ValueKind) -> Self {
        self.scope.insert(name.to_owned(), kind);
        self
    }
}

pub fn transpile(source: &str) -> Result<String, RhError> {
    let ast = parse(source)?;
    validate_ast(&ast)?;
    emit(&ast, EmitCtx::new(false))
}

pub fn transpile_cdylib(source: &str) -> Result<String, RhError> {
    Ok(transpile_cdylib_with_mode(source)?.rust)
}

pub fn transpile_cdylib_with_mode(source: &str) -> Result<CdylibTranspileOutput, RhError> {
    let ast = parse(source)?;
    let has_entry_fn = ast_has_entry_fn(&ast);
    let validate_ok = validate_ast(&ast).is_ok();
    // A native cdylib must have an explicit callable entry point. Top-level
    // compatibility scripts intentionally continue through whole-script
    // delegation below, while manifest qualification can reject that mode for
    // `.rh` tasks.
    if validate_ok && has_entry_fn {
        if let Ok(rust) = emit(&ast, EmitCtx::new(true)) {
            let execution_mode = if rust.matches("rh_host_eval_int(").count() > 1 {
                CdylibExecutionMode::HostEval
            } else {
                CdylibExecutionMode::Native
            };
            return Ok(CdylibTranspileOutput {
                rust,
                execution_mode,
            });
        }
    }
    compat_validate(source, &ast)?;
    Ok(CdylibTranspileOutput {
        rust: emit_compat_delegating(source)?,
        execution_mode: CdylibExecutionMode::CompatDelegating,
    })
}

fn ast_has_entry_fn(ast: &AST) -> bool {
    ast.iter_functions().any(|meta| meta.name == "entry")
}

fn emit_compat_delegating(source: &str) -> Result<String, RhError> {
    let mut out = String::from(
        "// Generated by agenterm-rh (compat delegating) — do not edit.\n\
         #![allow(clippy::all, dead_code, unused_variables)]\n\n\
         type INT = i64;\n\n",
    );
    emit_host_runtime(&mut out);
    out.push_str("const RH_SCRIPT_SOURCE: &str = ");
    out.push_str(&rust_raw_string_literal(source));
    out.push_str(
        ";\n\n\
         fn rh_entry_internal() -> INT {\n\
             rh_host_run_script(RH_SCRIPT_SOURCE)\n\
         }\n\n\
         #[no_mangle]\n\
         pub extern \"C\" fn rh_pack_api_version() -> u32 {\n    1\n}\n\n\
         #[no_mangle]\n\
         pub extern \"C\" fn rh_entry() -> i64 {\n\
             rh_entry_internal() as i64\n\
         }\n",
    );
    Ok(out)
}

fn parse(source: &str) -> Result<AST, RhError> {
    crate::check::parse_rh_ast(source)
}

fn emit(ast: &AST, ctx: EmitCtx) -> Result<String, RhError> {
    let mut out = String::from(
        "// Generated by agenterm-rh — do not edit.\n\
         #![allow(clippy::all, dead_code, unused_variables)]\n\n",
    );
    if ctx.cdylib {
        out.push_str("type INT = i64;\n\n");
        emit_host_runtime(&mut out);
    } else {
        out.push_str("use rhai::Dynamic;\n\n");
    }

    let mut wrote_fn = false;
    let mut has_entry = false;
    for meta in ast.iter_functions() {
        if meta.name == "entry" {
            has_entry = true;
        }
        if ctx.cdylib && meta.name == "cc_lines" {
            let Some(def) = find_fn_def(ast, meta.name) else {
                return Err(RhError::Transpile("missing cc_lines body".into()));
            };
            emit_cc_lines_exports(&mut out, def)?;
            wrote_fn = true;
            continue;
        }
        let Some(def) = find_fn_def(ast, meta.name) else {
            return Err(RhError::Transpile(format!(
                "missing body for function `{}`",
                meta.name
            )));
        };
        emit_fn(&mut out, def, &mut EmitCtx::new(ctx.cdylib))?;
        wrote_fn = true;
    }

    if ctx.cdylib {
        out.push_str("fn rh_entry_internal() -> INT {\n");
        if has_entry {
            out.push_str("    entry()\n");
        } else {
            out.push_str("    0\n");
        }
        out.push_str("}\n\n");
        out.push_str(
            "#[no_mangle]\npub extern \"C\" fn rh_pack_api_version() -> u32 {\n    1\n}\n\n",
        );
        out.push_str("#[no_mangle]\npub extern \"C\" fn rh_entry() -> i64 {\n");
        out.push_str("    rh_entry_internal() as i64\n");
        out.push_str("}\n");
    } else if !wrote_fn {
        out.push_str("pub fn rh_entry() -> Dynamic {\n    Dynamic::UNIT\n}\n");
    } else {
        out.push_str("\npub fn rh_entry() -> Dynamic {\n    Dynamic::UNIT\n}\n");
    }

    Ok(out)
}

pub fn cc_line_count(source: &str) -> Option<u32> {
    let ast = parse(source).ok()?;
    let def = find_fn_def(&ast, "cc_lines")?;
    let lines = extract_cc_lines(def).ok()?;
    u32::try_from(lines.len()).ok()
}

fn extract_cc_lines(def: &ScriptFuncDef) -> Result<Vec<String>, RhError> {
    let expr = single_expression(&def.body)?;
    extract_string_array_expr(expr)
}

fn extract_string_array_expr(expr: &Expr) -> Result<Vec<String>, RhError> {
    match expr {
        Expr::Array(items, ..) => items.iter().map(string_literal_expr).collect(),
        Expr::DynamicConstant(value, ..) => {
            if !value.is_array() {
                return Err(RhError::Transpile(
                    "cc_lines must return a string array literal".into(),
                ));
            }
            let array = value.clone().into_array().map_err(|_| {
                RhError::Transpile("cc_lines must return a string array literal".into())
            })?;
            array.iter().map(dynamic_string).collect()
        }
        _ => Err(RhError::Transpile(
            "cc_lines must return a string array literal".into(),
        )),
    }
}

fn string_literal_expr(expr: &Expr) -> Result<String, RhError> {
    match expr {
        Expr::StringConstant(value, ..) => Ok(value.to_string()),
        _ => Err(RhError::Transpile(
            "cc_lines array must contain string literals only".into(),
        )),
    }
}

fn dynamic_string(value: &rhai::Dynamic) -> Result<String, RhError> {
    value
        .clone()
        .into_string()
        .map_err(|_| RhError::Transpile("cc_lines array must contain strings only".into()))
}

pub(crate) fn single_expression(body: &StmtBlock) -> Result<&Expr, RhError> {
    let mut iter = body.iter();
    let Some(stmt) = iter.next() else {
        return Err(RhError::Transpile(
            "cc_lines must return a string array literal".into(),
        ));
    };
    if iter.next().is_some() {
        return Err(RhError::Transpile(
            "cc_lines must contain a single return expression".into(),
        ));
    }
    match stmt {
        Stmt::Expr(expr) => Ok(expr.as_ref()),
        Stmt::Return(Some(expr), ..) => Ok(expr.as_ref()),
        Stmt::Block(block) => single_expression(block),
        _ => Err(RhError::Transpile(
            "cc_lines must return a string array literal".into(),
        )),
    }
}

fn emit_cc_lines_exports(out: &mut String, def: &ScriptFuncDef) -> Result<(), RhError> {
    let lines = extract_cc_lines(def)?;
    let count = lines.len();
    out.push_str("static RH_CC_LINES: [&str; ");
    out.push_str(&count.to_string());
    out.push_str("] = [");
    for (index, line) in lines.iter().enumerate() {
        if index > 0 {
            out.push_str(", ");
        }
        out.push_str(&format!("{line:?}"));
    }
    out.push_str("];\n\n");
    out.push_str("#[no_mangle]\npub extern \"C\" fn rh_cc_line_count() -> u32 {\n    ");
    out.push_str(&count.to_string());
    out.push_str("\n}\n\n");
    out.push_str("#[no_mangle]\npub extern \"C\" fn rh_cc_line_len(i: u32) -> u32 {\n");
    out.push_str("    if (i as usize) >= RH_CC_LINES.len() {\n        return 0;\n    }\n");
    out.push_str("    RH_CC_LINES[i as usize].len() as u32\n}\n\n");
    out.push_str("#[no_mangle]\npub extern \"C\" fn rh_cc_line_ptr(i: u32) -> *const u8 {\n");
    out.push_str(
        "    if (i as usize) >= RH_CC_LINES.len() {\n        return std::ptr::null();\n    }\n",
    );
    out.push_str("    RH_CC_LINES[i as usize].as_ptr()\n}\n\n");
    Ok(())
}

fn find_fn_def<'a>(ast: &'a AST, name: &str) -> Option<&'a ScriptFuncDef> {
    ast.iter_fn_def()
        .find(|def| def.name == name)
        .map(|def| def.as_ref())
}

fn emit_fn(out: &mut String, def: &ScriptFuncDef, ctx: &mut EmitCtx) -> Result<(), RhError> {
    let mut fn_ctx = ctx.clone();
    for param in &def.params {
        fn_ctx = fn_ctx.with_binding(param.as_str(), ValueKind::Int);
    }
    out.push_str("pub fn ");
    out.push_str(def.name.as_str());
    out.push('(');
    for (index, param) in def.params.iter().enumerate() {
        if index > 0 {
            out.push_str(", ");
        }
        if fn_ctx.cdylib {
            out.push_str(param.as_str());
            out.push_str(": INT");
        } else {
            out.push_str("mut ");
            out.push_str(param.as_str());
            out.push_str(": Dynamic");
        }
    }
    out.push_str(") -> ");
    out.push_str(fn_ctx.value_type());
    out.push_str(" {\n");
    emit_block(out, &def.body, &mut fn_ctx, true)?;
    out.push_str("}\n\n");
    Ok(())
}

fn emit_block(
    out: &mut String,
    block: &StmtBlock,
    ctx: &mut EmitCtx,
    implicit_return: bool,
) -> Result<(), RhError> {
    let stmts: Vec<_> = block.iter().collect();
    for (index, stmt) in stmts.iter().enumerate() {
        let is_last = index + 1 == stmts.len();
        emit_stmt(out, stmt, ctx, implicit_return && is_last)?;
    }
    Ok(())
}

fn emit_stmt(
    out: &mut String,
    stmt: &Stmt,
    ctx: &mut EmitCtx,
    implicit_return: bool,
) -> Result<(), RhError> {
    match stmt {
        Stmt::Var(boxed, ..) => {
            let (ident, expr, _) = boxed.as_ref();
            let kind = infer_binding_kind(expr, ctx);
            out.push_str("    let mut ");
            out.push_str(ident.name.as_str());
            out.push_str(" = ");
            if kind == ValueKind::Json
                && let Some((binding, path)) = json_value_path(expr, ctx)
                && !path.is_empty()
            {
                out.push_str("rh_json_get_path(&");
                out.push_str(binding);
                out.push_str(", ");
                emit_json_path(out, &path);
                out.push(')');
            } else if kind == ValueKind::String && matches!(expr, Expr::StringConstant(..)) {
                out.push_str("String::from(");
                emit_native_string(out, expr, ctx)?;
                out.push(')');
            } else if kind == ValueKind::String
                && matches!(
                    expr,
                    Expr::Variable(ident, ..)
                        if ctx.scope.get(ident.1.as_str()).copied() == Some(ValueKind::String)
                )
            {
                emit_expr(out, expr, ctx)?;
                out.push_str(".clone()");
            } else {
                emit_expr(out, expr, ctx)?;
            }
            out.push_str(";\n");
            *ctx = ctx.clone().with_binding(ident.name.as_str(), kind);
        }
        Stmt::Assignment(boxed, ..) => {
            let (op, bin) = boxed.as_ref();
            let Expr::Variable(ident, ..) = &bin.lhs else {
                return Err(RhError::Transpile(
                    "assignment lhs must be a variable".into(),
                ));
            };
            out.push_str("    ");
            out.push_str(ident.1.as_str());
            if let Some((_, _, _, syntax, _, _)) = op.get_op_assignment_info() {
                out.push(' ');
                out.push_str(syntax);
                out.push(' ');
            } else {
                out.push_str(" = ");
            }
            emit_expr(out, &bin.rhs, ctx)?;
            out.push_str(";\n");
        }
        Stmt::Return(Some(expr), ..) => {
            out.push_str("    return ");
            emit_expr(out, expr, ctx)?;
            out.push_str(";\n");
        }
        Stmt::Return(None, ..) => {
            out.push_str("    return ");
            out.push_str(ctx.unit_expr());
            out.push_str(";\n");
        }
        Stmt::If(boxed, ..) => {
            let flow = boxed.as_ref();
            out.push_str("    if ");
            emit_expr(out, &flow.expr, ctx)?;
            out.push_str(" != 0 {\n");
            emit_block(out, &flow.body, ctx, true)?;
            out.push_str("    }");
            if !flow.branch.is_empty() {
                out.push_str(" else {\n");
                emit_block(out, &flow.branch, ctx, true)?;
                out.push_str("    }\n");
            } else {
                out.push('\n');
            }
        }
        Stmt::For(boxed, ..) => {
            let (counter, _, flow) = boxed.as_ref();
            if let Some(plan) = int_for_plan(&flow.expr) {
                match plan {
                    IntForPlan::Values(items) => {
                        out.push_str("    for ");
                        out.push_str(counter.name.as_str());
                        out.push_str(" in [");
                        for (index, item) in items.iter().enumerate() {
                            if index > 0 {
                                out.push_str(", ");
                            }
                            out.push_str(&item.to_string());
                        }
                        out.push_str("].iter().copied() {\n");
                    }
                    IntForPlan::Exclusive { start, end } => {
                        out.push_str("    for ");
                        out.push_str(counter.name.as_str());
                        out.push_str(" in ");
                        emit_for_bound(out, &start, ctx)?;
                        out.push_str("..");
                        emit_for_bound(out, &end, ctx)?;
                        out.push_str(" {\n");
                    }
                    IntForPlan::Inclusive { start, end } => {
                        out.push_str("    for ");
                        out.push_str(counter.name.as_str());
                        out.push_str(" in ");
                        emit_for_bound(out, &start, ctx)?;
                        out.push_str("..=");
                        emit_for_bound(out, &end, ctx)?;
                        out.push_str(" {\n");
                    }
                }
                let mut loop_ctx = ctx
                    .clone()
                    .with_binding(counter.name.as_str(), ValueKind::Int);
                emit_block(out, &flow.body, &mut loop_ctx, false)?;
                out.push_str("    }\n");
            } else if let Some((binding, path)) = json_value_path(&flow.expr, ctx) {
                out.push_str("    for ");
                out.push_str(counter.name.as_str());
                out.push_str(" in rh_json_array_items(&");
                out.push_str(binding);
                out.push_str(", ");
                emit_json_path(out, &path);
                out.push_str(") {\n");
                let mut loop_ctx = ctx
                    .clone()
                    .with_binding(counter.name.as_str(), ValueKind::Json);
                emit_block(out, &flow.body, &mut loop_ctx, false)?;
                out.push_str("    }\n");
            } else if let Some(binding) = string_for_binding(&flow.expr, ctx) {
                out.push_str("    for ");
                out.push_str(counter.name.as_str());
                out.push_str(" in ");
                out.push_str(binding);
                out.push_str(".chars() {\n");
                let mut loop_ctx = ctx
                    .clone()
                    .with_binding(counter.name.as_str(), ValueKind::Char);
                emit_block(out, &flow.body, &mut loop_ctx, false)?;
                out.push_str("    }\n");
            } else {
                let snippet = crate::expr_print::stmt_to_rhai(stmt)?;
                out.push_str("    let _for = rh_host_eval_int(");
                out.push_str(&format!("{:?}, ", snippet));
                ctx.emit_scope_json_expr(out);
                out.push_str(";\n");
            }
        }
        Stmt::While(boxed, ..) => {
            let flow = boxed.as_ref();
            if is_pure_int_expr(&flow.expr) {
                out.push_str("    while ");
                emit_expr(out, &flow.expr, ctx)?;
                out.push_str(" != 0 {\n");
                emit_block(out, &flow.body, ctx, false)?;
                out.push_str("    }\n");
            } else {
                let snippet = crate::expr_print::stmt_to_rhai(stmt)?;
                out.push_str("    let _while = rh_host_eval_int(");
                out.push_str(&format!("{:?}, ", snippet));
                ctx.emit_scope_json_expr(out);
                out.push_str(";\n");
            }
        }
        Stmt::TryCatch(boxed, ..) if ctx.cdylib => {
            let flow = boxed.as_ref();
            out.push_str("    match (|| -> Result<INT, INT> {\n");
            let mut try_ctx = ctx.clone().enter_try();
            emit_try_block(out, &flow.body, &mut try_ctx)?;
            out.push_str("    })() {\n");
            out.push_str("        Ok(__rh_try_v) => __rh_try_v,\n");
            out.push_str("        Err(_) => {\n");
            emit_block_tail_expr(out, &flow.branch, ctx)?;
            out.push_str("        }\n");
            out.push_str("    }\n");
        }
        Stmt::TryCatch(..) => {
            let snippet = crate::expr_print::stmt_to_rhai(stmt)?;
            out.push_str("    let _try = rh_host_eval_int(");
            out.push_str(&format!("{:?}, ", snippet));
            ctx.emit_scope_json_expr(out);
            out.push_str(";\n");
            if implicit_return {
                out.push_str("    return _try;\n");
            }
        }
        Stmt::Block(boxed) => {
            out.push_str("    {\n");
            emit_block(out, boxed, ctx, implicit_return)?;
            out.push_str("    }\n");
        }
        Stmt::Expr(expr) if ctx.cdylib && emit_string_mut_stmt(out, expr, ctx)? => {}
        Stmt::Expr(expr) if implicit_return => {
            out.push_str("    return ");
            emit_expr(out, expr, ctx)?;
            out.push_str(";\n");
        }
        Stmt::Expr(expr) => {
            out.push_str("    ");
            emit_expr(out, expr, ctx)?;
            out.push_str(";\n");
        }
        Stmt::FnCall(call, ..) if call.name == "throw" => {
            emit_throw_stmt(out, call, ctx, implicit_return)?;
        }
        Stmt::FnCall(call, ..)
            if ctx.cdylib
                && call.namespace.is_empty()
                && call.name == "require"
                && call.args.len() == 2 =>
        {
            emit_require_stmt(out, &call.args[0], &call.args[1], ctx)?;
        }
        Stmt::FnCall(call, ..) if implicit_return => {
            out.push_str("    return ");
            emit_call(out, call, ctx)?;
            out.push_str(";\n");
        }
        Stmt::FnCall(call, ..) => {
            out.push_str("    ");
            emit_call(out, call, ctx)?;
            out.push_str(";\n");
        }
        Stmt::BreakLoop(expr, flags, ..) => {
            if expr.is_some() {
                return Err(RhError::Transpile(
                    "break/continue with value is not in rh-3".into(),
                ));
            }
            if flags.contains(ASTFlags::BREAK) {
                out.push_str("    break;\n");
            } else {
                out.push_str("    continue;\n");
            }
        }
        Stmt::Noop(..) => {}
        other => {
            return Err(RhError::Transpile(format!(
                "unsupported statement in rh-2: {other:?}"
            )));
        }
    }
    Ok(())
}

fn emit_block_tail_expr(
    out: &mut String,
    block: &StmtBlock,
    ctx: &mut EmitCtx,
) -> Result<(), RhError> {
    let stmts: Vec<_> = block.iter().collect();
    if stmts.is_empty() {
        out.push_str("            0\n");
        return Ok(());
    }
    for stmt in &stmts[..stmts.len() - 1] {
        let mut inner = String::new();
        emit_stmt(&mut inner, stmt, ctx, false)?;
        for line in inner.lines() {
            out.push_str("            ");
            out.push_str(line);
            out.push('\n');
        }
    }
    match stmts.last().expect("non-empty") {
        Stmt::Expr(expr) => {
            out.push_str("            ");
            emit_expr(out, expr, ctx)?;
            out.push('\n');
        }
        other => {
            let mut inner = String::new();
            emit_stmt(&mut inner, other, ctx, true)?;
            for line in inner.lines() {
                out.push_str("            ");
                out.push_str(line);
                out.push('\n');
            }
        }
    }
    Ok(())
}

fn emit_try_block(out: &mut String, block: &StmtBlock, ctx: &mut EmitCtx) -> Result<(), RhError> {
    let stmts: Vec<_> = block.iter().collect();
    if stmts.is_empty() {
        out.push_str("        Ok(0)\n");
        return Ok(());
    }
    for (index, stmt) in stmts.iter().enumerate() {
        let is_last = index + 1 == stmts.len();
        emit_try_stmt(out, stmt, ctx, is_last)?;
    }
    Ok(())
}

fn emit_try_stmt(
    out: &mut String,
    stmt: &Stmt,
    ctx: &mut EmitCtx,
    implicit_ok: bool,
) -> Result<(), RhError> {
    match stmt {
        Stmt::Expr(expr) if implicit_ok => {
            out.push_str("        return Ok(");
            emit_expr(out, expr, ctx)?;
            out.push_str(");\n");
        }
        Stmt::Return(Some(expr), flags, ..) if flags.contains(ASTFlags::BREAK) => {
            out.push_str("        return Err(");
            emit_expr(out, expr, ctx)?;
            out.push_str(");\n");
        }
        Stmt::BreakLoop(expr, flags, ..) => {
            if expr.is_some() {
                return Err(RhError::Transpile(
                    "break/continue with value is not in rh-3".into(),
                ));
            }
            if flags.contains(ASTFlags::BREAK) {
                out.push_str("        break;\n");
            } else {
                out.push_str("        continue;\n");
            }
        }
        Stmt::FnCall(call, ..) if call.name == "throw" => {
            out.push_str("        ");
            emit_throw_expr(out, call, ctx)?;
            out.push('\n');
        }
        other => {
            let mut inner = String::new();
            emit_stmt(&mut inner, other, ctx, false)?;
            for line in inner.lines() {
                out.push_str("        ");
                out.push_str(line);
                out.push('\n');
            }
            if implicit_ok {
                out.push_str("        Ok(0)\n");
            }
        }
    }
    Ok(())
}

fn emit_throw_stmt(
    out: &mut String,
    call: &rhai::FnCallExpr,
    ctx: &mut EmitCtx,
    implicit_return: bool,
) -> Result<(), RhError> {
    if ctx.cdylib && !ctx.in_try() && call.args.len() == 1 {
        out.push_str("    return ");
        emit_rh_fail(out, &call.args[0], ctx)?;
        out.push_str(";\n");
        return Ok(());
    }
    if ctx.in_try() {
        out.push_str("    ");
        emit_throw_expr(out, call, ctx)?;
        out.push('\n');
    } else {
        let snippet = format!(
            "throw {};",
            expr_to_rhai(&call.args[0]).unwrap_or_else(|_| "0".into())
        );
        out.push_str("    let _throw = rh_host_eval_int(");
        out.push_str(&format!("{:?}, ", snippet));
        ctx.emit_scope_json_expr(out);
        out.push_str(";\n");
        if implicit_return {
            out.push_str("    return _throw;\n");
        }
    }
    Ok(())
}

fn emit_require_stmt(
    out: &mut String,
    condition: &Expr,
    message: &Expr,
    ctx: &mut EmitCtx,
) -> Result<(), RhError> {
    out.push_str("    if ");
    emit_expr(out, condition, ctx)?;
    out.push_str(" == 0 {\n");
    out.push_str("        return ");
    emit_rh_fail(out, message, ctx)?;
    out.push_str(";\n");
    out.push_str("    }\n");
    Ok(())
}

fn emit_rh_fail(out: &mut String, message: &Expr, ctx: &mut EmitCtx) -> Result<(), RhError> {
    if let Some(literal) = throw_message(message) {
        out.push_str("rh_fail(");
        out.push_str(&format!("{literal:?}"));
        out.push(')');
        return Ok(());
    }
    out.push_str("rh_fail(&");
    emit_stringish(out, message, ctx)?;
    out.push(')');
    Ok(())
}

fn throw_message(expr: &Expr) -> Option<String> {
    match expr {
        Expr::StringConstant(message, ..) => Some(message.to_string()),
        Expr::DynamicConstant(value, ..) if value.is::<rhai::ImmutableString>() => {
            Some(value.clone_cast::<rhai::ImmutableString>().to_string())
        }
        _ => None,
    }
}

fn emit_throw_expr(
    out: &mut String,
    call: &rhai::FnCallExpr,
    ctx: &mut EmitCtx,
) -> Result<(), RhError> {
    if call.args.len() != 1 {
        return Err(RhError::Transpile("throw expects one argument".into()));
    }
    if ctx.in_try() && is_pure_int_expr(&call.args[0]) {
        out.push_str("return Err(");
        emit_expr(out, &call.args[0], ctx)?;
        out.push_str(");");
    } else {
        let snippet = format!(
            "throw {};",
            expr_to_rhai(&call.args[0]).unwrap_or_else(|_| "0".into())
        );
        out.push_str("return Err(rh_host_eval_int(");
        out.push_str(&format!("{:?}, ", snippet));
        ctx.emit_scope_json_expr(out);
        out.push_str(");");
    }
    Ok(())
}

fn infer_binding_kind(expr: &Expr, ctx: &EmitCtx) -> ValueKind {
    match expr {
        Expr::BoolConstant(..) => ValueKind::Bool,
        Expr::StringConstant(..) => ValueKind::String,
        Expr::Variable(ident, ..) => match ctx.scope.get(ident.1.as_str()).copied() {
            Some(kind) => kind,
            None => ValueKind::Int,
        },
        _ if json_parse_arg(expr).is_some() => ValueKind::Json,
        _ if json_value_path(expr, ctx).is_some_and(|(_, path)| !path.is_empty()) => {
            ValueKind::Json
        }
        _ if string_concat_args(expr, ctx).is_some() => ValueKind::String,
        _ if args_index_expr(expr).is_some() => ValueKind::String,
        _ if std_fs_read_to_string_arg(expr).is_some() => ValueKind::String,
        _ if path_join_display_args(expr).is_some() => ValueKind::String,
        _ if uses_host_surface(expr) => ValueKind::Bool,
        _ => ValueKind::Int,
    }
}

fn string_concat_args<'a>(expr: &'a Expr, ctx: &EmitCtx) -> Option<(&'a Expr, &'a Expr)> {
    let Expr::FnCall(call, ..) = expr else {
        return None;
    };
    if !matches!(call.op_token.as_ref(), Some(Token::Plus)) || call.args.len() != 2 {
        return None;
    }
    prefers_string_ops(&call.args[0], &call.args[1], ctx).then_some((&call.args[0], &call.args[1]))
}

const MAX_NATIVE_FOR_SPAN: i64 = 4096;

#[derive(Debug, Clone)]
enum IntForBound {
    Const(i64),
    Expr(Expr),
}

#[derive(Debug, Clone)]
enum IntForPlan {
    Values(Vec<i64>),
    Exclusive {
        start: IntForBound,
        end: IntForBound,
    },
    Inclusive {
        start: IntForBound,
        end: IntForBound,
    },
}

fn int_for_bound(expr: &Expr) -> Option<IntForBound> {
    int_const(expr)
        .map(IntForBound::Const)
        .or_else(|| is_pure_int_expr(expr).then(|| IntForBound::Expr(expr.clone())))
}

fn emit_for_bound(out: &mut String, bound: &IntForBound, ctx: &mut EmitCtx) -> Result<(), RhError> {
    match bound {
        IntForBound::Const(value) => out.push_str(&value.to_string()),
        IntForBound::Expr(expr) => emit_expr(out, expr, ctx)?,
    }
    Ok(())
}

fn int_const(expr: &Expr) -> Option<i64> {
    match expr {
        Expr::IntegerConstant(value, ..) => Some(*value),
        _ => None,
    }
}

fn bounded_exclusive_span(start: i64, end: i64) -> Option<i64> {
    if end <= start {
        return Some(0);
    }
    let span = end.checked_sub(start)?;
    (span <= MAX_NATIVE_FOR_SPAN).then_some(span)
}

fn bounded_inclusive_span(start: i64, end: i64) -> Option<i64> {
    if end < start {
        return Some(0);
    }
    let span = end.checked_sub(start)?.checked_add(1)?;
    (span <= MAX_NATIVE_FOR_SPAN).then_some(span)
}

fn int_for_plan(iterable: &Expr) -> Option<IntForPlan> {
    if let Expr::Array(items, ..) = iterable {
        let mut values = Vec::new();
        for item in items {
            values.push(int_const(item)?);
        }
        return Some(IntForPlan::Values(values));
    }
    let Expr::FnCall(call, ..) = iterable else {
        return None;
    };
    if call.args.len() != 2 {
        return None;
    }
    let start = int_for_bound(&call.args[0])?;
    let end = int_for_bound(&call.args[1])?;
    let is_exclusive = call.name.as_str() == Token::ExclusiveRange.literal_syntax();
    let is_inclusive = call.name.as_str() == Token::InclusiveRange.literal_syntax();
    if !is_exclusive && !is_inclusive {
        return None;
    }
    if let (IntForBound::Const(start), IntForBound::Const(end)) = (&start, &end) {
        if is_exclusive {
            bounded_exclusive_span(*start, *end)?;
        } else {
            bounded_inclusive_span(*start, *end)?;
        }
    }
    if is_exclusive {
        Some(IntForPlan::Exclusive { start, end })
    } else {
        Some(IntForPlan::Inclusive { start, end })
    }
}

fn emit_host_expr(out: &mut String, expr: &Expr, ctx: &EmitCtx) -> Result<(), RhError> {
    let snippet = expr_to_rhai(expr)?;
    out.push_str("rh_host_eval_int(");
    out.push_str(&format!("{:?}, ", snippet));
    ctx.emit_scope_json_expr(out);
    out.push(')');
    Ok(())
}

fn std_fs_exists_arg(expr: &Expr) -> Option<&Expr> {
    std_fs_single_arg(expr, "exists")
}

fn std_fs_exists_case_exact_arg(expr: &Expr) -> Option<&Expr> {
    std_fs_single_arg(expr, "exists_case_exact")
}

fn std_fs_read_to_string_arg(expr: &Expr) -> Option<&Expr> {
    std_fs_single_arg(expr, "read_to_string")
}

fn std_fs_single_arg<'a>(expr: &'a Expr, name: &str) -> Option<&'a Expr> {
    let Expr::FnCall(call, ..) = expr else {
        return None;
    };
    if call.namespace.to_string() != "std::fs" || call.name != name || call.args.len() != 1 {
        return None;
    }
    Some(&call.args[0])
}

fn rh_fail_arg(expr: &Expr) -> Option<&Expr> {
    let Expr::FnCall(call, ..) = expr else {
        return None;
    };
    if call.namespace.to_string() != "rh" || call.name != "fail" || call.args.len() != 1 {
        return None;
    }
    Some(&call.args[0])
}

fn path_join_display_args(expr: &Expr) -> Option<(&Expr, &Expr)> {
    let Expr::Dot(boxed, ..) = expr else {
        return None;
    };
    let display = matches!(
        &boxed.rhs,
        Expr::Property(property, ..) if property.2.as_str() == "display"
    ) || matches!(
        &boxed.rhs,
        Expr::MethodCall(call, ..) if call.name == "display" && call.args.is_empty()
    );
    if !display {
        return None;
    }
    let Expr::FnCall(call, ..) = &boxed.lhs else {
        return None;
    };
    if call.namespace.to_string() != "std::path" || call.name != "join" || call.args.len() != 2 {
        return None;
    }
    Some((&call.args[0], &call.args[1]))
}

fn process_status_args(expr: &Expr) -> Option<(&Expr, &[Expr], &Expr)> {
    let Expr::FnCall(call, ..) = expr else {
        return None;
    };
    if call.namespace.to_string() != "std::process"
        || call.name != "command_status"
        || call.args.len() != 3
    {
        return None;
    }
    let Expr::Array(arguments, ..) = &call.args[1] else {
        return None;
    };
    Some((&call.args[0], arguments, &call.args[2]))
}

fn json_parse_arg(expr: &Expr) -> Option<&Expr> {
    let Expr::FnCall(call, ..) = expr else {
        return None;
    };
    if call.namespace.to_string() != "rhai::json" || call.name != "parse" || call.args.len() != 1 {
        return None;
    }
    Some(&call.args[0])
}

fn json_value_path<'a>(expr: &'a Expr, ctx: &EmitCtx) -> Option<(&'a str, Vec<&'a str>)> {
    match expr {
        Expr::Variable(ident, ..)
            if ctx.scope.get(ident.1.as_str()).copied() == Some(ValueKind::Json) =>
        {
            Some((ident.1.as_str(), Vec::new()))
        }
        Expr::Dot(boxed, ..) => {
            let (binding, mut path) = json_value_path(&boxed.lhs, ctx)?;
            if !append_json_properties(&boxed.rhs, &mut path) {
                return None;
            }
            Some((binding, path))
        }
        _ => None,
    }
}

fn append_json_properties<'a>(expr: &'a Expr, path: &mut Vec<&'a str>) -> bool {
    match expr {
        Expr::Property(property, ..) => {
            path.push(property.2.as_str());
            true
        }
        Expr::Dot(boxed, ..) => {
            append_json_properties(&boxed.lhs, path) && append_json_properties(&boxed.rhs, path)
        }
        _ => false,
    }
}

fn json_array_len_path<'a>(expr: &'a Expr, ctx: &EmitCtx) -> Option<(&'a str, Vec<&'a str>)> {
    let Expr::Dot(boxed, ..) = expr else {
        return None;
    };
    let (binding, mut path) = json_value_path(&boxed.lhs, ctx)?;
    append_json_array_len(&boxed.rhs, &mut path).then_some((binding, path))
}

fn append_json_array_len<'a>(expr: &'a Expr, path: &mut Vec<&'a str>) -> bool {
    match expr {
        Expr::Property(property, ..) if property.2.as_str() == "len" => true,
        Expr::MethodCall(call, ..) if call.name == "len" && call.args.is_empty() => true,
        Expr::Dot(boxed, ..) => {
            append_json_properties(&boxed.lhs, path) && append_json_array_len(&boxed.rhs, path)
        }
        _ => false,
    }
}

fn emit_json_path(out: &mut String, path: &[&str]) {
    out.push_str("&[");
    for (index, segment) in path.iter().enumerate() {
        if index > 0 {
            out.push_str(", ");
        }
        out.push_str(&format!("{segment:?}"));
    }
    out.push(']');
}

fn type_of_arg(expr: &Expr) -> Option<&Expr> {
    let Expr::FnCall(call, ..) = expr else {
        return None;
    };
    if call.namespace.is_empty() && call.name == "type_of" && call.args.len() == 1 {
        Some(&call.args[0])
    } else {
        None
    }
}

fn is_explicit_string_expr(expr: &Expr, ctx: &EmitCtx) -> bool {
    matches!(expr, Expr::StringConstant(..))
        || type_of_arg(expr).is_some()
        || args_index_expr(expr).is_some()
        || matches!(
            expr,
            Expr::Variable(ident, ..)
                if ctx.scope.get(ident.1.as_str()).copied() == Some(ValueKind::String)
        )
}

fn prefers_string_ops(lhs: &Expr, rhs: &Expr, ctx: &EmitCtx) -> bool {
    is_explicit_string_expr(lhs, ctx) || is_explicit_string_expr(rhs, ctx)
}

fn is_native_json_int_expr(expr: &Expr, ctx: &EmitCtx) -> bool {
    if json_array_len_path(expr, ctx).is_some()
        || json_value_path(expr, ctx).is_some_and(|(_, path)| !path.is_empty())
        || type_of_arg(expr).is_some_and(|argument| {
            json_value_path(argument, ctx).is_some()
                || matches!(
                    argument,
                    Expr::Variable(ident, ..)
                        if ctx.scope.get(ident.1.as_str()).copied() == Some(ValueKind::Json)
                )
        })
    {
        return true;
    }
    match expr {
        Expr::IntegerConstant(..) | Expr::BoolConstant(..) | Expr::StringConstant(..) => true,
        Expr::Variable(ident, ..) => matches!(
            ctx.scope.get(ident.1.as_str()),
            Some(ValueKind::Int | ValueKind::Bool | ValueKind::String | ValueKind::Json)
        ),
        Expr::FnCall(call, ..) if call.op_token.is_some() => call
            .args
            .iter()
            .all(|argument| is_native_json_int_expr(argument, ctx)),
        _ => false,
    }
}

fn emit_type_of(out: &mut String, argument: &Expr, ctx: &EmitCtx) -> Result<bool, RhError> {
    if let Some((binding, path)) = json_value_path(argument, ctx) {
        out.push_str("rh_json_type_name(&");
        out.push_str(binding);
        out.push_str(", ");
        emit_json_path(out, &path);
        out.push(')');
        return Ok(true);
    }
    if let Expr::Variable(ident, ..) = argument
        && ctx.scope.get(ident.1.as_str()).copied() == Some(ValueKind::Json)
    {
        out.push_str("rh_json_type_name_value(&");
        out.push_str(ident.1.as_str());
        out.push(')');
        return Ok(true);
    }
    Ok(false)
}

fn emit_stringish(out: &mut String, expr: &Expr, ctx: &mut EmitCtx) -> Result<(), RhError> {
    if let Some((lhs, rhs)) = string_concat_args(expr, ctx) {
        out.push_str("format!(\"{}{}\", ");
        emit_stringish(out, lhs, ctx)?;
        out.push_str(", ");
        emit_stringish(out, rhs, ctx)?;
        out.push(')');
        return Ok(());
    }
    if let Some(argument) = type_of_arg(expr) {
        if emit_type_of(out, argument, ctx)? {
            return Ok(());
        }
        return Err(RhError::Transpile(
            "type_of argument must be a JSON value or JSON path".into(),
        ));
    }
    if let Some((binding, path)) = json_value_path(expr, ctx)
        && !path.is_empty()
    {
        out.push_str("rh_json_string_path(&");
        out.push_str(binding);
        out.push_str(", ");
        emit_json_path(out, &path);
        out.push(')');
        return Ok(());
    }
    if let Some(index) = args_index_expr(expr) {
        out.push_str("rh_arg(");
        emit_expr(out, index, ctx)?;
        out.push(')');
        return Ok(());
    }
    match expr {
        Expr::StringConstant(value, ..) => {
            out.push_str("String::from(");
            out.push_str(&format!("{value:?}"));
            out.push(')');
        }
        Expr::Variable(ident, ..)
            if ctx.scope.get(ident.1.as_str()).copied() == Some(ValueKind::String) =>
        {
            out.push_str(ident.1.as_str());
        }
        Expr::Variable(ident, ..)
            if ctx.scope.get(ident.1.as_str()).copied() == Some(ValueKind::Json) =>
        {
            out.push_str("rh_json_as_str(&");
            out.push_str(ident.1.as_str());
            out.push(')');
        }
        _ => {
            return Err(RhError::Transpile(
                "unsupported string expression in native rh".into(),
            ));
        }
    }
    Ok(())
}

fn emit_intish(out: &mut String, expr: &Expr, ctx: &mut EmitCtx) -> Result<(), RhError> {
    if let Some((binding, path)) = json_array_len_path(expr, ctx) {
        out.push_str("rh_json_array_len(&");
        out.push_str(binding);
        out.push_str(", ");
        emit_json_path(out, &path);
        out.push(')');
        return Ok(());
    }
    if let Some((binding, path)) = json_value_path(expr, ctx)
        && !path.is_empty()
    {
        out.push_str("rh_json_int_path(&");
        out.push_str(binding);
        out.push_str(", ");
        emit_json_path(out, &path);
        out.push(')');
        return Ok(());
    }
    if let Expr::Variable(ident, ..) = expr
        && ctx.scope.get(ident.1.as_str()).copied() == Some(ValueKind::Json)
    {
        out.push_str("rh_json_as_i64(&");
        out.push_str(ident.1.as_str());
        out.push(')');
        return Ok(());
    }
    emit_expr(out, expr, ctx)
}

fn emit_expr(out: &mut String, expr: &Expr, ctx: &mut EmitCtx) -> Result<(), RhError> {
    if ctx.cdylib {
        if let Some(index) = args_index_expr(expr) {
            emit_args_index(out, index, ctx)?;
            return Ok(());
        }
        if let Some(path) = std_fs_exists_arg(expr)
            && emit_std_fs_exists(out, path, ctx)?
        {
            return Ok(());
        }
        if let Some(path) = std_fs_exists_case_exact_arg(expr)
            && emit_std_fs_exists_case_exact(out, path, ctx)?
        {
            return Ok(());
        }
        if let Some(message) = rh_fail_arg(expr) {
            emit_rh_fail(out, message, ctx)?;
            return Ok(());
        }
        if let Some((program, arguments, timeout)) = process_status_args(expr)
            && emit_process_status(out, program, arguments, timeout, ctx)?
        {
            return Ok(());
        }
        if let Some(source) = json_parse_arg(expr)
            && emit_json_parse(out, source, ctx)?
        {
            return Ok(());
        }
        if let Some(argument) = type_of_arg(expr)
            && emit_type_of(out, argument, ctx)?
        {
            return Ok(());
        }
        if let Some((binding, path)) = json_array_len_path(expr, ctx) {
            out.push_str("rh_json_array_len(&");
            out.push_str(binding);
            out.push_str(", ");
            emit_json_path(out, &path);
            out.push(')');
            return Ok(());
        }
        if let Some((binding, path)) = json_value_path(expr, ctx)
            && !path.is_empty()
        {
            out.push_str("rh_json_int_path(&");
            out.push_str(binding);
            out.push_str(", ");
            emit_json_path(out, &path);
            out.push(')');
            return Ok(());
        }
        if let Some(path) = std_fs_read_to_string_arg(expr)
            && emit_std_fs_read_to_string(out, path, ctx)?
        {
            return Ok(());
        }
        if emit_string_predicate(out, expr, ctx)? {
            return Ok(());
        }
        if emit_string_mut_expr(out, expr, ctx)? {
            return Ok(());
        }
        if let Some((base, child)) = path_join_display_args(expr)
            && emit_path_join(out, base, child, ctx)?
        {
            return Ok(());
        }
        if let Some(call) = parse_fleet_call(expr) {
            validate_fleet_call(&call)?;
            let params = fleet_params_json(&call)?;
            out.push_str("rh_fleet_call(");
            out.push_str(&format!("{:?}, {:?}", call.operation_id, params));
            out.push(')');
            return Ok(());
        }
        if let Expr::FnCall(call, ..) = expr
            && call.op_token.is_some()
        {
            return emit_call(out, call, ctx);
        }
        if let Expr::And(args, ..) = expr {
            return logical_nary(out, "&&", args, ctx);
        }
        if let Expr::Or(args, ..) = expr {
            return logical_nary(out, "||", args, ctx);
        }
        if !is_pure_int_expr(expr)
            && !is_native_json_int_expr(expr, ctx)
            && (uses_host_surface(expr)
                || !matches!(
                    expr,
                    Expr::IntegerConstant(..) | Expr::BoolConstant(..) | Expr::Variable(..)
                ))
        {
            return emit_host_expr(out, expr, ctx);
        }
    }
    match expr {
        Expr::IntegerConstant(value, ..) => {
            if ctx.cdylib {
                out.push_str(&value.to_string());
            } else {
                out.push_str("Dynamic::from(");
                out.push_str(&value.to_string());
                out.push(')');
            }
        }
        Expr::BoolConstant(value, ..) => {
            if ctx.cdylib {
                out.push_str(if *value { "1" } else { "0" });
            } else {
                out.push_str("Dynamic::from(");
                out.push_str(if *value { "true" } else { "false" });
                out.push(')');
            }
        }
        Expr::StringConstant(value, ..) => {
            if ctx.cdylib {
                out.push_str("String::from(");
                out.push_str(&format!("{value:?}"));
                out.push(')');
            } else {
                out.push_str("Dynamic::from(");
                out.push_str(&format!("{value:?}"));
                out.push(')');
            }
        }
        Expr::Unit(..) => out.push_str(ctx.unit_expr()),
        Expr::Variable(ident, ..) => out.push_str(ident.1.as_str()),
        Expr::Dot(..) if is_args_len_expr(expr) => out.push_str("rh_args_len()"),
        Expr::Dot(..)
            if var_len_name(expr)
                .is_some_and(|name| ctx.scope.get(name).copied() == Some(ValueKind::String)) =>
        {
            out.push('(');
            out.push_str(var_len_name(expr).expect("checked string binding"));
            out.push_str(".chars().count() as INT)");
        }
        Expr::Dot(..) if is_var_len_expr(expr) => emit_host_expr(out, expr, ctx)?,
        Expr::FnCall(call, ..) => emit_call(out, call, ctx)?,
        Expr::Stmt(block) => {
            out.push_str("{ ");
            emit_block(out, block, ctx, true)?;
            out.push_str(" }");
        }
        other if ctx.cdylib && uses_host_surface(other) => emit_host_expr(out, expr, ctx)?,
        other => {
            return Err(RhError::Transpile(format!(
                "unsupported expression in rh-2: {other:?}"
            )));
        }
    }
    Ok(())
}

fn emit_std_fs_exists(out: &mut String, path: &Expr, ctx: &mut EmitCtx) -> Result<bool, RhError> {
    let mut path_expr = String::new();
    if !emit_native_string(&mut path_expr, path, ctx)? {
        return Ok(false);
    }
    out.push_str("rh_std_fs_exists(");
    out.push_str(&path_expr);
    out.push(')');
    Ok(true)
}

fn emit_std_fs_read_to_string(
    out: &mut String,
    path: &Expr,
    ctx: &mut EmitCtx,
) -> Result<bool, RhError> {
    let mut path_expr = String::new();
    if !emit_native_string(&mut path_expr, path, ctx)? {
        return Ok(false);
    }
    out.push_str("rh_std_fs_read_to_string(");
    out.push_str(&path_expr);
    out.push(')');
    Ok(true)
}

fn emit_std_fs_exists_case_exact(
    out: &mut String,
    path: &Expr,
    ctx: &mut EmitCtx,
) -> Result<bool, RhError> {
    let mut path_expr = String::new();
    if !emit_native_string(&mut path_expr, path, ctx)? {
        return Ok(false);
    }
    out.push_str("rh_std_fs_exists_case_exact(");
    out.push_str(&path_expr);
    out.push(')');
    Ok(true)
}

fn emit_process_status(
    out: &mut String,
    program: &Expr,
    arguments: &[Expr],
    timeout: &Expr,
    ctx: &mut EmitCtx,
) -> Result<bool, RhError> {
    let mut program_expr = String::new();
    if !emit_native_string(&mut program_expr, program, ctx)? || !is_pure_int_expr(timeout) {
        return Ok(false);
    }
    let mut argument_exprs = Vec::with_capacity(arguments.len());
    for argument in arguments {
        let mut argument_expr = String::new();
        if !emit_native_string(&mut argument_expr, argument, ctx)? {
            return Ok(false);
        }
        argument_exprs.push(argument_expr);
    }
    out.push_str("rh_process_status(");
    out.push_str(&program_expr);
    out.push_str(", &vec![");
    for (index, argument) in argument_exprs.iter().enumerate() {
        if index > 0 {
            out.push_str(", ");
        }
        out.push_str("String::from(");
        out.push_str(argument);
        out.push(')');
    }
    out.push_str("], ");
    emit_expr(out, timeout, ctx)?;
    out.push(')');
    Ok(true)
}

fn emit_json_parse(out: &mut String, source: &Expr, ctx: &mut EmitCtx) -> Result<bool, RhError> {
    let mut source_expr = String::new();
    if !emit_native_string(&mut source_expr, source, ctx)? {
        return Ok(false);
    }
    out.push_str("rh_json_parse(");
    out.push_str(&source_expr);
    out.push(')');
    Ok(true)
}

fn emit_native_string(out: &mut String, expr: &Expr, ctx: &mut EmitCtx) -> Result<bool, RhError> {
    match expr {
        Expr::StringConstant(value, ..) => out.push_str(&format!("{value:?}")),
        Expr::Variable(ident, ..)
            if ctx.scope.get(ident.1.as_str()).copied() == Some(ValueKind::String) =>
        {
            out.push('&');
            out.push_str(ident.1.as_str());
        }
        _ if args_index_expr(expr).is_some() => {
            out.push('&');
            emit_args_index(out, args_index_expr(expr).expect("checked args index"), ctx)?;
        }
        _ => return Ok(false),
    }
    Ok(true)
}

fn string_for_binding<'a>(expr: &'a Expr, ctx: &'a EmitCtx) -> Option<&'a str> {
    let Expr::Variable(ident, ..) = expr else {
        return None;
    };
    (ctx.scope.get(ident.1.as_str()).copied() == Some(ValueKind::String))
        .then_some(ident.1.as_str())
}

#[derive(Clone, Copy)]
enum StringReceiver<'a> {
    Binding(&'a str),
    Literal(&'a str),
}

fn parse_string_method_call<'a>(
    expr: &'a Expr,
    ctx: &EmitCtx,
) -> Option<(StringReceiver<'a>, &'a rhai::FnCallExpr)> {
    let Expr::Dot(boxed, ..) = expr else {
        return None;
    };
    let receiver = match &boxed.lhs {
        Expr::Variable(ident, ..)
            if ctx.scope.get(ident.1.as_str()).copied() == Some(ValueKind::String) =>
        {
            StringReceiver::Binding(ident.1.as_str())
        }
        Expr::StringConstant(value, ..) => StringReceiver::Literal(value.as_str()),
        _ => return None,
    };
    let Expr::MethodCall(call, ..) = &boxed.rhs else {
        return None;
    };
    Some((receiver, call))
}

fn emit_string_receiver(out: &mut String, receiver: StringReceiver<'_>) {
    match receiver {
        StringReceiver::Binding(name) => out.push_str(name),
        StringReceiver::Literal(value) => {
            out.push_str(&format!("{value:?}"));
        }
    }
}

fn char_to_string_binding<'a>(expr: &'a Expr, ctx: &'a EmitCtx) -> Option<&'a str> {
    let Expr::Dot(boxed, ..) = expr else {
        return None;
    };
    let Expr::Variable(ident, ..) = &boxed.lhs else {
        return None;
    };
    if ctx.scope.get(ident.1.as_str()).copied() != Some(ValueKind::Char) {
        return None;
    }
    let Expr::MethodCall(call, ..) = &boxed.rhs else {
        return None;
    };
    (call.name == "to_string" && call.args.is_empty()).then_some(ident.1.as_str())
}

fn emit_string_needle(out: &mut String, expr: &Expr, ctx: &EmitCtx) -> Result<bool, RhError> {
    match expr {
        Expr::StringConstant(value, ..) => {
            out.push_str(&format!("{value:?}"));
            Ok(true)
        }
        Expr::Variable(ident, ..)
            if ctx.scope.get(ident.1.as_str()).copied() == Some(ValueKind::String) =>
        {
            out.push_str(ident.1.as_str());
            out.push_str(".as_str()");
            Ok(true)
        }
        _ if char_to_string_binding(expr, ctx).is_some() => {
            let binding = char_to_string_binding(expr, ctx).expect("checked char to_string");
            out.push('&');
            out.push_str(binding);
            out.push_str(".to_string()");
            Ok(true)
        }
        _ => Ok(false),
    }
}

fn emit_string_predicate(
    out: &mut String,
    expr: &Expr,
    ctx: &mut EmitCtx,
) -> Result<bool, RhError> {
    let Some((receiver, call)) = parse_string_method_call(expr, ctx) else {
        return Ok(false);
    };
    if !matches!(call.name.as_str(), "contains" | "starts_with" | "ends_with")
        || call.args.len() != 1
    {
        return Ok(false);
    }
    let mut needle = String::new();
    if !emit_string_needle(&mut needle, &call.args[0], ctx)? {
        return Ok(false);
    }
    out.push('(');
    emit_string_receiver(out, receiver);
    out.push('.');
    out.push_str(call.name.as_str());
    out.push('(');
    out.push_str(&needle);
    out.push_str(") as INT)");
    Ok(true)
}

fn emit_string_mut_expr(out: &mut String, expr: &Expr, ctx: &mut EmitCtx) -> Result<bool, RhError> {
    let Some((StringReceiver::Binding(binding), call)) = parse_string_method_call(expr, ctx) else {
        return Ok(false);
    };
    match call.name.as_str() {
        "trim" if call.args.is_empty() => {
            out.push_str(binding);
            out.push_str(" = ");
            out.push_str(binding);
            out.push_str(".trim().to_string()");
            Ok(true)
        }
        "replace" if call.args.len() == 2 => {
            let mut from = String::new();
            let mut to = String::new();
            if !emit_native_string(&mut from, &call.args[0], ctx)?
                || !emit_native_string(&mut to, &call.args[1], ctx)?
            {
                return Ok(false);
            }
            out.push_str(binding);
            out.push_str(" = ");
            out.push_str(binding);
            out.push_str(".replace(");
            out.push_str(&from);
            out.push_str(", ");
            out.push_str(&to);
            out.push(')');
            Ok(true)
        }
        _ => Ok(false),
    }
}

fn emit_string_mut_stmt(out: &mut String, expr: &Expr, ctx: &mut EmitCtx) -> Result<bool, RhError> {
    let mut inner = String::new();
    if !emit_string_mut_expr(&mut inner, expr, ctx)? {
        return Ok(false);
    }
    out.push_str("    ");
    out.push_str(&inner);
    out.push_str(";\n");
    Ok(true)
}

fn emit_path_join(
    out: &mut String,
    base: &Expr,
    child: &Expr,
    ctx: &mut EmitCtx,
) -> Result<bool, RhError> {
    let mut base_expr = String::new();
    let mut child_expr = String::new();
    if !emit_native_string(&mut base_expr, base, ctx)?
        || !emit_native_string(&mut child_expr, child, ctx)?
    {
        return Ok(false);
    }
    out.push_str("rh_path_join(");
    out.push_str(&base_expr);
    out.push_str(", ");
    out.push_str(&child_expr);
    out.push(')');
    Ok(true)
}

fn emit_args_index(out: &mut String, index: &Expr, ctx: &mut EmitCtx) -> Result<(), RhError> {
    if !is_pure_int_expr(index) {
        return Err(RhError::Transpile(
            "native args index must be a pure integer expression".into(),
        ));
    }
    out.push_str("rh_arg(");
    emit_expr(out, index, ctx)?;
    out.push(')');
    Ok(())
}

fn emit_call(out: &mut String, call: &rhai::FnCallExpr, ctx: &mut EmitCtx) -> Result<(), RhError> {
    if let Some(op) = &call.op_token {
        return emit_op(out, op, &call.args, ctx);
    }
    if call.namespace.is_empty() && call.name == "type_of" && call.args.len() == 1 {
        if emit_type_of(out, &call.args[0], ctx)? {
            return Ok(());
        }
        return Err(RhError::Transpile(
            "type_of argument must be a JSON value or JSON path".into(),
        ));
    }
    if call.name == "print" && !ctx.cdylib {
        out.push_str("println!(");
        for (index, arg) in call.args.iter().enumerate() {
            if index > 0 {
                out.push_str(", ");
            }
            emit_expr(out, arg, ctx)?;
        }
        out.push(')');
        return Ok(());
    }
    if call.name == "throw" && ctx.cdylib {
        if !ctx.in_try() && call.args.len() == 1 {
            out.push_str("return ");
            emit_rh_fail(out, &call.args[0], ctx)?;
            return Ok(());
        }
        if ctx.in_try() {
            return emit_throw_expr(out, call, ctx);
        }
        let mut snippet = String::from("throw ");
        snippet.push_str(&expr_to_rhai(&call.args[0])?);
        out.push_str("rh_host_eval_int(");
        out.push_str(&format!("{:?}, ", snippet));
        ctx.emit_scope_json_expr(out);
        return Ok(());
    }
    if call.namespace.is_empty() && call.name == "require" && call.args.len() == 2 && ctx.cdylib {
        out.push_str("{\n");
        emit_require_stmt(out, &call.args[0], &call.args[1], ctx)?;
        out.push_str("    0\n}");
        return Ok(());
    }
    Err(RhError::Transpile(format!(
        "unsupported call `{}` in rh-2",
        call.name
    )))
}

fn emit_op(out: &mut String, op: &Token, args: &[Expr], ctx: &mut EmitCtx) -> Result<(), RhError> {
    match (op, args.len()) {
        (Token::Plus, 2) if ctx.cdylib && prefers_string_ops(&args[0], &args[1], ctx) => {
            out.push_str("format!(\"{}{}\", ");
            emit_stringish(out, &args[0], ctx)?;
            out.push_str(", ");
            emit_stringish(out, &args[1], ctx)?;
            out.push(')');
            Ok(())
        }
        (Token::Plus, 2) => int_binary(out, "+", &args[0], &args[1], ctx),
        (Token::Minus, 2) => int_binary(out, "-", &args[0], &args[1], ctx),
        (Token::Multiply, 2) => int_binary(out, "*", &args[0], &args[1], ctx),
        (Token::Divide, 2) => int_binary(out, "/", &args[0], &args[1], ctx),
        (Token::Modulo, 2) => int_binary(out, "%", &args[0], &args[1], ctx),
        (Token::Equals, 2) | (Token::EqualsTo, 2) => {
            comparison_binary(out, "==", &args[0], &args[1], ctx)
        }
        (Token::NotEqualsTo, 2) => comparison_binary(out, "!=", &args[0], &args[1], ctx),
        (Token::GreaterThan, 2) => comparison_binary(out, ">", &args[0], &args[1], ctx),
        (Token::GreaterThanEqualsTo, 2) => comparison_binary(out, ">=", &args[0], &args[1], ctx),
        (Token::LessThan, 2) => comparison_binary(out, "<", &args[0], &args[1], ctx),
        (Token::LessThanEqualsTo, 2) => comparison_binary(out, "<=", &args[0], &args[1], ctx),
        (Token::And, 2) => logical_binary(out, "&&", &args[0], &args[1], ctx),
        (Token::Or, 2) => logical_binary(out, "||", &args[0], &args[1], ctx),
        (Token::And, n) if ctx.cdylib && n > 2 => logical_nary(out, "&&", args, ctx),
        (Token::Or, n) if ctx.cdylib && n > 2 => logical_nary(out, "||", args, ctx),
        (Token::Minus, 1) => {
            out.push_str("(-(");
            emit_intish(out, &args[0], ctx)?;
            out.push_str("))");
            Ok(())
        }
        (Token::Bang, 1) if ctx.cdylib => {
            out.push_str("(((");
            emit_expr(out, &args[0], ctx)?;
            out.push_str(" == 0)) as INT)");
            Ok(())
        }
        (Token::Bang, 1) => {
            out.push_str("(!(");
            emit_expr(out, &args[0], ctx)?;
            out.push_str("))");
            Ok(())
        }
        _ => Err(RhError::Transpile(format!(
            "unsupported operator `{op:?}` in rh-0"
        ))),
    }
}

fn comparison_binary(
    out: &mut String,
    op: &str,
    lhs: &Expr,
    rhs: &Expr,
    ctx: &mut EmitCtx,
) -> Result<(), RhError> {
    if ctx.cdylib {
        out.push('(');
        if prefers_string_ops(lhs, rhs, ctx) {
            emit_stringish(out, lhs, ctx)?;
            out.push(' ');
            out.push_str(op);
            out.push(' ');
            emit_stringish(out, rhs, ctx)?;
        } else {
            int_binary(out, op, lhs, rhs, ctx)?;
        }
        out.push_str(") as INT");
    } else {
        binary(out, op, lhs, rhs, ctx)?;
    }
    Ok(())
}

fn int_binary(
    out: &mut String,
    op: &str,
    lhs: &Expr,
    rhs: &Expr,
    ctx: &mut EmitCtx,
) -> Result<(), RhError> {
    out.push('(');
    emit_intish(out, lhs, ctx)?;
    out.push(' ');
    out.push_str(op);
    out.push(' ');
    emit_intish(out, rhs, ctx)?;
    out.push(')');
    Ok(())
}

fn logical_binary(
    out: &mut String,
    op: &str,
    lhs: &Expr,
    rhs: &Expr,
    ctx: &mut EmitCtx,
) -> Result<(), RhError> {
    if ctx.cdylib {
        out.push_str("(((");
        emit_expr(out, lhs, ctx)?;
        out.push_str(" != 0) ");
        out.push_str(op);
        out.push_str(" (");
        emit_expr(out, rhs, ctx)?;
        out.push_str(" != 0)) as INT");
    } else {
        binary(out, op, lhs, rhs, ctx)?;
    }
    Ok(())
}

fn logical_nary(
    out: &mut String,
    op: &str,
    args: &[Expr],
    ctx: &mut EmitCtx,
) -> Result<(), RhError> {
    out.push('(');
    for (index, arg) in args.iter().enumerate() {
        if index > 0 {
            out.push(' ');
            out.push_str(op);
            out.push(' ');
        }
        out.push('(');
        emit_expr(out, arg, ctx)?;
        out.push_str(" != 0)");
    }
    out.push_str(") as INT");
    Ok(())
}

fn binary(
    out: &mut String,
    op: &str,
    lhs: &Expr,
    rhs: &Expr,
    ctx: &mut EmitCtx,
) -> Result<(), RhError> {
    out.push('(');
    emit_expr(out, lhs, ctx)?;
    out.push(' ');
    out.push_str(op);
    out.push(' ');
    emit_expr(out, rhs, ctx)?;
    out.push(')');
    Ok(())
}

#[cfg(test)]
mod tests {
    use rhai::Stmt;

    use super::{CdylibExecutionMode, transpile, transpile_cdylib, transpile_cdylib_with_mode};

    #[test]
    fn transpiles_add_fn() {
        let rust = transpile("fn add(a, b) { a + b }").expect("transpile");
        assert!(rust.contains("pub fn add"));
        assert!(rust.contains("a + b"));
    }

    #[test]
    fn cdylib_transpile_uses_int_and_unsafe_export() {
        let rust = transpile_cdylib("fn entry() { 42 }").expect("transpile");
        assert!(rust.contains("fn entry() -> INT"));
        assert!(rust.contains("type INT = i64;"));
        assert!(!rust.contains("rhai::"));
        assert!(rust.contains("#[no_mangle]"));
    }

    #[test]
    fn compat_delegating_pack_uses_owned_int_abi() {
        let rust =
            transpile_cdylib("fn entry() { switch 1 { 1 => 42, _ => 0 } }").expect("transpile");
        assert!(rust.contains("compat delegating"));
        assert!(rust.contains("type INT = i64;"));
        assert!(!rust.contains("rhai::"));
    }

    #[test]
    fn cdylib_transpile_emits_while_loop() {
        let source = include_str!("../../../fixtures/rh/while.rh");
        let rust = transpile_cdylib(source).expect("transpile");
        assert!(rust.contains("while "), "expected while in:\n{rust}");
    }

    #[test]
    fn cdylib_transpile_emits_std_fs_exists_literal_fast_path() {
        let source = include_str!("../../../fixtures/rh/stdlib.rh");
        let rust = transpile_cdylib(source).expect("transpile");
        assert!(rust.contains("rh_std_fs_exists(\"/tmp\")"));
        assert!(!rust.contains("rh_host_eval_int(\"std::fs::exists"));
    }

    #[test]
    fn cdylib_transpile_emits_std_fs_exists_arg_binding_fast_path() {
        let output =
            transpile_cdylib_with_mode("fn entry() { let path = args[0]; std::fs::exists(path) }")
                .expect("transpile");
        assert_eq!(
            output.execution_mode,
            CdylibExecutionMode::Native,
            "{}",
            output.rust
        );
        assert!(output.rust.contains("rh_std_fs_exists(&path)"));
    }

    #[test]
    fn cdylib_transpile_emits_read_and_contains_fast_path() {
        let output = transpile_cdylib_with_mode(
            "fn entry() { let path = args[0]; let text = std::fs::read_to_string(path); text.contains(\"agenterm\") }",
        )
        .expect("transpile");
        assert_eq!(
            output.execution_mode,
            CdylibExecutionMode::Native,
            "{}",
            output.rust
        );
        assert!(output.rust.contains("rh_std_fs_read_to_string(&path)"));
        assert!(output.rust.contains("text.contains(\"agenterm\") as INT"));
    }

    #[test]
    fn cdylib_transpile_emits_string_methods_and_char_iteration() {
        let source = include_str!("../../../fixtures/rh/string-validate.rh");
        let output = transpile_cdylib_with_mode(source).expect("transpile");
        assert_eq!(
            output.execution_mode,
            CdylibExecutionMode::Native,
            "{}",
            output.rust
        );
        assert!(output.rust.contains("for character in role.chars()"));
        assert!(output.rust.contains("name.starts_with("));
        assert!(output.rust.contains("name.ends_with("));
        assert!(output.rust.contains("role.replace("));
        assert!(!output.rust.contains("rh_host_eval_int(\"for"));
    }

    #[test]
    fn cdylib_transpile_emits_path_join_display_fast_path() {
        let output = transpile_cdylib_with_mode(
            "fn entry() { let root = args[0]; let path = std::path::join(root, \"Cargo.toml\").display; std::fs::exists(path) }",
        )
        .expect("transpile");
        assert_eq!(
            output.execution_mode,
            CdylibExecutionMode::Native,
            "{}",
            output.rust
        );
        assert!(output.rust.contains("rh_path_join(&root, \"Cargo.toml\")"));
        assert!(output.rust.contains("rh_std_fs_exists(&path)"));
    }

    #[test]
    fn cdylib_transpile_emits_for_int_range() {
        let source = include_str!("../../../fixtures/rh/for-range.rh");
        let rust = transpile_cdylib(source).expect("transpile");
        assert!(
            rust.contains("for value in 1..5"),
            "expected native for-range in:\n{rust}"
        );
    }

    #[test]
    fn fleet_ast_is_dot_method_chain() {
        let ast = super::parse("fn entry() { fleet.protocol.info() }").expect("parse");
        let def = ast.iter_fn_def().next().expect("fn");
        let stmt = def.body.iter().next().expect("stmt");
        let call = match stmt {
            Stmt::Expr(expr) => {
                super::super::fleet::parse_fleet_call(expr.as_ref()).expect("fleet")
            }
            other => panic!("unexpected stmt: {other:?}"),
        };
        assert_eq!(call.operation_id, "protocol.info");
    }

    #[test]
    fn cdylib_transpile_emits_for_dyn_int_range() {
        let source = include_str!("../../../fixtures/rh/for-dyn-range.rh");
        let rust = transpile_cdylib(source).expect("transpile");
        assert!(
            rust.contains("for value in 1..limit"),
            "expected native dynamic for-range in:\n{rust}"
        );
        assert!(!rust.contains("rh_host_eval_int(\"for"));
    }

    #[test]
    fn cdylib_transpile_emits_for_var_len_range() {
        let rust = transpile_cdylib(
            "fn entry() { let total = 0; for index in 1..args.len { total += index; } total }",
        )
        .expect("transpile");
        assert!(
            rust.contains("for index in 1.."),
            "expected native for-range in:\n{rust}"
        );
        assert!(
            rust.contains("rh_args_len()"),
            "expected native args length bound in:\n{rust}"
        );
        assert!(!rust.contains("rh_host_eval_int(\"for"));
    }

    #[test]
    fn int_for_plan_skips_span_check_for_dynamic_bounds() {
        let ast = super::parse("fn entry() { for i in 0..count { 0 } }").expect("parse");
        let def = ast.iter_fn_def().next().expect("fn");
        let stmt = def.body.iter().next().expect("stmt");
        let rhai::Stmt::For(boxed, ..) = stmt else {
            panic!("expected for stmt");
        };
        let (_, _, flow) = boxed.as_ref();
        let plan = super::int_for_plan(&flow.expr).expect("plan");
        assert!(matches!(
            plan,
            super::IntForPlan::Exclusive {
                start: super::IntForBound::Const(0),
                end: super::IntForBound::Expr(_),
            }
        ));
    }

    #[test]
    fn cdylib_emits_native_args_len() {
        let rust = transpile_cdylib("fn entry() { args.len() }").expect("transpile");
        let entry = rust
            .split_once("pub fn entry() -> INT {")
            .and_then(|(_, suffix)| suffix.split_once("fn rh_entry_internal()"))
            .map(|(entry, _)| entry)
            .expect("entry");
        assert!(entry.contains("rh_args_len()"), "{entry}");
        assert!(!entry.contains("rh_host_eval_int"), "{entry}");
        assert!(!entry.contains("rh_host_run_script"), "{entry}");
    }

    #[test]
    fn cdylib_emits_native_utf8_arg_and_string_len() {
        let output =
            transpile_cdylib_with_mode("fn entry() { let first = args[0]; args.len + first.len }")
                .expect("transpile");
        assert_eq!(
            output.execution_mode,
            CdylibExecutionMode::Native,
            "{}",
            output.rust
        );
        let entry = output
            .rust
            .split_once("pub fn entry() -> INT {")
            .and_then(|(_, suffix)| suffix.split_once("fn rh_entry_internal()"))
            .map(|(entry, _)| entry)
            .expect("entry");
        assert!(entry.contains("rh_arg(0)"), "{entry}");
        assert!(entry.contains("first.chars().count() as INT"), "{entry}");
        assert!(!entry.contains("rh_host_eval_int"), "{entry}");
    }

    #[test]
    fn transpiles_fleet_protocol_info_for_cdylib() {
        let rust = transpile_cdylib("fn entry() { fleet.protocol.info(); 9 }").expect("transpile");
        assert!(rust.contains("rh_fleet_call"));
        assert!(rust.contains("protocol.info"));
    }
}
