use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use rhai::{AST, ASTFlags, BinaryExpr, Expr, OpAssignment, ScriptFuncDef, Stmt, StmtBlock, Token};

use crate::{
    RhError,
    bundle::bundle_project_source,
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
    Set,
    StringList,
    Metadata,
    DirEntry,
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
    local_fns: BTreeSet<String>,
    local_fn_sigs: BTreeMap<String, Vec<ValueKind>>,
    try_depth: u32,
}

impl EmitCtx {
    fn new(cdylib: bool) -> Self {
        Self {
            cdylib,
            scope: BTreeMap::new(),
            local_fns: BTreeSet::new(),
            local_fn_sigs: BTreeMap::new(),
            try_depth: 0,
        }
    }

    fn with_local_fns(mut self, local_fns: BTreeSet<String>) -> Self {
        self.local_fns = local_fns;
        self
    }

    fn with_local_fn_sigs(mut self, local_fn_sigs: BTreeMap<String, Vec<ValueKind>>) -> Self {
        self.local_fn_sigs = local_fn_sigs;
        self
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
                ValueKind::Set | ValueKind::StringList => {
                    out.push_str(&format!(
                        "\\\"{name}\\\":{{\\\"kind\\\":\\\"json\\\",\\\"value\\\":{{}}}}"
                    ));
                }
                ValueKind::Metadata => {
                    out.push_str(&format!(
                        "\\\"{name}\\\":{{\\\"kind\\\":\\\"json\\\",\\\"value\\\":{{}}}}"
                    ));
                }
                ValueKind::DirEntry => {
                    out.push_str(&format!(
                        "\\\"{name}\\\":{{\\\"kind\\\":\\\"json\\\",\\\"value\\\":{{}}}}"
                    ));
                }
            }
        }
        out.push_str("}}}}\"");
        for (name, kind) in &self.scope {
            out.push_str(", ");
            if matches!(
                kind,
                ValueKind::String | ValueKind::Json | ValueKind::StringList
            ) {
                out.push_str("serde_json::to_string(&");
                out.push_str(name);
                out.push_str(").unwrap_or_else(|_| \"\\\"\\\"\".to_owned())");
            } else if matches!(kind, ValueKind::Set) {
                out.push_str("serde_json::to_string(&");
                out.push_str(name);
                out.push_str(
                    ".iter().cloned().collect::<Vec<_>>()).unwrap_or_else(|_| \"[]\".to_owned())",
                );
            } else if matches!(kind, ValueKind::Metadata | ValueKind::DirEntry) {
                out.push_str("\"{}\"");
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

/// Bundle project-relative imports, then transpile the flattened source.
pub fn transpile_cdylib_with_project(
    project_root: &Path,
    source: &str,
) -> Result<CdylibTranspileOutput, RhError> {
    let bundled = bundle_project_source(project_root, source)?;
    transpile_cdylib_with_mode(&bundled)
}

fn local_fn_names(ast: &AST) -> BTreeSet<String> {
    ast.iter_functions()
        .filter(|meta| meta.name != "entry" && meta.name != "cc_lines")
        .map(|meta| meta.name.to_string())
        .collect()
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

    let local_fns = local_fn_names(ast);
    let sig_probe = ctx.clone().with_local_fns(local_fns.clone());
    let mut local_fn_sigs = BTreeMap::new();
    let mut local_defs: Vec<&ScriptFuncDef> = Vec::new();
    for meta in ast.iter_functions() {
        if meta.name == "entry" || meta.name == "cc_lines" {
            continue;
        }
        if let Some(def) = find_fn_def(ast, meta.name) {
            local_fn_sigs.insert(meta.name.to_string(), infer_param_kinds(def, &sig_probe));
            local_defs.push(def);
        }
    }
    propagate_local_fn_param_kinds(&local_defs, &mut local_fn_sigs);
    let mut base_ctx = ctx
        .clone()
        .with_local_fns(local_fns)
        .with_local_fn_sigs(local_fn_sigs);
    let mut wrote_fn = false;
    let mut has_entry = false;
    for meta in ast.iter_functions() {
        if meta.name == "entry" {
            has_entry = true;
        }
        if base_ctx.cdylib && meta.name == "cc_lines" {
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
        emit_fn(&mut out, def, &mut base_ctx)?;
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
    fn_ctx.scope.clear();
    // Prefer the shared (possibly inter-procedurally upgraded) signature so
    // call-site emit_string_arg/emit_json_arg matches the generated Rust params.
    let param_kinds = ctx
        .local_fn_sigs
        .get(def.name.as_str())
        .cloned()
        .unwrap_or_else(|| infer_param_kinds(def, &fn_ctx));
    for (param, kind) in def.params.iter().zip(param_kinds.iter().copied()) {
        fn_ctx = fn_ctx.with_binding(param.as_str(), kind);
    }
    out.push_str("pub fn ");
    out.push_str(def.name.as_str());
    out.push('(');
    for (index, (param, kind)) in def.params.iter().zip(param_kinds.iter()).enumerate() {
        if index > 0 {
            out.push_str(", ");
        }
        if fn_ctx.cdylib {
            out.push_str(param.as_str());
            out.push_str(match kind {
                ValueKind::String => ": String",
                ValueKind::Json => ": serde_json::Value",
                _ => ": INT",
            });
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

fn infer_param_kinds(def: &ScriptFuncDef, ctx: &EmitCtx) -> Vec<ValueKind> {
    def.params
        .iter()
        .map(|param| {
            if param_used_as_json(def, param.as_str(), ctx) {
                ValueKind::Json
            } else if param_used_as_string(def, param.as_str(), ctx) {
                ValueKind::String
            } else {
                ValueKind::Int
            }
        })
        .collect()
}

fn propagate_local_fn_param_kinds(
    defs: &[&ScriptFuncDef],
    local_fn_sigs: &mut BTreeMap<String, Vec<ValueKind>>,
) {
    let mut changed = true;
    while changed {
        changed = false;
        for def in defs {
            let upgrades = collect_param_kind_upgrades(def, local_fn_sigs);
            let Some(sig) = local_fn_sigs.get_mut(def.name.as_str()) else {
                continue;
            };
            for (index, kind) in upgrades {
                if index >= sig.len() {
                    continue;
                }
                if sig[index] == ValueKind::Int
                    && matches!(kind, ValueKind::String | ValueKind::Json)
                {
                    sig[index] = kind;
                    changed = true;
                }
            }
        }
    }
}

fn collect_param_kind_upgrades(
    def: &ScriptFuncDef,
    local_fn_sigs: &BTreeMap<String, Vec<ValueKind>>,
) -> Vec<(usize, ValueKind)> {
    let mut upgrades = Vec::new();
    for stmt in def.body.iter() {
        collect_param_kind_upgrades_in_stmt(stmt, def, local_fn_sigs, &mut upgrades);
    }
    upgrades
}

fn collect_param_kind_upgrades_in_stmt(
    stmt: &Stmt,
    def: &ScriptFuncDef,
    local_fn_sigs: &BTreeMap<String, Vec<ValueKind>>,
    upgrades: &mut Vec<(usize, ValueKind)>,
) {
    match stmt {
        Stmt::Expr(expr) | Stmt::Return(Some(expr), ..) => {
            collect_param_kind_upgrades_in_expr(expr.as_ref(), def, local_fn_sigs, upgrades);
        }
        Stmt::Var(boxed, ..) => {
            collect_param_kind_upgrades_in_expr(&boxed.1, def, local_fn_sigs, upgrades);
        }
        Stmt::Assignment(boxed, ..) => {
            collect_param_kind_upgrades_in_expr(&boxed.1.lhs, def, local_fn_sigs, upgrades);
            collect_param_kind_upgrades_in_expr(&boxed.1.rhs, def, local_fn_sigs, upgrades);
        }
        Stmt::If(boxed, ..) => {
            let flow = boxed.as_ref();
            collect_param_kind_upgrades_in_expr(&flow.expr, def, local_fn_sigs, upgrades);
            for stmt in flow.body.iter() {
                collect_param_kind_upgrades_in_stmt(stmt, def, local_fn_sigs, upgrades);
            }
            for stmt in flow.branch.iter() {
                collect_param_kind_upgrades_in_stmt(stmt, def, local_fn_sigs, upgrades);
            }
        }
        Stmt::For(boxed, ..) => {
            let (_, _, flow) = boxed.as_ref();
            collect_param_kind_upgrades_in_expr(&flow.expr, def, local_fn_sigs, upgrades);
            for stmt in flow.body.iter() {
                collect_param_kind_upgrades_in_stmt(stmt, def, local_fn_sigs, upgrades);
            }
        }
        Stmt::While(boxed, ..) => {
            let flow = boxed.as_ref();
            collect_param_kind_upgrades_in_expr(&flow.expr, def, local_fn_sigs, upgrades);
            for stmt in flow.body.iter() {
                collect_param_kind_upgrades_in_stmt(stmt, def, local_fn_sigs, upgrades);
            }
        }
        Stmt::FnCall(call, ..) => {
            collect_param_kind_upgrades_in_call(call, def, local_fn_sigs, upgrades);
        }
        _ => {}
    }
}

fn collect_param_kind_upgrades_in_expr(
    expr: &Expr,
    def: &ScriptFuncDef,
    local_fn_sigs: &BTreeMap<String, Vec<ValueKind>>,
    upgrades: &mut Vec<(usize, ValueKind)>,
) {
    match expr {
        Expr::FnCall(call, ..) | Expr::MethodCall(call, ..) => {
            collect_param_kind_upgrades_in_call(call, def, local_fn_sigs, upgrades);
            for arg in &call.args {
                collect_param_kind_upgrades_in_expr(arg, def, local_fn_sigs, upgrades);
            }
        }
        Expr::Dot(boxed, ..) | Expr::Index(boxed, ..) => {
            collect_param_kind_upgrades_in_expr(&boxed.lhs, def, local_fn_sigs, upgrades);
            collect_param_kind_upgrades_in_expr(&boxed.rhs, def, local_fn_sigs, upgrades);
        }
        Expr::Array(items, ..) => {
            for item in items {
                collect_param_kind_upgrades_in_expr(item, def, local_fn_sigs, upgrades);
            }
        }
        Expr::Map(map, ..) => {
            for (_, value) in &map.0 {
                collect_param_kind_upgrades_in_expr(value, def, local_fn_sigs, upgrades);
            }
        }
        Expr::And(args, ..) | Expr::Or(args, ..) => {
            for arg in args.iter() {
                collect_param_kind_upgrades_in_expr(arg, def, local_fn_sigs, upgrades);
            }
        }
        _ => {}
    }
}

fn collect_param_kind_upgrades_in_call(
    call: &rhai::FnCallExpr,
    def: &ScriptFuncDef,
    local_fn_sigs: &BTreeMap<String, Vec<ValueKind>>,
    upgrades: &mut Vec<(usize, ValueKind)>,
) {
    if !call.namespace.is_empty() {
        return;
    }
    let Some(callee_sig) = local_fn_sigs.get(call.name.as_str()) else {
        return;
    };
    for (arg_index, arg) in call.args.iter().enumerate() {
        let Expr::Variable(ident, ..) = arg else {
            continue;
        };
        let Some(param_index) = def
            .params
            .iter()
            .position(|param| param.as_str() == ident.1.as_str())
        else {
            continue;
        };
        let Some(kind) = callee_sig.get(arg_index).copied() else {
            continue;
        };
        if matches!(kind, ValueKind::String | ValueKind::Json) {
            upgrades.push((param_index, kind));
        }
    }
}

fn param_used_as_json(def: &ScriptFuncDef, param: &str, _ctx: &EmitCtx) -> bool {
    def.body
        .iter()
        .any(|stmt| stmt_uses_json_param(stmt, param))
}

fn stmt_uses_json_param(stmt: &Stmt, param: &str) -> bool {
    match stmt {
        Stmt::Expr(expr) | Stmt::Return(Some(expr), ..) => {
            expr_uses_json_param(expr.as_ref(), param)
        }
        Stmt::Var(boxed, ..) => expr_uses_json_param(&boxed.1, param),
        Stmt::Assignment(boxed, ..) => {
            expr_uses_json_param(&boxed.1.lhs, param) || expr_uses_json_param(&boxed.1.rhs, param)
        }
        Stmt::If(boxed, ..) => {
            let flow = boxed.as_ref();
            expr_uses_json_param(&flow.expr, param)
                || flow
                    .body
                    .iter()
                    .any(|stmt| stmt_uses_json_param(stmt, param))
                || flow
                    .branch
                    .iter()
                    .any(|stmt| stmt_uses_json_param(stmt, param))
        }
        Stmt::For(boxed, ..) => {
            let (_, _, flow) = boxed.as_ref();
            for_iterable_uses_json_param(&flow.expr, param)
                || flow
                    .body
                    .iter()
                    .any(|stmt| stmt_uses_json_param(stmt, param))
        }
        Stmt::While(boxed, ..) => {
            let flow = boxed.as_ref();
            expr_uses_json_param(&flow.expr, param)
                || flow
                    .body
                    .iter()
                    .any(|stmt| stmt_uses_json_param(stmt, param))
        }
        _ => false,
    }
}

fn for_iterable_uses_json_param(expr: &Expr, param: &str) -> bool {
    match expr {
        // Bare `for x in param` is also valid for strings/StringLists — do not
        // treat it as JSON evidence. Prefer `param.field` / `param[index]`.
        Expr::Dot(boxed, ..) => {
            matches!(&boxed.lhs, Expr::Variable(ident, ..) if ident.1.as_str() == param)
        }
        Expr::Index(boxed, ..) => {
            matches!(&boxed.lhs, Expr::Variable(ident, ..) if ident.1.as_str() == param)
        }
        _ => false,
    }
}

fn is_stringish_method_name(name: &str) -> bool {
    matches!(
        name,
        "contains"
            | "starts_with"
            | "ends_with"
            | "trim"
            | "replace"
            | "to_lower"
            | "to_string"
            | "split"
            | "len"
    )
}

fn is_json_method_name(name: &str) -> bool {
    matches!(name, "push" | "insert" | "get")
}

fn expr_uses_json_param(expr: &Expr, param: &str) -> bool {
    match expr {
        Expr::Index(boxed, ..) => {
            matches!(&boxed.lhs, Expr::Variable(ident, ..) if ident.1.as_str() == param)
                || expr_uses_json_param(&boxed.lhs, param)
                || expr_uses_json_param(&boxed.rhs, param)
        }
        Expr::FnCall(call, ..) => {
            // Known JSON-consuming callees. Merely passing `param` to an
            // arbitrary call is not JSON evidence (strings are passed too).
            if ((call.namespace.to_string() == "rhai::json" && call.name == "stringify_pretty")
                || (call.namespace.is_empty() && call.name == "stringify_pretty"))
                && call.args.len() == 1
                && is_param_var(&call.args[0], param)
            {
                return true;
            }
            call.args.iter().any(|arg| expr_uses_json_param(arg, param))
        }
        Expr::MethodCall(call, ..) => call.args.iter().any(|arg| expr_uses_json_param(arg, param)),
        Expr::Array(items, ..) => items.iter().any(|item| expr_uses_json_param(item, param)),
        Expr::Map(map, ..) => map
            .0
            .iter()
            .any(|(_, value)| expr_uses_json_param(value, param)),
        Expr::Dot(boxed, ..) => {
            if is_param_var(&boxed.lhs, param) {
                match &boxed.rhs {
                    // `param.len` is shared by strings/arrays — not JSON-only.
                    Expr::Property(property, ..) if property.2.as_str() == "len" => false,
                    Expr::Property(..) => true,
                    Expr::MethodCall(call, ..) if is_json_method_name(call.name.as_str()) => true,
                    Expr::MethodCall(call, ..) if is_stringish_method_name(call.name.as_str()) => {
                        call.args.iter().any(|arg| expr_uses_json_param(arg, param))
                    }
                    _ => expr_uses_json_param(&boxed.rhs, param),
                }
            } else {
                expr_uses_json_param(&boxed.lhs, param) || expr_uses_json_param(&boxed.rhs, param)
            }
        }
        _ => false,
    }
}

fn param_used_as_string(def: &ScriptFuncDef, param: &str, _ctx: &EmitCtx) -> bool {
    def.body
        .iter()
        .any(|stmt| stmt_uses_string_param(stmt, param))
}

fn stmt_uses_string_param(stmt: &Stmt, param: &str) -> bool {
    match stmt {
        Stmt::Expr(expr) | Stmt::Return(Some(expr), ..) => {
            expr_uses_string_param(expr.as_ref(), param)
        }
        Stmt::Var(boxed, ..) => expr_uses_string_param(&boxed.1, param),
        Stmt::Assignment(boxed, ..) => {
            expr_uses_string_param(&boxed.1.lhs, param)
                || expr_uses_string_param(&boxed.1.rhs, param)
        }
        Stmt::If(boxed, ..) => {
            let flow = boxed.as_ref();
            expr_uses_string_param(&flow.expr, param)
                || flow
                    .body
                    .iter()
                    .any(|stmt| stmt_uses_string_param(stmt, param))
                || flow
                    .branch
                    .iter()
                    .any(|stmt| stmt_uses_string_param(stmt, param))
        }
        Stmt::For(boxed, ..) => {
            let (_, _, flow) = boxed.as_ref();
            // Bare `for ch in param` is the string character-iteration form.
            matches!(&flow.expr, Expr::Variable(ident, ..) if ident.1.as_str() == param)
                || expr_uses_string_param(&flow.expr, param)
                || flow
                    .body
                    .iter()
                    .any(|stmt| stmt_uses_string_param(stmt, param))
        }
        Stmt::While(boxed, ..) => {
            let flow = boxed.as_ref();
            expr_uses_string_param(&flow.expr, param)
                || flow
                    .body
                    .iter()
                    .any(|stmt| stmt_uses_string_param(stmt, param))
        }
        Stmt::FnCall(call, ..) => call
            .args
            .iter()
            .any(|arg| expr_uses_string_param(arg, param)),
        _ => false,
    }
}

fn expr_uses_string_param(expr: &Expr, param: &str) -> bool {
    if path_absolute_display_arg(expr).is_some_and(|path| is_param_var(path, param))
        || std_fs_exists_arg(expr).is_some_and(|path| is_param_var(path, param))
        || std_fs_read_to_string_arg(expr).is_some_and(|path| is_param_var(path, param))
        || std_fs_read_dir_arg(expr).is_some_and(|path| is_param_var(path, param))
        || std_fs_remove_file_arg(expr).is_some_and(|path| is_param_var(path, param))
        || std_fs_try_remove_file_arg(expr).is_some_and(|path| is_param_var(path, param))
        || std_fs_copy_args(expr)
            .is_some_and(|(src, dst)| is_param_var(src, param) || is_param_var(dst, param))
        || std_fs_try_copy_args(expr)
            .is_some_and(|(src, dst)| is_param_var(src, param) || is_param_var(dst, param))
        || std_fs_create_dir_all_arg(expr).is_some_and(|path| is_param_var(path, param))
        || std_fs_try_create_dir_all_arg(expr).is_some_and(|path| is_param_var(path, param))
        || std_fs_rename_args(expr)
            .is_some_and(|(src, dst)| is_param_var(src, param) || is_param_var(dst, param))
        || std_fs_try_rename_args(expr)
            .is_some_and(|(src, dst)| is_param_var(src, param) || is_param_var(dst, param))
        || std_fs_symlink_metadata_arg(expr).is_some_and(|path| is_param_var(path, param))
        || path_join_display_args(expr)
            .is_some_and(|(base, child)| is_param_var(base, param) || is_param_var(child, param))
        || rh_fail_arg(expr).is_some_and(|message| is_param_var(message, param))
    {
        return true;
    }
    // `param.len` is the common string-length property form (also used by hex checks).
    if let Expr::Dot(boxed, ..) = expr
        && is_param_var(&boxed.lhs, param)
        && matches!(
            &boxed.rhs,
            Expr::Property(property, ..) if property.2.as_str() == "len"
        )
    {
        return true;
    }
    if let Expr::Dot(boxed, ..) = expr
        && is_param_var(&boxed.lhs, param)
        && matches!(
            &boxed.rhs,
            Expr::MethodCall(call, ..)
                if matches!(
                    call.name.as_str(),
                    "contains" | "starts_with" | "ends_with" | "trim" | "replace" | "to_lower"
                )
        )
    {
        return true;
    }
    // Dynamic needles for string methods are themselves strings.
    if let Expr::Dot(boxed, ..) = expr
        && let Expr::MethodCall(call, ..) = &boxed.rhs
        && matches!(call.name.as_str(), "contains" | "starts_with" | "ends_with")
        && call.args.len() == 1
        && is_param_var(&call.args[0], param)
    {
        return true;
    }
    if let Expr::Dot(boxed, ..) = expr
        && let Expr::Dot(inner, ..) = &boxed.rhs
        && let Expr::MethodCall(call, ..) = &inner.rhs
        && matches!(call.name.as_str(), "contains" | "starts_with" | "ends_with")
        && call.args.len() == 1
        && is_param_var(&call.args[0], param)
    {
        return true;
    }
    // Only treat `+` as string evidence when the other operand is a string literal.
    if let Expr::FnCall(call, ..) = expr
        && matches!(call.op_token.as_ref(), Some(Token::Plus))
        && call.args.len() == 2
        && ((is_param_var(&call.args[0], param)
            && matches!(call.args[1], Expr::StringConstant(..)))
            || (is_param_var(&call.args[1], param)
                && matches!(call.args[0], Expr::StringConstant(..))))
    {
        return true;
    }
    if let Expr::FnCall(call, ..) = expr
        && matches!(
            call.op_token.as_ref(),
            Some(Token::Equals | Token::EqualsTo | Token::NotEqualsTo)
        )
        && call.args.len() == 2
        && ((is_param_var(&call.args[0], param)
            && matches!(call.args[1], Expr::StringConstant(..)))
            || (is_param_var(&call.args[1], param)
                && matches!(call.args[0], Expr::StringConstant(..))))
    {
        return true;
    }
    match expr {
        Expr::FnCall(call, ..) | Expr::MethodCall(call, ..) => call
            .args
            .iter()
            .any(|arg| expr_uses_string_param(arg, param)),
        Expr::Dot(boxed, ..) | Expr::Index(boxed, ..) => {
            expr_uses_string_param(&boxed.lhs, param) || expr_uses_string_param(&boxed.rhs, param)
        }
        Expr::And(args, ..) | Expr::Or(args, ..) => {
            args.iter().any(|arg| expr_uses_string_param(arg, param))
        }
        _ => false,
    }
}

fn is_param_var(expr: &Expr, param: &str) -> bool {
    matches!(expr, Expr::Variable(ident, ..) if ident.1.as_str() == param)
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
            } else if kind == ValueKind::Json
                && matches!(expr, Expr::Array(items, ..) if items.is_empty())
            {
                out.push_str("serde_json::Value::Array(Vec::new())");
            } else if kind == ValueKind::Json && matches!(expr, Expr::Map(..)) {
                emit_json_map_literal(out, expr, ctx)?;
            } else if kind == ValueKind::StringList {
                if let Some((source, separator)) = string_split_args(expr, ctx) {
                    out.push_str("rh_string_split(&");
                    emit_stringish(out, source, ctx)?;
                    out.push_str(", ");
                    emit_native_string(out, separator, ctx)?;
                    out.push(')');
                } else {
                    return Err(RhError::Transpile(
                        "string list binding requires .split(\"…\")".into(),
                    ));
                }
            } else if kind == ValueKind::Set {
                out.push_str("std::collections::HashSet::<String>::new()");
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
            if let Some((set_name, key)) = set_insert_assignment(boxed, ctx) {
                out.push_str("    ");
                out.push_str(set_name);
                out.push_str(".insert(");
                emit_set_key(out, key, ctx)?;
                out.push_str(");\n");
                return Ok(());
            }
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
            // Only treat branch tails as returns when this `if` itself is the
            // function/block result. Nested statement `if` bodies must not
            // early-return (e.g. trailing `require` inside a `for` loop).
            emit_block(out, &flow.body, ctx, implicit_return)?;
            out.push_str("    }");
            if !flow.branch.is_empty() {
                out.push_str(" else {\n");
                emit_block(out, &flow.branch, ctx, implicit_return)?;
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
            } else if let Some((source, separator)) = string_split_args(&flow.expr, ctx) {
                out.push_str("    for ");
                out.push_str(counter.name.as_str());
                out.push_str(" in rh_string_split(&");
                emit_stringish(out, source, ctx)?;
                out.push_str(", ");
                emit_native_string(out, separator, ctx)?;
                out.push_str(") {\n");
                let mut loop_ctx = ctx
                    .clone()
                    .with_binding(counter.name.as_str(), ValueKind::String);
                emit_block(out, &flow.body, &mut loop_ctx, false)?;
                out.push_str("    }\n");
            } else if let Expr::Variable(ident, ..) = &flow.expr
                && ctx.scope.get(ident.1.as_str()).copied() == Some(ValueKind::StringList)
            {
                out.push_str("    for ");
                out.push_str(counter.name.as_str());
                out.push_str(" in ");
                out.push_str(ident.1.as_str());
                out.push_str(".iter().cloned() {\n");
                let mut loop_ctx = ctx
                    .clone()
                    .with_binding(counter.name.as_str(), ValueKind::String);
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
            } else if let Some(path) = std_fs_read_dir_arg(&flow.expr) {
                let mut path_expr = String::new();
                if emit_native_string(&mut path_expr, path, ctx)? {
                    out.push_str("    for ");
                    out.push_str(counter.name.as_str());
                    out.push_str(" in rh_read_dir(");
                    out.push_str(&path_expr);
                    out.push_str(") {\n");
                    let mut loop_ctx = ctx
                        .clone()
                        .with_binding(counter.name.as_str(), ValueKind::DirEntry);
                    emit_block(out, &flow.body, &mut loop_ctx, false)?;
                    out.push_str("    }\n");
                } else {
                    let snippet = crate::expr_print::stmt_to_rhai(stmt)?;
                    out.push_str("    let _for = rh_host_eval_int(");
                    out.push_str(&format!("{:?}, ", snippet));
                    ctx.emit_scope_json_expr(out);
                    out.push_str(";\n");
                }
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
        Stmt::Expr(expr) if ctx.cdylib && emit_json_array_push_stmt(out, expr, ctx)? => {}
        Stmt::Expr(expr)
            if ctx.cdylib
                && let Expr::FnCall(call, ..) = expr.as_ref()
                && call.namespace.is_empty()
                && call.name == "require"
                && call.args.len() == 2 =>
        {
            emit_require_stmt(out, &call.args[0], &call.args[1], ctx)?;
            if implicit_return {
                out.push_str("    return 0;\n");
            }
        }
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
        Expr::Map(map, ..) if map.0.is_empty() => ValueKind::Set,
        Expr::Map(..) => ValueKind::Json,
        Expr::Array(items, ..) if items.is_empty() => ValueKind::Json,
        Expr::Variable(ident, ..) => match ctx.scope.get(ident.1.as_str()).copied() {
            Some(kind) => kind,
            None => ValueKind::Int,
        },
        _ if json_parse_arg(expr).is_some() => ValueKind::Json,
        _ if json_value_path(expr, ctx).is_some_and(|(_, path)| !path.is_empty()) => {
            ValueKind::Json
        }
        _ if json_array_index(expr, ctx).is_some() => ValueKind::Json,
        _ if string_list_index(expr, ctx).is_some() => ValueKind::String,
        _ if string_split_args(expr, ctx).is_some() => ValueKind::StringList,
        _ if json_stringify_pretty_arg(expr).is_some() => ValueKind::String,
        _ if string_concat_args(expr, ctx).is_some() => ValueKind::String,
        _ if args_index_expr(expr).is_some() => ValueKind::String,
        _ if std_fs_read_to_string_arg(expr).is_some() => ValueKind::String,
        _ if path_join_display_args(expr).is_some() => ValueKind::String,
        _ if path_absolute_display_arg(expr).is_some() => ValueKind::String,
        _ if std_fs_symlink_metadata_arg(expr).is_some() => ValueKind::Metadata,
        _ if std_time_system_time_now_unix_millis(expr) => ValueKind::Int,
        _ if std_time_system_time_now_rfc3339(expr) => ValueKind::String,
        _ if std_env_get_arg(expr).is_some() => ValueKind::String,
        _ if crypto_sha256_file_arg(expr).is_some() => ValueKind::String,
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

fn std_fs_two_arg<'a>(expr: &'a Expr, name: &str) -> Option<(&'a Expr, &'a Expr)> {
    let Expr::FnCall(call, ..) = expr else {
        return None;
    };
    if call.namespace.to_string() != "std::fs" || call.name != name || call.args.len() != 2 {
        return None;
    }
    Some((&call.args[0], &call.args[1]))
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

fn path_absolute_display_arg(expr: &Expr) -> Option<&Expr> {
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
    if call.namespace.to_string() != "std::path" || call.name != "absolute" || call.args.len() != 1
    {
        return None;
    }
    Some(&call.args[0])
}

fn std_fs_symlink_metadata_arg(expr: &Expr) -> Option<&Expr> {
    std_fs_single_arg(expr, "symlink_metadata")
}

fn std_fs_remove_file_arg(expr: &Expr) -> Option<&Expr> {
    std_fs_single_arg(expr, "remove_file")
}

fn std_fs_try_remove_file_arg(expr: &Expr) -> Option<&Expr> {
    std_fs_single_arg(expr, "try_remove_file")
}

fn std_fs_copy_args(expr: &Expr) -> Option<(&Expr, &Expr)> {
    std_fs_two_arg(expr, "copy")
}

fn std_fs_try_copy_args(expr: &Expr) -> Option<(&Expr, &Expr)> {
    std_fs_two_arg(expr, "try_copy")
}

fn std_fs_create_dir_all_arg(expr: &Expr) -> Option<&Expr> {
    std_fs_single_arg(expr, "create_dir_all")
}

fn std_fs_try_create_dir_all_arg(expr: &Expr) -> Option<&Expr> {
    std_fs_single_arg(expr, "try_create_dir_all")
}

fn std_fs_rename_args(expr: &Expr) -> Option<(&Expr, &Expr)> {
    std_fs_two_arg(expr, "rename")
}

fn std_fs_try_rename_args(expr: &Expr) -> Option<(&Expr, &Expr)> {
    std_fs_two_arg(expr, "try_rename")
}

fn std_fs_read_dir_arg(expr: &Expr) -> Option<&Expr> {
    std_fs_single_arg(expr, "read_dir")
}

fn std_time_system_time_now_rfc3339(expr: &Expr) -> bool {
    let Expr::Dot(boxed, ..) = expr else {
        return false;
    };
    let rfc3339 = matches!(
        &boxed.rhs,
        Expr::Property(property, ..) if property.2.as_str() == "rfc3339"
    ) || matches!(
        &boxed.rhs,
        Expr::MethodCall(call, ..) if call.name == "rfc3339" && call.args.is_empty()
    );
    if !rfc3339 {
        return false;
    }
    let Expr::FnCall(call, ..) = &boxed.lhs else {
        return false;
    };
    call.namespace.to_string() == "std::time::SystemTime"
        && call.name == "now"
        && call.args.is_empty()
}

fn std_env_has_arg(expr: &Expr) -> Option<&Expr> {
    let Expr::FnCall(call, ..) = expr else {
        return None;
    };
    if call.namespace.to_string() != "std::env" || call.name != "has" || call.args.len() != 1 {
        return None;
    }
    Some(&call.args[0])
}

fn std_env_get_arg(expr: &Expr) -> Option<&Expr> {
    let Expr::FnCall(call, ..) = expr else {
        return None;
    };
    if call.namespace.to_string() != "std::env" || call.name != "get" || call.args.len() != 1 {
        return None;
    }
    Some(&call.args[0])
}

fn crypto_sha256_file_arg(expr: &Expr) -> Option<&Expr> {
    let Expr::FnCall(call, ..) = expr else {
        return None;
    };
    if call.namespace.to_string() != "rhai::crypto"
        || call.name != "sha256_file"
        || call.args.len() != 1
    {
        return None;
    }
    Some(&call.args[0])
}

fn runtime_atomic_write_args(expr: &Expr) -> Option<(&Expr, &Expr)> {
    let Expr::FnCall(call, ..) = expr else {
        return None;
    };
    if call.namespace.to_string() != "rhai::runtime"
        || call.name != "atomic_write"
        || call.args.len() != 2
    {
        return None;
    }
    Some((&call.args[0], &call.args[1]))
}

fn json_stringify_pretty_arg(expr: &Expr) -> Option<&Expr> {
    let Expr::FnCall(call, ..) = expr else {
        return None;
    };
    if call.namespace.to_string() != "rhai::json"
        || call.name != "stringify_pretty"
        || call.args.len() != 1
    {
        return None;
    }
    Some(&call.args[0])
}

fn string_split_args<'a>(expr: &'a Expr, ctx: &EmitCtx) -> Option<(&'a Expr, &'a Expr)> {
    let Expr::Dot(boxed, ..) = expr else {
        return None;
    };
    let Expr::MethodCall(call, ..) = &boxed.rhs else {
        return None;
    };
    if call.name != "split" || call.args.len() != 1 {
        return None;
    }
    if !matches!(&call.args[0], Expr::StringConstant(..)) {
        return None;
    }
    let receiver_ok = match &boxed.lhs {
        Expr::Variable(ident, ..) => {
            matches!(
                ctx.scope.get(ident.1.as_str()).copied(),
                Some(ValueKind::String)
            )
        }
        Expr::StringConstant(..) => true,
        _ => {
            string_concat_args(&boxed.lhs, ctx).is_some()
                || args_index_expr(&boxed.lhs).is_some()
                || std_fs_read_to_string_arg(&boxed.lhs).is_some()
                || std_env_get_arg(&boxed.lhs).is_some()
                || crypto_sha256_file_arg(&boxed.lhs).is_some()
                || json_stringify_pretty_arg(&boxed.lhs).is_some()
        }
    };
    if !receiver_ok {
        return None;
    }
    Some((&boxed.lhs, &call.args[0]))
}

fn string_list_index<'a>(expr: &'a Expr, ctx: &EmitCtx) -> Option<(&'a str, &'a Expr)> {
    let Expr::Index(boxed, ..) = expr else {
        return None;
    };
    let Expr::Variable(ident, ..) = &boxed.lhs else {
        return None;
    };
    if ctx.scope.get(ident.1.as_str()).copied() != Some(ValueKind::StringList) {
        return None;
    }
    Some((ident.1.as_str(), &boxed.rhs))
}

fn json_array_index<'a>(expr: &'a Expr, ctx: &EmitCtx) -> Option<(&'a str, &'a Expr)> {
    let Expr::Index(boxed, ..) = expr else {
        return None;
    };
    let Expr::Variable(ident, ..) = &boxed.lhs else {
        return None;
    };
    if ctx.scope.get(ident.1.as_str()).copied() != Some(ValueKind::Json) {
        return None;
    }
    Some((ident.1.as_str(), &boxed.rhs))
}

fn json_array_push_call<'a>(expr: &'a Expr, ctx: &EmitCtx) -> Option<(&'a str, &'a Expr)> {
    let Expr::Dot(boxed, ..) = expr else {
        return None;
    };
    let Expr::Variable(ident, ..) = &boxed.lhs else {
        return None;
    };
    if ctx.scope.get(ident.1.as_str()).copied() != Some(ValueKind::Json) {
        return None;
    }
    let Expr::MethodCall(call, ..) = &boxed.rhs else {
        return None;
    };
    if call.name != "push" || call.args.len() != 1 {
        return None;
    }
    Some((ident.1.as_str(), &call.args[0]))
}

fn std_time_system_time_now_unix_millis(expr: &Expr) -> bool {
    let Expr::Dot(boxed, ..) = expr else {
        return false;
    };
    let unix_millis = matches!(
        &boxed.rhs,
        Expr::Property(property, ..) if property.2.as_str() == "unix_millis"
    ) || matches!(
        &boxed.rhs,
        Expr::MethodCall(call, ..) if call.name == "unix_millis" && call.args.is_empty()
    );
    if !unix_millis {
        return false;
    }
    let Expr::FnCall(call, ..) = &boxed.lhs else {
        return false;
    };
    call.namespace.to_string() == "std::time::SystemTime"
        && call.name == "now"
        && call.args.is_empty()
}

fn symlink_metadata_property<'a>(expr: &'a Expr) -> Option<(&'a Expr, &'a str)> {
    let Expr::Dot(boxed, ..) = expr else {
        return None;
    };
    let path = std_fs_symlink_metadata_arg(&boxed.lhs)?;
    let name = match &boxed.rhs {
        Expr::Property(property, ..) => property.2.as_str(),
        Expr::MethodCall(call, ..) if call.args.is_empty() => call.name.as_str(),
        _ => return None,
    };
    matches!(
        name,
        "is_file" | "is_dir" | "is_symlink" | "is_reparse_point" | "len"
    )
    .then_some((path, name))
}

fn dir_entry_variable<'a>(expr: &'a Expr, ctx: &EmitCtx) -> Option<&'a str> {
    let Expr::Dot(boxed, ..) = expr else {
        return None;
    };
    let Expr::Variable(ident, ..) = &boxed.lhs else {
        return None;
    };
    (ctx.scope.get(ident.1.as_str()).copied() == Some(ValueKind::DirEntry))
        .then_some(ident.1.as_str())
}

fn dir_entry_field_name<'a>(rhs: &'a Expr) -> Option<&'a str> {
    match rhs {
        Expr::Property(property, ..) => Some(property.2.as_str()),
        Expr::Dot(boxed, ..) => match &boxed.lhs {
            Expr::Property(property, ..) => Some(property.2.as_str()),
            _ => None,
        },
        Expr::MethodCall(call, ..) if call.args.is_empty() => Some(call.name.as_str()),
        _ => None,
    }
}

fn dir_entry_binding<'a>(expr: &'a Expr, ctx: &EmitCtx) -> Option<(&'a str, &'a str)> {
    let binding = dir_entry_variable(expr, ctx)?;
    let Expr::Dot(boxed, ..) = expr else {
        return None;
    };
    let field = dir_entry_field_name(&boxed.rhs)?;
    Some((binding, field))
}

fn dir_entry_int_field<'a>(expr: &'a Expr, ctx: &EmitCtx) -> Option<(&'a str, &'a str)> {
    let (binding, field) = dir_entry_binding(expr, ctx)?;
    matches!(field, "is_file" | "is_dir" | "is_symlink").then_some((binding, field))
}

fn dir_entry_string_field<'a>(expr: &'a Expr, ctx: &EmitCtx) -> Option<(&'a str, &'a str)> {
    let (binding, field) = dir_entry_binding(expr, ctx)?;
    matches!(field, "file_name" | "path").then_some((binding, field))
}

fn dir_entry_path_display_binding<'a>(expr: &'a Expr, ctx: &'a EmitCtx) -> Option<&'a str> {
    let binding = dir_entry_variable(expr, ctx)?;
    let Expr::Dot(boxed, ..) = expr else {
        return None;
    };
    let Expr::Dot(inner, ..) = &boxed.rhs else {
        return None;
    };
    let Expr::Property(path, ..) = &inner.lhs else {
        return None;
    };
    if path.2.as_str() != "path" {
        return None;
    }
    let display = matches!(
        &inner.rhs,
        Expr::Property(property, ..) if property.2.as_str() == "display"
    ) || matches!(
        &inner.rhs,
        Expr::MethodCall(call, ..) if call.name == "display" && call.args.is_empty()
    );
    display.then_some(binding)
}

fn metadata_property_binding<'a>(expr: &'a Expr, ctx: &EmitCtx) -> Option<(&'a str, &'a str)> {
    let Expr::Dot(boxed, ..) = expr else {
        return None;
    };
    let Expr::Variable(ident, ..) = &boxed.lhs else {
        return None;
    };
    if ctx.scope.get(ident.1.as_str()).copied() != Some(ValueKind::Metadata) {
        return None;
    }
    let name = match &boxed.rhs {
        Expr::Property(property, ..) => property.2.as_str(),
        Expr::MethodCall(call, ..) if call.args.is_empty() => call.name.as_str(),
        _ => return None,
    };
    matches!(
        name,
        "is_file" | "is_dir" | "is_symlink" | "is_reparse_point" | "len"
    )
    .then_some((ident.1.as_str(), name))
}

fn process_status_args(expr: &Expr) -> Option<(&Expr, &[Expr], &Expr, Option<&Expr>)> {
    let Expr::FnCall(call, ..) = expr else {
        return None;
    };
    if call.namespace.to_string() != "std::process" || call.name != "command_status" {
        return None;
    }
    let options = match call.args.len() {
        3 => None,
        4 => Some(&call.args[3]),
        _ => return None,
    };
    let Expr::Array(arguments, ..) = &call.args[1] else {
        return None;
    };
    Some((&call.args[0], arguments, &call.args[2], options))
}

fn process_stdout_file_args(
    expr: &Expr,
) -> Option<(&Expr, &[Expr], &Expr, &Expr, Option<&Expr>)> {
    let Expr::FnCall(call, ..) = expr else {
        return None;
    };
    if call.namespace.to_string() != "std::process" || call.name != "command_stdout_file" {
        return None;
    }
    let options = match call.args.len() {
        4 => None,
        5 => Some(&call.args[4]),
        _ => return None,
    };
    let Expr::Array(arguments, ..) = &call.args[1] else {
        return None;
    };
    Some((
        &call.args[0],
        arguments,
        &call.args[2],
        &call.args[3],
        options,
    ))
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
    if matches!(expr, Expr::StringConstant(..))
        || type_of_arg(expr).is_some()
        || args_index_expr(expr).is_some()
        || matches!(
            expr,
            Expr::Variable(ident, ..)
                if ctx.scope.get(ident.1.as_str()).copied() == Some(ValueKind::String)
        )
        || std_env_get_arg(expr).is_some()
        || path_join_display_args(expr).is_some()
        || path_absolute_display_arg(expr).is_some()
        || std_fs_read_to_string_arg(expr).is_some()
        || crypto_sha256_file_arg(expr).is_some()
        || json_stringify_pretty_arg(expr).is_some()
        || string_list_index(expr, ctx).is_some()
        || std_time_system_time_now_rfc3339(expr)
    {
        return true;
    }
    // Nested `+` chains: any string leaf makes the whole chain string-typed so
    // `("a" + "b") + n` uses format! rather than Rust `String + i64`.
    if let Expr::FnCall(call, ..) = expr
        && matches!(call.op_token.as_ref(), Some(Token::Plus))
        && call.args.len() == 2
    {
        return is_explicit_string_expr(&call.args[0], ctx)
            || is_explicit_string_expr(&call.args[1], ctx);
    }
    false
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
    if let Some((binding, path)) = json_array_len_path(expr, ctx) {
        out.push_str("rh_json_array_len(&");
        out.push_str(binding);
        out.push_str(", ");
        emit_json_path(out, &path);
        out.push_str(").to_string()");
        return Ok(());
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
        Expr::IntegerConstant(value, ..) => {
            out.push_str("String::from(");
            out.push_str(&format!("\"{value}\""));
            out.push(')');
        }
        _ if let Some((binding, index)) = string_list_index(expr, ctx) => {
            out.push_str("rh_string_list_get(&");
            out.push_str(binding);
            out.push_str(", ");
            emit_expr(out, index, ctx)?;
            out.push(')');
        }
        _ if let Some(value) = json_stringify_pretty_arg(expr) => {
            if !emit_json_stringify_pretty(out, value, ctx)? {
                return Err(RhError::Transpile(
                    "stringify_pretty argument must be a JSON value".into(),
                ));
            }
        }
        _ if let Some(path) = crypto_sha256_file_arg(expr) => {
            if !emit_crypto_sha256_file(out, path, ctx)? {
                return Err(RhError::Transpile(
                    "sha256_file argument must be a string path".into(),
                ));
            }
        }
        _ if let Some(name) = std_env_get_arg(expr) => {
            if !emit_std_env_get(out, name, ctx)? {
                return Err(RhError::Transpile(
                    "env::get argument must be a string name".into(),
                ));
            }
        }
        _ if std_time_system_time_now_rfc3339(expr) => {
            out.push_str("rh_system_time_now_rfc3339()");
        }
        _ if is_pure_int_expr(expr) || is_native_json_int_expr(expr, ctx) => {
            out.push('(');
            emit_expr(out, expr, ctx)?;
            out.push_str(").to_string()");
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
        if let Expr::FnCall(call, ..) = expr
            && call.namespace.is_empty()
            && call.name == "require"
            && call.args.len() == 2
        {
            out.push_str("{\n");
            emit_require_stmt(out, &call.args[0], &call.args[1], ctx)?;
            out.push_str("    0\n}");
            return Ok(());
        }
        if let Expr::FnCall(call, ..) = expr
            && call.namespace.is_empty()
            && call.name == "print"
            && call.args.len() == 1
        {
            out.push_str("rh_print(&");
            emit_stringish(out, &call.args[0], ctx)?;
            out.push(')');
            return Ok(());
        }
        if let Some((program, arguments, timeout, options)) = process_status_args(expr)
            && emit_process_status(out, program, arguments, timeout, options, ctx)?
        {
            return Ok(());
        }
        if let Some((program, arguments, timeout, stdout_path, options)) =
            process_stdout_file_args(expr)
            && emit_process_stdout_file(out, program, arguments, timeout, stdout_path, options, ctx)?
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
        if let Some(path) = std_fs_remove_file_arg(expr)
            && emit_std_fs_remove_file(out, path, ctx)?
        {
            return Ok(());
        }
        if let Some(path) = std_fs_try_remove_file_arg(expr)
            && emit_std_fs_try_remove_file(out, path, ctx)?
        {
            return Ok(());
        }
        if let Some((src, dst)) = std_fs_copy_args(expr)
            && emit_std_fs_copy(out, src, dst, ctx)?
        {
            return Ok(());
        }
        if let Some((src, dst)) = std_fs_try_copy_args(expr)
            && emit_std_fs_try_copy(out, src, dst, ctx)?
        {
            return Ok(());
        }
        if let Some(path) = std_fs_create_dir_all_arg(expr)
            && emit_std_fs_create_dir_all(out, path, ctx)?
        {
            return Ok(());
        }
        if let Some(path) = std_fs_try_create_dir_all_arg(expr)
            && emit_std_fs_try_create_dir_all(out, path, ctx)?
        {
            return Ok(());
        }
        if let Some((src, dst)) = std_fs_rename_args(expr)
            && emit_std_fs_rename(out, src, dst, ctx)?
        {
            return Ok(());
        }
        if let Some((src, dst)) = std_fs_try_rename_args(expr)
            && emit_std_fs_try_rename(out, src, dst, ctx)?
        {
            return Ok(());
        }
        if emit_set_predicate(out, expr, ctx)? {
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
        if let Some(path) = path_absolute_display_arg(expr)
            && emit_path_absolute(out, path, ctx)?
        {
            return Ok(());
        }
        if let Some(path) = std_fs_symlink_metadata_arg(expr)
            && emit_std_fs_symlink_metadata(out, path, ctx)?
        {
            return Ok(());
        }
        if emit_metadata_property(out, expr, ctx)? {
            return Ok(());
        }
        if std_time_system_time_now_unix_millis(expr) {
            out.push_str("rh_system_time_now_unix_millis()");
            return Ok(());
        }
        if std_time_system_time_now_rfc3339(expr) {
            out.push_str("rh_system_time_now_rfc3339()");
            return Ok(());
        }
        if let Some(name) = std_env_has_arg(expr)
            && emit_std_env_has(out, name, ctx)?
        {
            return Ok(());
        }
        if let Some(name) = std_env_get_arg(expr)
            && emit_std_env_get(out, name, ctx)?
        {
            return Ok(());
        }
        if let Some(path) = crypto_sha256_file_arg(expr)
            && emit_crypto_sha256_file(out, path, ctx)?
        {
            return Ok(());
        }
        if let Some((path, value)) = runtime_atomic_write_args(expr)
            && emit_runtime_atomic_write(out, path, value, ctx)?
        {
            return Ok(());
        }
        if let Some(value) = json_stringify_pretty_arg(expr)
            && emit_json_stringify_pretty(out, value, ctx)?
        {
            return Ok(());
        }
        if let Some((binding, index)) = string_list_index(expr, ctx) {
            out.push_str("rh_string_list_get(&");
            out.push_str(binding);
            out.push_str(", ");
            emit_expr(out, index, ctx)?;
            out.push(')');
            return Ok(());
        }
        if let Some((binding, index)) = json_array_index(expr, ctx) {
            out.push_str("rh_json_array_get(&");
            out.push_str(binding);
            out.push_str(", ");
            emit_expr(out, index, ctx)?;
            out.push(')');
            return Ok(());
        }
        if let Some((source, separator)) = string_split_args(expr, ctx) {
            out.push_str("rh_string_split(&");
            emit_stringish(out, source, ctx)?;
            out.push_str(", ");
            emit_native_string(out, separator, ctx)?;
            out.push(')');
            return Ok(());
        }
        if emit_dir_entry_property(out, expr, ctx)? {
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
        if let Expr::FnCall(call, ..) = expr
            && call.op_token.is_none()
            && call.namespace.is_empty()
            && ctx.local_fns.contains(call.name.as_str())
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
        Expr::Dot(..)
            if var_len_name(expr).is_some_and(|name| {
                ctx.scope.get(name).copied() == Some(ValueKind::StringList)
            }) =>
        {
            out.push('(');
            out.push_str(var_len_name(expr).expect("checked string list binding"));
            out.push_str(".len() as INT)");
        }
        Expr::Dot(..)
            if var_len_name(expr)
                .is_some_and(|name| ctx.scope.get(name).copied() == Some(ValueKind::Json)) =>
        {
            out.push_str("rh_json_array_len(&");
            out.push_str(var_len_name(expr).expect("checked json binding"));
            out.push_str(", &[])");
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
    if let Some((base, child)) = path_join_display_args(path) {
        let mut join_expr = String::new();
        if !emit_path_join(&mut join_expr, base, child, ctx)? {
            return Ok(false);
        }
        out.push_str("rh_std_fs_read_to_string(&");
        out.push_str(&join_expr);
        out.push(')');
        return Ok(true);
    }
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

fn emit_json_stringify_pretty(
    out: &mut String,
    value: &Expr,
    ctx: &mut EmitCtx,
) -> Result<bool, RhError> {
    match value {
        Expr::Variable(ident, ..)
            if ctx.scope.get(ident.1.as_str()).copied() == Some(ValueKind::Json) =>
        {
            out.push_str("rh_json_stringify_pretty(&");
            out.push_str(ident.1.as_str());
            out.push(')');
            Ok(true)
        }
        Expr::Map(..) => {
            out.push_str("rh_json_stringify_pretty(&");
            emit_json_map_literal(out, value, ctx)?;
            out.push(')');
            Ok(true)
        }
        _ => Ok(false),
    }
}

fn emit_json_array_push_stmt(
    out: &mut String,
    expr: &Expr,
    ctx: &mut EmitCtx,
) -> Result<bool, RhError> {
    let Some((binding, item)) = json_array_push_call(expr, ctx) else {
        return Ok(false);
    };
    out.push_str("    let _ = rh_json_array_push(&mut ");
    out.push_str(binding);
    out.push_str(", ");
    emit_json_value_expr(out, item, ctx)?;
    out.push_str(");\n");
    Ok(true)
}

fn emit_json_map_literal(out: &mut String, expr: &Expr, ctx: &mut EmitCtx) -> Result<(), RhError> {
    let Expr::Map(map, ..) = expr else {
        return Err(RhError::Transpile(
            "emit_json_map_literal expected map literal".into(),
        ));
    };
    out.push_str("{\n");
    out.push_str("        let mut __rh_map = serde_json::Map::new();\n");
    for (key, value) in &map.0 {
        out.push_str("        __rh_map.insert(");
        out.push_str(&format!("{:?}.to_owned()", key.as_str()));
        out.push_str(", ");
        emit_json_value_expr(out, value, ctx)?;
        out.push_str(");\n");
    }
    out.push_str("        serde_json::Value::Object(__rh_map)\n");
    out.push_str("    }");
    Ok(())
}

fn emit_json_value_expr(out: &mut String, expr: &Expr, ctx: &mut EmitCtx) -> Result<(), RhError> {
    match expr {
        Expr::StringConstant(value, ..) => {
            out.push_str("serde_json::Value::String(String::from(");
            out.push_str(&format!("{value:?}"));
            out.push_str("))");
        }
        Expr::IntegerConstant(value, ..) => {
            out.push_str("serde_json::json!(");
            out.push_str(&value.to_string());
            out.push(')');
        }
        Expr::BoolConstant(value, ..) => {
            out.push_str("serde_json::json!(");
            out.push_str(if *value { "true" } else { "false" });
            out.push(')');
        }
        Expr::Map(..) => emit_json_map_literal(out, expr, ctx)?,
        Expr::Array(items, ..) if items.is_empty() => {
            out.push_str("serde_json::Value::Array(Vec::new())");
        }
        Expr::Array(items, ..)
            if items
                .iter()
                .all(|item| matches!(item, Expr::StringConstant(..))) =>
        {
            out.push_str("serde_json::Value::Array(vec![");
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                let Expr::StringConstant(value, ..) = item else {
                    unreachable!("checked string constants");
                };
                out.push_str("serde_json::Value::String(String::from(");
                out.push_str(&format!("{value:?}"));
                out.push_str("))");
            }
            out.push_str("])");
        }
        Expr::Variable(ident, ..)
            if ctx.scope.get(ident.1.as_str()).copied() == Some(ValueKind::Json) =>
        {
            out.push_str(ident.1.as_str());
            out.push_str(".clone()");
        }
        Expr::Variable(ident, ..)
            if ctx.scope.get(ident.1.as_str()).copied() == Some(ValueKind::String) =>
        {
            out.push_str("serde_json::Value::String(");
            out.push_str(ident.1.as_str());
            out.push_str(".clone())");
        }
        Expr::Variable(ident, ..)
            if ctx.scope.get(ident.1.as_str()).copied() == Some(ValueKind::Bool) =>
        {
            out.push_str("serde_json::Value::Bool(");
            out.push_str(ident.1.as_str());
            out.push_str(" != 0)");
        }
        Expr::Variable(ident, ..)
            if ctx.scope.get(ident.1.as_str()).copied() == Some(ValueKind::Int) =>
        {
            out.push_str("serde_json::json!(");
            out.push_str(ident.1.as_str());
            out.push(')');
        }
        _ if string_concat_args(expr, ctx).is_some()
            || args_index_expr(expr).is_some()
            || std_env_get_arg(expr).is_some()
            || crypto_sha256_file_arg(expr).is_some()
            || json_stringify_pretty_arg(expr).is_some()
            || string_list_index(expr, ctx).is_some()
            || std_time_system_time_now_rfc3339(expr) =>
        {
            out.push_str("serde_json::Value::String(");
            emit_stringish(out, expr, ctx)?;
            out.push(')');
        }
        // Prefer preserving JSON field types (string name/role, nested objects)
        // before the int-coercion fallback — `is_native_json_int_expr` matches any
        // non-empty JSON path and would otherwise force `rh_json_int_path`.
        _ if let Some((binding, path)) = json_value_path(expr, ctx) => {
            if path.is_empty() {
                out.push_str(binding);
                out.push_str(".clone()");
            } else {
                out.push_str("rh_json_get_path(&");
                out.push_str(binding);
                out.push_str(", ");
                emit_json_path(out, &path);
                out.push(')');
            }
        }
        _ if json_array_index(expr, ctx).is_some() => {
            emit_expr(out, expr, ctx)?;
        }
        _ if metadata_property_binding(expr, ctx).is_some_and(|(_, name)| name == "len")
            || is_pure_int_expr(expr)
            || is_native_json_int_expr(expr, ctx) =>
        {
            out.push_str("serde_json::json!(");
            emit_expr(out, expr, ctx)?;
            out.push(')');
        }
        _ => {
            return Err(RhError::Transpile(format!(
                "unsupported json value expression: {expr:?}"
            )));
        }
    }
    Ok(())
}

fn emit_std_env_has(out: &mut String, name: &Expr, ctx: &mut EmitCtx) -> Result<bool, RhError> {
    let mut name_expr = String::new();
    if !emit_native_string(&mut name_expr, name, ctx)? {
        return Ok(false);
    }
    out.push_str("rh_env_has(");
    out.push_str(&name_expr);
    out.push(')');
    Ok(true)
}

fn emit_std_env_get(out: &mut String, name: &Expr, ctx: &mut EmitCtx) -> Result<bool, RhError> {
    let mut name_expr = String::new();
    if !emit_native_string(&mut name_expr, name, ctx)? {
        return Ok(false);
    }
    out.push_str("rh_env_get(");
    out.push_str(&name_expr);
    out.push(')');
    Ok(true)
}

fn emit_crypto_sha256_file(
    out: &mut String,
    path: &Expr,
    ctx: &mut EmitCtx,
) -> Result<bool, RhError> {
    let mut path_expr = String::new();
    if !emit_native_string(&mut path_expr, path, ctx)? {
        return Ok(false);
    }
    out.push_str("rh_sha256_file(");
    out.push_str(&path_expr);
    out.push(')');
    Ok(true)
}

fn emit_runtime_atomic_write(
    out: &mut String,
    path: &Expr,
    value: &Expr,
    ctx: &mut EmitCtx,
) -> Result<bool, RhError> {
    let mut path_expr = String::new();
    if !emit_native_string(&mut path_expr, path, ctx)? {
        return Ok(false);
    }
    out.push_str("rh_atomic_write(");
    out.push_str(&path_expr);
    out.push_str(", &");
    emit_stringish(out, value, ctx)?;
    out.push(')');
    Ok(true)
}

fn emit_process_options(
    out: &mut String,
    options: &Expr,
    ctx: &mut EmitCtx,
) -> Result<bool, RhError> {
    match options {
        Expr::Map(..) => {
            out.push_str("Some(&");
            emit_json_map_literal(out, options, ctx)?;
            out.push(')');
            Ok(true)
        }
        Expr::Variable(ident, ..)
            if ctx.scope.get(ident.1.as_str()).copied() == Some(ValueKind::Json) =>
        {
            out.push_str("Some(&");
            out.push_str(ident.1.as_str());
            out.push(')');
            Ok(true)
        }
        _ => Ok(false),
    }
}

fn emit_process_status(
    out: &mut String,
    program: &Expr,
    arguments: &[Expr],
    timeout: &Expr,
    options: Option<&Expr>,
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
    let mut options_expr = String::new();
    if let Some(options) = options {
        if !emit_process_options(&mut options_expr, options, ctx)? {
            return Ok(false);
        }
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
    out.push_str(", ");
    if options.is_some() {
        out.push_str(&options_expr);
    } else {
        out.push_str("None");
    }
    out.push(')');
    Ok(true)
}

fn emit_process_stdout_file(
    out: &mut String,
    program: &Expr,
    arguments: &[Expr],
    timeout: &Expr,
    stdout_path: &Expr,
    options: Option<&Expr>,
    ctx: &mut EmitCtx,
) -> Result<bool, RhError> {
    let mut program_expr = String::new();
    let mut stdout_path_expr = String::new();
    if !emit_native_string(&mut program_expr, program, ctx)?
        || !is_pure_int_expr(timeout)
        || !emit_native_string(&mut stdout_path_expr, stdout_path, ctx)?
    {
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
    let mut options_expr = String::new();
    if let Some(options) = options {
        if !emit_process_options(&mut options_expr, options, ctx)? {
            return Ok(false);
        }
    }
    out.push_str("rh_process_stdout_file(");
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
    out.push_str(", ");
    out.push_str(&stdout_path_expr);
    out.push_str(", ");
    if options.is_some() {
        out.push_str(&options_expr);
    } else {
        out.push_str("None");
    }
    out.push(')');
    Ok(true)
}

fn emit_json_parse(out: &mut String, source: &Expr, ctx: &mut EmitCtx) -> Result<bool, RhError> {
    if let Some(path) = std_fs_read_to_string_arg(source) {
        let mut path_expr = String::new();
        if !emit_native_string(&mut path_expr, path, ctx)? {
            return Ok(false);
        }
        out.push_str("rh_json_parse(&rh_std_fs_read_to_string(");
        out.push_str(&path_expr);
        out.push_str("))");
        return Ok(true);
    }
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
        _ if let Some((binding, path)) = json_value_path(expr, ctx)
            && !path.is_empty() =>
        {
            // Temporary String is valid for the duration of the call argument.
            out.push_str("&rh_json_string_path(&");
            out.push_str(binding);
            out.push_str(", ");
            emit_json_path(out, &path);
            out.push(')');
        }
        _ if dir_entry_path_display_binding(expr, ctx).is_some()
            || dir_entry_string_field(expr, ctx).is_some_and(|(_, field)| field == "path") =>
        {
            out.push('&');
            let binding = dir_entry_path_display_binding(expr, ctx)
                .or_else(|| dir_entry_string_field(expr, ctx).map(|(name, _)| name))
                .expect("checked dir entry path");
            out.push_str(binding);
            out.push_str(".path");
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
    JsonBinding(&'a str),
    Literal(&'a str),
    DirEntryField { binding: &'a str, field: &'a str },
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
        Expr::Variable(ident, ..)
            if ctx.scope.get(ident.1.as_str()).copied() == Some(ValueKind::Json) =>
        {
            StringReceiver::JsonBinding(ident.1.as_str())
        }
        Expr::Variable(ident, ..)
            if ctx.scope.get(ident.1.as_str()).copied() == Some(ValueKind::DirEntry) =>
        {
            let Expr::Dot(inner, ..) = &boxed.rhs else {
                return None;
            };
            let Expr::Property(property, ..) = &inner.lhs else {
                return None;
            };
            if !matches!(property.2.as_str(), "file_name" | "path") {
                return None;
            }
            StringReceiver::DirEntryField {
                binding: ident.1.as_str(),
                field: property.2.as_str(),
            }
        }
        Expr::StringConstant(value, ..) => StringReceiver::Literal(value.as_str()),
        _ => return None,
    };
    let call = match &boxed.rhs {
        Expr::MethodCall(call, ..) => call,
        Expr::Dot(inner, ..) => match &inner.rhs {
            Expr::MethodCall(call, ..) => call,
            _ => return None,
        },
        _ => return None,
    };
    Some((receiver, call))
}

fn emit_string_receiver(out: &mut String, receiver: StringReceiver<'_>) {
    match receiver {
        StringReceiver::Binding(name) => out.push_str(name),
        StringReceiver::JsonBinding(name) => {
            out.push_str("rh_json_as_str(&");
            out.push_str(name);
            out.push(')');
        }
        StringReceiver::Literal(value) => {
            out.push_str(&format!("{value:?}"));
        }
        StringReceiver::DirEntryField { binding, field } => {
            out.push_str(binding);
            out.push('.');
            out.push_str(field);
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
        Expr::Variable(ident, ..)
            if ctx.scope.get(ident.1.as_str()).copied() == Some(ValueKind::Json) =>
        {
            out.push_str("rh_json_as_str(&");
            out.push_str(ident.1.as_str());
            out.push(')');
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

fn set_insert_assignment<'a>(
    assign: &'a (OpAssignment, BinaryExpr),
    ctx: &EmitCtx,
) -> Option<(&'a str, &'a Expr)> {
    let (op, bin) = assign;
    if op.get_op_assignment_info().is_some() {
        return None;
    }
    let Expr::Index(boxed, ..) = &bin.lhs else {
        return None;
    };
    let Expr::Variable(ident, ..) = &boxed.lhs else {
        return None;
    };
    if ctx.scope.get(ident.1.as_str()).copied() != Some(ValueKind::Set) {
        return None;
    }
    matches!(bin.rhs, Expr::BoolConstant(true, ..)).then_some((ident.1.as_str(), &boxed.rhs))
}

fn emit_set_key(out: &mut String, key: &Expr, ctx: &EmitCtx) -> Result<(), RhError> {
    match key {
        Expr::StringConstant(value, ..) => {
            out.push_str("String::from(");
            out.push_str(&format!("{value:?}"));
            out.push(')');
            Ok(())
        }
        Expr::Variable(ident, ..)
            if ctx.scope.get(ident.1.as_str()).copied() == Some(ValueKind::String) =>
        {
            out.push_str(ident.1.as_str());
            out.push_str(".clone()");
            Ok(())
        }
        Expr::Variable(ident, ..)
            if ctx.scope.get(ident.1.as_str()).copied() == Some(ValueKind::Json) =>
        {
            out.push_str("rh_json_as_str(&");
            out.push_str(ident.1.as_str());
            out.push(')');
            Ok(())
        }
        _ => Err(RhError::Transpile(
            "set index key must be a string binding or literal".into(),
        )),
    }
}

fn parse_set_method_call<'a>(
    expr: &'a Expr,
    ctx: &EmitCtx,
) -> Option<(&'a str, &'a rhai::FnCallExpr)> {
    let Expr::Dot(boxed, ..) = expr else {
        return None;
    };
    let Expr::Variable(ident, ..) = &boxed.lhs else {
        return None;
    };
    if ctx.scope.get(ident.1.as_str()).copied() != Some(ValueKind::Set) {
        return None;
    }
    let Expr::MethodCall(call, ..) = &boxed.rhs else {
        return None;
    };
    Some((ident.1.as_str(), call))
}

fn emit_set_predicate(out: &mut String, expr: &Expr, ctx: &mut EmitCtx) -> Result<bool, RhError> {
    let Some((binding, call)) = parse_set_method_call(expr, ctx) else {
        return Ok(false);
    };
    if call.name != "contains" || call.args.len() != 1 {
        return Ok(false);
    }
    let mut needle = String::new();
    if !emit_string_needle(&mut needle, &call.args[0], ctx)? {
        return Ok(false);
    }
    out.push('(');
    out.push_str(binding);
    out.push_str(".contains(");
    out.push_str(&needle);
    out.push_str(") as INT)");
    Ok(true)
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
    let Some((receiver, call)) = parse_string_method_call(expr, ctx) else {
        return Ok(false);
    };
    match (receiver, call.name.as_str()) {
        (StringReceiver::Binding(binding), "trim") if call.args.is_empty() => {
            out.push_str(binding);
            out.push_str(" = ");
            out.push_str(binding);
            out.push_str(".trim().to_string()");
            Ok(true)
        }
        (StringReceiver::Binding(binding), "to_lower") if call.args.is_empty() => {
            out.push_str(binding);
            out.push_str(" = ");
            out.push_str(binding);
            out.push_str(".to_ascii_lowercase()");
            Ok(true)
        }
        (StringReceiver::JsonBinding(binding), "trim") if call.args.is_empty() => {
            out.push_str(binding);
            out.push_str(" = serde_json::Value::String(rh_json_as_str(&");
            out.push_str(binding);
            out.push_str(").trim().to_string())");
            Ok(true)
        }
        (StringReceiver::Binding(binding), "replace") if call.args.len() == 2 => {
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
        (StringReceiver::JsonBinding(binding), "replace") if call.args.len() == 2 => {
            let mut from = String::new();
            let mut to = String::new();
            if !emit_native_string(&mut from, &call.args[0], ctx)?
                || !emit_native_string(&mut to, &call.args[1], ctx)?
            {
                return Ok(false);
            }
            out.push_str(binding);
            out.push_str(" = serde_json::Value::String(rh_json_as_str(&");
            out.push_str(binding);
            out.push_str(").replace(");
            out.push_str(&from);
            out.push_str(", ");
            out.push_str(&to);
            out.push_str("))");
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

fn emit_path_absolute(out: &mut String, path: &Expr, ctx: &mut EmitCtx) -> Result<bool, RhError> {
    let mut path_expr = String::new();
    if !emit_native_string(&mut path_expr, path, ctx)? {
        return Ok(false);
    }
    out.push_str("rh_path_absolute(");
    out.push_str(&path_expr);
    out.push(')');
    Ok(true)
}

fn emit_std_fs_symlink_metadata(
    out: &mut String,
    path: &Expr,
    ctx: &mut EmitCtx,
) -> Result<bool, RhError> {
    let mut path_expr = String::new();
    if !emit_native_string(&mut path_expr, path, ctx)? {
        return Ok(false);
    }
    out.push_str("rh_symlink_metadata(");
    out.push_str(&path_expr);
    out.push(')');
    Ok(true)
}

fn emit_std_fs_remove_file(
    out: &mut String,
    path: &Expr,
    ctx: &mut EmitCtx,
) -> Result<bool, RhError> {
    let mut path_expr = String::new();
    if !emit_native_string(&mut path_expr, path, ctx)? {
        return Ok(false);
    }
    out.push_str("rh_remove_file(");
    out.push_str(&path_expr);
    out.push(')');
    Ok(true)
}

fn emit_std_fs_try_remove_file(
    out: &mut String,
    path: &Expr,
    ctx: &mut EmitCtx,
) -> Result<bool, RhError> {
    let mut path_expr = String::new();
    if !emit_native_string(&mut path_expr, path, ctx)? {
        return Ok(false);
    }
    out.push_str("rh_try_remove_file(");
    out.push_str(&path_expr);
    out.push(')');
    Ok(true)
}

fn emit_std_fs_copy(
    out: &mut String,
    src: &Expr,
    dst: &Expr,
    ctx: &mut EmitCtx,
) -> Result<bool, RhError> {
    let mut src_expr = String::new();
    let mut dst_expr = String::new();
    if !emit_native_string(&mut src_expr, src, ctx)?
        || !emit_native_string(&mut dst_expr, dst, ctx)?
    {
        return Ok(false);
    }
    out.push_str("rh_copy(");
    out.push_str(&src_expr);
    out.push_str(", ");
    out.push_str(&dst_expr);
    out.push(')');
    Ok(true)
}

fn emit_std_fs_try_copy(
    out: &mut String,
    src: &Expr,
    dst: &Expr,
    ctx: &mut EmitCtx,
) -> Result<bool, RhError> {
    let mut src_expr = String::new();
    let mut dst_expr = String::new();
    if !emit_native_string(&mut src_expr, src, ctx)?
        || !emit_native_string(&mut dst_expr, dst, ctx)?
    {
        return Ok(false);
    }
    out.push_str("rh_try_copy(");
    out.push_str(&src_expr);
    out.push_str(", ");
    out.push_str(&dst_expr);
    out.push(')');
    Ok(true)
}

fn emit_std_fs_create_dir_all(
    out: &mut String,
    path: &Expr,
    ctx: &mut EmitCtx,
) -> Result<bool, RhError> {
    let mut path_expr = String::new();
    if !emit_native_string(&mut path_expr, path, ctx)? {
        return Ok(false);
    }
    out.push_str("rh_create_dir_all(");
    out.push_str(&path_expr);
    out.push(')');
    Ok(true)
}

fn emit_std_fs_try_create_dir_all(
    out: &mut String,
    path: &Expr,
    ctx: &mut EmitCtx,
) -> Result<bool, RhError> {
    let mut path_expr = String::new();
    if !emit_native_string(&mut path_expr, path, ctx)? {
        return Ok(false);
    }
    out.push_str("rh_try_create_dir_all(");
    out.push_str(&path_expr);
    out.push(')');
    Ok(true)
}

fn emit_std_fs_rename(
    out: &mut String,
    src: &Expr,
    dst: &Expr,
    ctx: &mut EmitCtx,
) -> Result<bool, RhError> {
    let mut src_expr = String::new();
    let mut dst_expr = String::new();
    if !emit_native_string(&mut src_expr, src, ctx)?
        || !emit_native_string(&mut dst_expr, dst, ctx)?
    {
        return Ok(false);
    }
    out.push_str("rh_rename(");
    out.push_str(&src_expr);
    out.push_str(", ");
    out.push_str(&dst_expr);
    out.push(')');
    Ok(true)
}

fn emit_std_fs_try_rename(
    out: &mut String,
    src: &Expr,
    dst: &Expr,
    ctx: &mut EmitCtx,
) -> Result<bool, RhError> {
    let mut src_expr = String::new();
    let mut dst_expr = String::new();
    if !emit_native_string(&mut src_expr, src, ctx)?
        || !emit_native_string(&mut dst_expr, dst, ctx)?
    {
        return Ok(false);
    }
    out.push_str("rh_try_rename(");
    out.push_str(&src_expr);
    out.push_str(", ");
    out.push_str(&dst_expr);
    out.push(')');
    Ok(true)
}

fn emit_metadata_property(
    out: &mut String,
    expr: &Expr,
    ctx: &mut EmitCtx,
) -> Result<bool, RhError> {
    if let Some((binding, property)) = metadata_property_binding(expr, ctx) {
        out.push_str(binding);
        out.push('.');
        out.push_str(property);
        return Ok(true);
    }
    let Some((path, property)) = symlink_metadata_property(expr) else {
        return Ok(false);
    };
    let mut path_expr = String::new();
    if !emit_native_string(&mut path_expr, path, ctx)? {
        return Ok(false);
    }
    out.push_str("rh_symlink_metadata(");
    out.push_str(&path_expr);
    out.push(')');
    out.push('.');
    out.push_str(property);
    Ok(true)
}

fn emit_dir_entry_property(out: &mut String, expr: &Expr, ctx: &EmitCtx) -> Result<bool, RhError> {
    if let Some((binding, field)) = dir_entry_int_field(expr, ctx) {
        out.push_str(binding);
        out.push('.');
        out.push_str(field);
        return Ok(true);
    }
    if let Some((binding, field)) = dir_entry_string_field(expr, ctx) {
        out.push_str(binding);
        out.push('.');
        out.push_str(field);
        return Ok(true);
    }
    Ok(false)
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
    if call.namespace.is_empty() && call.name == "print" && call.args.len() == 1 && ctx.cdylib {
        out.push_str("rh_print(&");
        emit_stringish(out, &call.args[0], ctx)?;
        out.push(')');
        return Ok(());
    }
    if call.namespace.is_empty() && ctx.local_fns.contains(call.name.as_str()) && ctx.cdylib {
        let sig = ctx
            .local_fn_sigs
            .get(call.name.as_str())
            .cloned()
            .unwrap_or_default();
        out.push_str(call.name.as_str());
        out.push('(');
        for (index, arg) in call.args.iter().enumerate() {
            if index > 0 {
                out.push_str(", ");
            }
            match sig.get(index).copied() {
                Some(ValueKind::String) => emit_string_arg(out, arg, ctx)?,
                Some(ValueKind::Json) => emit_json_arg(out, arg, ctx)?,
                _ => emit_expr(out, arg, ctx)?,
            }
        }
        out.push(')');
        return Ok(());
    }
    Err(RhError::Transpile(format!(
        "unsupported call `{}` in rh-2",
        call.name
    )))
}

fn emit_json_arg(out: &mut String, expr: &Expr, ctx: &mut EmitCtx) -> Result<(), RhError> {
    match expr {
        Expr::Variable(ident, ..)
            if ctx.scope.get(ident.1.as_str()).copied() == Some(ValueKind::Json) =>
        {
            out.push_str(ident.1.as_str());
            out.push_str(".clone()");
            Ok(())
        }
        Expr::Map(..) | Expr::Array(..) => emit_json_value_expr(out, expr, ctx),
        _ if json_value_path(expr, ctx).is_some() || json_array_index(expr, ctx).is_some() => {
            emit_json_value_expr(out, expr, ctx)
        }
        _ => Err(RhError::Transpile(
            "local fn JSON argument must be a JSON value".into(),
        )),
    }
}

fn emit_string_arg(out: &mut String, expr: &Expr, ctx: &mut EmitCtx) -> Result<(), RhError> {
    match expr {
        Expr::Variable(ident, ..)
            if ctx.scope.get(ident.1.as_str()).copied() == Some(ValueKind::String) =>
        {
            out.push_str(ident.1.as_str());
            out.push_str(".clone()");
        }
        Expr::Variable(ident, ..)
            if ctx.scope.get(ident.1.as_str()).copied() == Some(ValueKind::Json) =>
        {
            out.push_str("rh_json_as_str(&");
            out.push_str(ident.1.as_str());
            out.push(')');
        }
        _ => emit_stringish(out, expr, ctx)?,
    }
    Ok(())
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
    use super::{
        CdylibExecutionMode, transpile, transpile_cdylib, transpile_cdylib_with_mode,
        transpile_cdylib_with_project,
    };

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
    fn cdylib_transpile_emits_map_set_membership() {
        let source = include_str!("../../../fixtures/rh/map-set-membership.rh");
        let output = transpile_cdylib_with_mode(source).expect("transpile");
        assert_eq!(
            output.execution_mode,
            CdylibExecutionMode::Native,
            "{}",
            output.rust
        );
        assert!(output.rust.contains("HashSet::<String>::new()"));
        assert!(output.rust.contains(".contains("));
        assert!(output.rust.contains(".insert("));
        assert!(!output.rust.contains("rh_host_run_script(RH_SCRIPT_SOURCE)"));
        assert_eq!(output.rust.matches("rh_host_eval_int(").count(), 1);
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
    fn cdylib_transpile_emits_path_absolute_and_symlink_metadata() {
        let source = include_str!("../../../fixtures/rh/path-metadata-probe.rh");
        let output = transpile_cdylib_with_mode(source).expect("transpile");
        assert_eq!(
            output.execution_mode,
            CdylibExecutionMode::Native,
            "{}",
            output.rust
        );
        assert!(output.rust.contains("rh_path_absolute("));
        assert!(output.rust.contains("rh_symlink_metadata("));
        assert!(output.rust.contains("meta.is_file"));
        assert!(output.rust.contains("meta.is_symlink"));
        assert!(output.rust.contains("meta.is_reparse_point"));
        assert!(!output.rust.contains("rh_host_run_script(RH_SCRIPT_SOURCE)"));
        assert_eq!(output.rust.matches("rh_host_eval_int(").count(), 1);
    }

    #[test]
    fn cdylib_transpile_with_project_bundles_import_calls() {
        let source = include_str!("../../../fixtures/rh/import-bundle-probe.rh");
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let output = super::transpile_cdylib_with_project(&root, source).expect("transpile");
        assert_eq!(
            output.execution_mode,
            CdylibExecutionMode::Native,
            "{}",
            output.rust
        );
        assert!(output.rust.contains("pub fn add("), "{}", output.rust);
        assert!(output.rust.contains("add(40, 2)"), "{}", output.rust);
        assert!(!output.rust.contains("rh_host_run_script(RH_SCRIPT_SOURCE)"));
        assert_eq!(output.rust.matches("rh_host_eval_int(").count(), 1);
    }

    #[test]
    fn cdylib_transpile_emits_nested_json_parse_read_to_string() {
        let output = transpile_cdylib_with_mode(
            "fn entry() { let path = args[0]; let doc = rhai::json::parse(std::fs::read_to_string(path)); doc.schema_version }",
        )
        .expect("transpile");
        assert_eq!(
            output.execution_mode,
            CdylibExecutionMode::Native,
            "{}",
            output.rust
        );
        assert!(
            output
                .rust
                .contains("rh_json_parse(&rh_std_fs_read_to_string(&path))"),
            "{}",
            output.rust
        );
    }

    #[test]
    fn cdylib_transpile_keeps_require_inside_statement_if() {
        let source = r#"
fn entry() {
    let total = 0;
    for index in 1..=2 {
        if index == 1 {
            require(index > 0, "pos");
        } else {
            require(index > 1, "gt");
        }
        total += index;
    }
    total
}
"#;
        let output = transpile_cdylib_with_mode(source).expect("transpile");
        assert_eq!(
            output.execution_mode,
            CdylibExecutionMode::Native,
            "{}",
            output.rust
        );
        assert!(
            !output.rust.contains("return {\n    if "),
            "statement if/require must not early-return:\n{}",
            output.rust
        );
        assert!(output.rust.contains("total += index"), "{}", output.rust);
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
            rhai::Stmt::Expr(expr) => {
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

    #[test]
    fn cdylib_transpile_emits_command_stdout_file_native() {
        let output = transpile_cdylib_with_mode(
            r#"fn entry() {
    let repo = args[0];
    let out = args[1];
    std::process::command_stdout_file("git", ["-C", repo, "rev-parse", "--show-prefix"], 10000, out)
}"#,
        )
        .expect("transpile");
        assert_eq!(
            output.execution_mode,
            CdylibExecutionMode::Native,
            "{}",
            output.rust
        );
        assert!(
            output.rust.contains("rh_process_stdout_file("),
            "{}",
            output.rust
        );
        assert!(output.rust.contains(", None)"), "{}", output.rust);
        assert!(output.rust.contains("\"--show-prefix\""), "{}", output.rust);
        assert!(
            !output
                .rust
                .contains("rh_host_eval_int(\"std::process::command_stdout_file")
        );
        assert!(!output.rust.contains("rh_host_run_script(RH_SCRIPT_SOURCE)"));
    }

    #[test]
    fn cdylib_transpile_emits_command_stdout_file_with_process_options() {
        let output = transpile_cdylib_with_mode(
            r#"fn entry() {
    let repo = args[0];
    let out = args[1];
    std::process::command_stdout_file(
        "git",
        ["rev-parse", "--show-prefix"],
        10000,
        out,
        #{
            current_dir: repo,
            env: #{ "AGENTERM_NO_ACTIVATE": "1" },
            env_remove: ["AGENTERM_IPC_ADDRESS"],
        }
    )
}"#,
        )
        .expect("transpile");
        assert_eq!(
            output.execution_mode,
            CdylibExecutionMode::Native,
            "{}",
            output.rust
        );
        assert!(
            output.rust.contains("rh_process_stdout_file("),
            "{}",
            output.rust
        );
        assert!(
            output.rust.contains("\"current_dir\""),
            "{}",
            output.rust
        );
        assert!(output.rust.contains("\"env\""), "{}", output.rust);
        assert!(
            output.rust.contains("\"env_remove\""),
            "{}",
            output.rust
        );
        assert!(
            output.rust.contains("AGENTERM_NO_ACTIVATE"),
            "{}",
            output.rust
        );
        assert!(
            !output
                .rust
                .contains("rh_host_eval_int(\"std::process::command_stdout_file")
        );
    }

    #[test]
    fn cdylib_transpile_emits_command_status_with_process_options() {
        let output = transpile_cdylib_with_mode(
            r#"fn entry() {
    let repo = args[0];
    std::process::command_status(
        "git",
        ["status", "--porcelain"],
        5000,
        #{ current_dir: repo }
    )
}"#,
        )
        .expect("transpile");
        assert_eq!(
            output.execution_mode,
            CdylibExecutionMode::Native,
            "{}",
            output.rust
        );
        assert!(output.rust.contains("rh_process_status("), "{}", output.rust);
        assert!(
            output.rust.contains("\"current_dir\""),
            "{}",
            output.rust
        );
    }

    #[test]
    fn cdylib_transpile_emits_json_builder_split_and_stringify() {
        let output = transpile_cdylib_with_mode(
            r#"fn entry() {
    let text = "a\nb";
    let lines = text.split("\n");
    let items = [];
    for line in lines {
        items.push(#{ name: line, size: 1 });
    }
    let manifest = #{ schema_version: 2, executables: items };
    let pretty = rhai::json::stringify_pretty(manifest);
    pretty.len + lines.len
}"#,
        )
        .expect("transpile");
        assert_eq!(
            output.execution_mode,
            CdylibExecutionMode::Native,
            "{}",
            output.rust
        );
        assert!(output.rust.contains("rh_string_split("), "{}", output.rust);
        assert!(
            output.rust.contains("rh_json_array_push("),
            "{}",
            output.rust
        );
        assert!(
            output.rust.contains("rh_json_stringify_pretty("),
            "{}",
            output.rust
        );
        assert!(
            output.rust.contains("serde_json::Map::new()"),
            "{}",
            output.rust
        );
        assert!(!output.rust.contains("rh_host_run_script(RH_SCRIPT_SOURCE)"));
    }

    #[test]
    fn cdylib_transpile_emits_env_sha256_atomic_rfc3339_and_to_lower() {
        let output = transpile_cdylib_with_mode(
            r#"fn entry() {
    if std::env::has("PATH") {
        let path = args[0];
        let digest = rhai::crypto::sha256_file(path);
        digest.to_lower();
        let _stamp = std::time::SystemTime::now().rfc3339;
        rhai::runtime::atomic_write(path, digest);
        let meta = std::fs::symlink_metadata(path);
        meta.len
    } else {
        0
    }
}"#,
        )
        .expect("transpile");
        assert_eq!(
            output.execution_mode,
            CdylibExecutionMode::Native,
            "{}",
            output.rust
        );
        assert!(output.rust.contains("rh_env_has("), "{}", output.rust);
        assert!(output.rust.contains("rh_sha256_file("), "{}", output.rust);
        assert!(output.rust.contains("rh_atomic_write("), "{}", output.rust);
        assert!(
            output.rust.contains("rh_system_time_now_rfc3339()"),
            "{}",
            output.rust
        );
        assert!(
            output.rust.contains(".to_ascii_lowercase()"),
            "{}",
            output.rust
        );
        assert!(output.rust.contains("meta.len"), "{}", output.rust);
        assert!(!output.rust.contains("rh_host_run_script(RH_SCRIPT_SOURCE)"));
    }

    #[test]
    fn cdylib_transpile_emits_std_fs_remove_file_native() {
        let output = transpile_cdylib_with_mode(
            "fn entry() { let path = args[0]; std::fs::remove_file(path); std::fs::remove_file(path) }",
        )
        .expect("transpile");
        assert_eq!(
            output.execution_mode,
            CdylibExecutionMode::Native,
            "{}",
            output.rust
        );
        assert_eq!(output.rust.matches("rh_remove_file(&path)").count(), 2);
        assert!(
            !output
                .rust
                .contains("rh_host_eval_int(\"std::fs::remove_file")
        );
        assert!(!output.rust.contains("rh_host_run_script(RH_SCRIPT_SOURCE)"));
    }

    #[test]
    fn cdylib_transpile_emits_std_fs_try_remove_file_native() {
        let output = transpile_cdylib_with_mode(
            "fn entry() { let path = args[0]; std::fs::try_remove_file(path) }",
        )
        .expect("transpile");
        assert_eq!(
            output.execution_mode,
            CdylibExecutionMode::Native,
            "{}",
            output.rust
        );
        assert!(output.rust.contains("rh_try_remove_file(&path)"));
        assert!(
            !output
                .rust
                .contains("rh_host_eval_int(\"std::fs::try_remove_file")
        );
    }

    #[test]
    fn cdylib_transpile_emits_read_dir_for_loop_native() {
        let source = r#"
fn entry() {
    let removed = 0;
    let directory = args[0];
    for dir_entry in std::fs::read_dir(directory) {
        if dir_entry.is_file
                && dir_entry.file_name.starts_with("agenterm")
                && dir_entry.file_name.ends_with(".exe") {
            if std::fs::try_remove_file(dir_entry.path.display) != 0 {
                removed += 1;
            }
        }
    }
    if std::fs::symlink_metadata(directory).is_dir {
        removed += 100;
    }
    std::fs::remove_file(args[1]);
    removed
}
"#;
        let output = transpile_cdylib_with_mode(source).expect("transpile");
        assert_eq!(
            output.execution_mode,
            CdylibExecutionMode::Native,
            "{}",
            output.rust
        );
        assert!(
            output
                .rust
                .contains("for dir_entry in rh_read_dir(&directory)")
        );
        assert!(output.rust.contains("dir_entry.is_file"));
        assert!(output.rust.contains("dir_entry.file_name.starts_with("));
        assert!(output.rust.contains("rh_try_remove_file(&dir_entry.path)"));
        assert!(
            output
                .rust
                .contains("rh_symlink_metadata(&directory).is_dir")
        );
        assert!(output.rust.contains("rh_remove_file("));
        assert!(!output.rust.contains("rh_host_run_script(RH_SCRIPT_SOURCE)"));
        assert_eq!(output.rust.matches("rh_host_eval_int(").count(), 1);
    }

    #[test]
    fn cdylib_transpile_infers_string_params_for_read_dir_prefix() {
        let source = r#"
fn clean_locked_for_name(directory, prefix) {
    let removed = 0;
    for dir_entry in std::fs::read_dir(directory) {
        if dir_entry.is_file && dir_entry.file_name.starts_with(prefix) {
            removed += std::fs::try_remove_file(dir_entry.path.display);
        }
    }
    removed
}
fn entry() { clean_locked_for_name(args[0], args[1]) }
"#;
        let output = transpile_cdylib_with_mode(source).expect("transpile");
        assert_eq!(
            output.execution_mode,
            CdylibExecutionMode::Native,
            "{}",
            output.rust
        );
        assert!(output.rust.contains("directory: String"), "{}", output.rust);
        assert!(output.rust.contains("prefix: String"), "{}", output.rust);
        assert!(
            output.rust.contains("starts_with(prefix.as_str())"),
            "{}",
            output.rust
        );
        assert!(!output.rust.contains("rh_host_eval_int(\"for dir_entry"));
    }

    #[test]
    fn cdylib_transpile_emits_chained_symlink_metadata_property_native() {
        let output = transpile_cdylib_with_mode(
            "fn entry() { let path = args[0]; if std::fs::symlink_metadata(path).is_file { 1 } else { 0 } }",
        )
        .expect("transpile");
        assert_eq!(
            output.execution_mode,
            CdylibExecutionMode::Native,
            "{}",
            output.rust
        );
        assert!(output.rust.contains("rh_symlink_metadata(&path).is_file"));
        assert!(
            !output
                .rust
                .contains("rh_host_eval_int(\"std::fs::symlink_metadata")
        );
    }

    #[test]
    fn cdylib_transpile_emits_std_fs_copy_native() {
        let output = transpile_cdylib_with_mode(
            "fn entry() { let src = args[0]; let dst = args[1]; std::fs::copy(src, dst) }",
        )
        .expect("transpile");
        assert_eq!(
            output.execution_mode,
            CdylibExecutionMode::Native,
            "{}",
            output.rust
        );
        assert!(output.rust.contains("rh_copy(&src, &dst)"));
        assert!(!output.rust.contains("rh_host_eval_int(\"std::fs::copy"));
        assert!(!output.rust.contains("rh_host_run_script(RH_SCRIPT_SOURCE)"));
    }

    #[test]
    fn cdylib_transpile_emits_std_fs_try_copy_native() {
        let output = transpile_cdylib_with_mode(
            "fn entry() { let src = args[0]; let dst = args[1]; std::fs::try_copy(src, dst) }",
        )
        .expect("transpile");
        assert_eq!(
            output.execution_mode,
            CdylibExecutionMode::Native,
            "{}",
            output.rust
        );
        assert!(output.rust.contains("rh_try_copy(&src, &dst)"));
        assert!(!output.rust.contains("rh_host_eval_int(\"std::fs::try_copy"));
    }

    #[test]
    fn cdylib_transpile_emits_std_fs_create_dir_all_native() {
        let output = transpile_cdylib_with_mode(
            "fn entry() { let path = args[0]; std::fs::create_dir_all(path); std::fs::create_dir_all(path) }",
        )
        .expect("transpile");
        assert_eq!(
            output.execution_mode,
            CdylibExecutionMode::Native,
            "{}",
            output.rust
        );
        assert_eq!(output.rust.matches("rh_create_dir_all(&path)").count(), 2);
        assert!(
            !output
                .rust
                .contains("rh_host_eval_int(\"std::fs::create_dir_all")
        );
    }

    #[test]
    fn cdylib_transpile_emits_std_fs_rename_native() {
        let output = transpile_cdylib_with_mode(
            "fn entry() { let src = args[0]; let dst = args[1]; std::fs::rename(src, dst) }",
        )
        .expect("transpile");
        assert_eq!(
            output.execution_mode,
            CdylibExecutionMode::Native,
            "{}",
            output.rust
        );
        assert!(output.rust.contains("rh_rename(&src, &dst)"));
        assert!(!output.rust.contains("rh_host_eval_int(\"std::fs::rename"));
    }

    #[test]
    fn cdylib_transpile_infers_string_params_for_copy_and_rename() {
        let source = r#"
fn stage_copy(source, destination) {
    std::fs::create_dir_all(destination);
    if std::fs::try_copy(source, destination) != 0 {
        std::fs::rename(source, destination);
        1
    } else {
        0
    }
}
fn entry() { stage_copy(args[0], args[1]) }
"#;
        let output = transpile_cdylib_with_mode(source).expect("transpile");
        assert_eq!(
            output.execution_mode,
            CdylibExecutionMode::Native,
            "{}",
            output.rust
        );
        assert!(output.rust.contains("source: String"), "{}", output.rust);
        assert!(
            output.rust.contains("destination: String"),
            "{}",
            output.rust
        );
        assert!(output.rust.contains("rh_create_dir_all(&destination)"));
        assert!(output.rust.contains("rh_try_copy(&source, &destination)"));
        assert!(output.rust.contains("rh_rename(&source, &destination)"));
        assert!(!output.rust.contains("rh_host_eval_int(\"std::fs::copy"));
    }

    #[test]
    fn cdylib_transpile_emits_system_time_now_unix_millis_fast_path() {
        let output = transpile_cdylib_with_mode(
            "fn entry() { let suffix = std::time::SystemTime::now().unix_millis; suffix + 1 }",
        )
        .expect("transpile");
        assert_eq!(
            output.execution_mode,
            CdylibExecutionMode::Native,
            "{}",
            output.rust
        );
        assert!(output.rust.contains("rh_system_time_now_unix_millis()"));
        assert!(
            !output
                .rust
                .contains("rh_host_eval_int(\"std::time::SystemTime::now().unix_millis")
        );
        assert_eq!(output.rust.matches("rh_host_eval_int(").count(), 1);
    }

    #[test]
    fn write_build_metadata_project_transpiles_native() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("repo root");
        let source = std::fs::read_to_string(root.join("scripts/rh/write-build-metadata.rh"))
            .expect("entry");
        let bundled = crate::bundle_project_source(&root, &source).expect("bundle");
        let ast = super::parse(&bundled).expect("parse");
        crate::subset::validate_ast(&ast).expect("validate");
        let output = transpile_cdylib_with_project(&root, &source).expect("transpile");
        assert_eq!(
            output.execution_mode,
            CdylibExecutionMode::Native,
            "{}",
            output.rust
        );
        assert!(
            output.rust.contains("rh_process_stdout_file("),
            "{}",
            output.rust
        );
        assert!(output.rust.contains("rh_sha256_file("), "{}", output.rust);
        assert!(output.rust.contains("rh_atomic_write("), "{}", output.rust);
        assert!(
            output.rust.contains("rh_json_stringify_pretty("),
            "{}",
            output.rust
        );
        assert!(
            output.rust.contains("serde_json::Value::Bool("),
            "{}",
            output.rust
        );
        assert!(
            output
                .rust
                .contains("rh_json_get_path(&artifact, &[\"name\"])"),
            "{}",
            output.rust
        );
        assert!(!output.rust.contains("rh_host_run_script(RH_SCRIPT_SOURCE)"));
        // Host API always defines `fn rh_host_eval_int`; Native packs must not call it.
        assert_eq!(output.rust.matches("rh_host_eval_int(").count(), 1);
    }
}
