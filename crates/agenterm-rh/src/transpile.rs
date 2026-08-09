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
    host_api::{emit_host_runtime, host_api_module},
    subset::validate_ast,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ValueKind {
    Int,
    Bool,
    String,
    Char,
    Json,
    Set,
    StringList,
    Metadata,
    SystemTime,
    DirEntry,
    /// `std::path::PathBuf` binding stored as a UTF-8 path string.
    Path,
    Command,
    Output,
    Child,
    ChildList,
    WindowControl,
    WindowRect,
    Stream,
    Bytes,
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
    local_fn_return_kinds: BTreeMap<String, ValueKind>,
    /// Return kind of the function body currently being emitted (`entry` defaults to Int).
    current_return_kind: ValueKind,
    try_depth: u32,
    /// Locals initialized as `[]` that later `push` Child bindings (owned_children).
    empty_child_lists: BTreeSet<String>,
    empty_string_lists: BTreeSet<String>,
    set_map_bindings: BTreeSet<String>,
    binding_aliases: BTreeMap<String, String>,
}

impl EmitCtx {
    fn new(cdylib: bool) -> Self {
        Self {
            cdylib,
            scope: BTreeMap::new(),
            local_fns: BTreeSet::new(),
            local_fn_sigs: BTreeMap::new(),
            local_fn_return_kinds: BTreeMap::new(),
            current_return_kind: ValueKind::Int,
            try_depth: 0,
            empty_child_lists: BTreeSet::new(),
            empty_string_lists: BTreeSet::new(),
            set_map_bindings: BTreeSet::new(),
            binding_aliases: BTreeMap::new(),
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

    fn with_local_fn_return_kinds(
        mut self,
        local_fn_return_kinds: BTreeMap<String, ValueKind>,
    ) -> Self {
        self.local_fn_return_kinds = local_fn_return_kinds;
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

    // Unreferenced today (CI's dead-code gate proved it): kept under an
    // explicit expectation instead of deleted because it is a complete,
    // typed scope→JSON emitter that the AOT lane may be mid-wiring. Listed
    // in plan/design-binary-size-and-reuse.md §5.3 — if it is still
    // unreferenced when that inventory's cooling window closes, delete it
    // (git history is the archive); wiring it up removes this attribute.
    #[expect(
        dead_code,
        reason = "graybox inventory §5.3: possible in-progress AOT wiring"
    )]
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
            // Fragments are embedded inside a generated `format!("…")` string, so
            // literal `{` / `}` must be doubled once more than the meta `format!` here.
            match kind {
                ValueKind::Int => {
                    out.push_str(&format!(
                        "\\\"{name}\\\":{{{{\\\"kind\\\":\\\"int\\\",\\\"value\\\":{{}}}}}}"
                    ));
                }
                ValueKind::Bool => {
                    out.push_str(&format!(
                        "\\\"{name}\\\":{{{{\\\"kind\\\":\\\"bool\\\",\\\"value\\\":{{}}}}}}"
                    ));
                }
                ValueKind::String | ValueKind::Char | ValueKind::Path => {
                    out.push_str(&format!(
                        "\\\"{name}\\\":{{{{\\\"kind\\\":\\\"string\\\",\\\"value\\\":{{}}}}}}"
                    ));
                }
                ValueKind::Json => {
                    out.push_str(&format!(
                        "\\\"{name}\\\":{{{{\\\"kind\\\":\\\"json\\\",\\\"value\\\":{{}}}}}}"
                    ));
                }
                ValueKind::Set | ValueKind::StringList => {
                    out.push_str(&format!(
                        "\\\"{name}\\\":{{{{\\\"kind\\\":\\\"json\\\",\\\"value\\\":{{}}}}}}"
                    ));
                }
                ValueKind::Metadata | ValueKind::SystemTime => {
                    out.push_str(&format!(
                        "\\\"{name}\\\":{{{{\\\"kind\\\":\\\"json\\\",\\\"value\\\":{{}}}}}}"
                    ));
                }
                ValueKind::DirEntry => {
                    out.push_str(&format!(
                        "\\\"{name}\\\":{{{{\\\"kind\\\":\\\"json\\\",\\\"value\\\":{{}}}}}}"
                    ));
                }
                ValueKind::Command
                | ValueKind::Output
                | ValueKind::Child
                | ValueKind::ChildList
                | ValueKind::WindowControl
                | ValueKind::WindowRect
                | ValueKind::Stream
                | ValueKind::Bytes => {
                    out.push_str(&format!(
                        "\\\"{name}\\\":{{{{\\\"kind\\\":\\\"json\\\",\\\"value\\\":{{}}}}}}"
                    ));
                }
            }
        }
        out.push_str("}}}}\"");
        for (name, kind) in &self.scope {
            out.push_str(", ");
            if matches!(
                kind,
                ValueKind::String | ValueKind::Path | ValueKind::Json | ValueKind::StringList
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
            } else if matches!(
                kind,
                ValueKind::Metadata
                    | ValueKind::SystemTime
                    | ValueKind::DirEntry
                    | ValueKind::Command
                    | ValueKind::Output
                    | ValueKind::Child
                    | ValueKind::ChildList
                    | ValueKind::WindowControl
                    | ValueKind::WindowRect
                    | ValueKind::Stream
                    | ValueKind::Bytes
            ) {
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

    fn with_empty_child_lists(mut self, empty_child_lists: BTreeSet<String>) -> Self {
        self.empty_child_lists = empty_child_lists;
        self
    }

    fn resolve_binding<'a>(&'a self, name: &'a str) -> &'a str {
        self.binding_aliases
            .get(name)
            .map(|alias| alias.as_str())
            .unwrap_or(name)
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
    if !ast_has_entry_fn(&ast) {
        return Err(RhError::Transpile("cdylib pack requires fn entry()".into()));
    }
    validate_ast(&ast)?;
    let rust = emit(&ast, EmitCtx::new(true))?;
    Ok(CdylibTranspileOutput {
        rust,
        execution_mode: CdylibExecutionMode::Native,
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

fn parse(source: &str) -> Result<AST, RhError> {
    crate::check::parse_rh_ast(source)
}

/// After a `for` body runs on a cloned `loop_ctx`, copy upgraded kinds for
/// bindings that already existed in the outer scope (excluding the loop
/// counter). Without this, `let x = null; for e in arr { x = e; } … x.field`
/// keeps `x` typed as the pre-loop kind and poisons later JSON string/int emits.
fn merge_loop_binding_upgrades(outer: &mut EmitCtx, loop_ctx: &EmitCtx, counter: &str) {
    let outer_names: Vec<String> = outer.scope.keys().cloned().collect();
    for name in outer_names {
        if name == counter {
            continue;
        }
        let Some(new_kind) = loop_ctx.scope.get(&name).copied() else {
            continue;
        };
        let Some(old) = outer.scope.get(&name).copied() else {
            continue;
        };
        if old == new_kind {
            continue;
        }
        let clobber = matches!(
            (old, new_kind),
            (
                ValueKind::String
                    | ValueKind::Path
                    | ValueKind::Json
                    | ValueKind::Child
                    | ValueKind::ChildList
                    | ValueKind::Command
                    | ValueKind::StringList
                    | ValueKind::Set
                    | ValueKind::Output,
                ValueKind::Int | ValueKind::Bool
            )
        );
        if !clobber {
            *outer = outer.clone().with_binding(&name, new_kind);
        }
    }
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
    let mut call_site_defs: Vec<&ScriptFuncDef> = Vec::new();
    for meta in ast.iter_functions() {
        if meta.name == "cc_lines" {
            continue;
        }
        let Some(def) = find_fn_def(ast, meta.name) else {
            continue;
        };
        // `entry` is not a local helper, but its call sites must upgrade callee
        // params (e.g. `spec(..., argv: StringList, ...)`).
        call_site_defs.push(def);
        if meta.name == "entry" {
            continue;
        }
        local_fn_sigs.insert(meta.name.to_string(), infer_param_kinds(def, &sig_probe));
        local_defs.push(def);
    }
    propagate_local_fn_param_kinds(&local_defs, &mut local_fn_sigs);
    // Iterate return kinds so wrappers like `complete_quiet -> complete_impl`
    // see the callee Json return instead of defaulting to Int. Call-site
    // param upgrades need those return kinds (e.g. `empty_environment()` → Json).
    let mut local_fn_return_kinds = BTreeMap::new();
    let mut changed = true;
    while changed {
        changed = false;
        let return_probe = sig_probe
            .clone()
            .with_local_fn_sigs(local_fn_sigs.clone())
            .with_local_fn_return_kinds(local_fn_return_kinds.clone());
        for def in &local_defs {
            let kind = infer_return_kind(def, &return_probe);
            match local_fn_return_kinds.get(def.name.as_str()).copied() {
                Some(existing) if existing == kind => {}
                _ => {
                    local_fn_return_kinds.insert(def.name.to_string(), kind);
                    changed = true;
                }
            }
        }
        let call_probe = sig_probe
            .clone()
            .with_local_fn_sigs(local_fn_sigs.clone())
            .with_local_fn_return_kinds(local_fn_return_kinds.clone());
        if propagate_callee_param_kinds_from_call_sites(
            &call_site_defs,
            &mut local_fn_sigs,
            &call_probe,
        ) {
            changed = true;
            propagate_local_fn_param_kinds(&local_defs, &mut local_fn_sigs);
        }
    }
    let mut base_ctx = ctx
        .clone()
        .with_local_fns(local_fns)
        .with_local_fn_sigs(local_fn_sigs)
        .with_local_fn_return_kinds(local_fn_return_kinds);
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

fn rust_return_type(kind: ValueKind) -> &'static str {
    match kind {
        ValueKind::String | ValueKind::Path => "String",
        ValueKind::Json => "serde_json::Value",
        ValueKind::Command => "RhCommand",
        ValueKind::Output => "RhOutput",
        ValueKind::Child => "RhChild",
        ValueKind::ChildList => "Vec<RhChild>",
        ValueKind::WindowControl => "RhWindowControl",
        ValueKind::WindowRect => "RhWindowRect",
        ValueKind::StringList => "Vec<String>",
        ValueKind::Stream => "RhStream",
        ValueKind::Bytes => "RhBytes",
        _ => "INT",
    }
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
    let return_kind = ctx
        .local_fn_return_kinds
        .get(def.name.as_str())
        .copied()
        .unwrap_or(ValueKind::Int);
    fn_ctx.current_return_kind = return_kind;
    fn_ctx.empty_child_lists = discover_empty_child_list_bindings(&def.body, ctx);
    fn_ctx.empty_string_lists = discover_empty_string_list_bindings(&def.body, ctx);
    fn_ctx.set_map_bindings = discover_set_map_bindings(&def.body);
    out.push_str("pub fn ");
    out.push_str(def.name.as_str());
    out.push('(');
    for (index, (param, kind)) in def.params.iter().zip(param_kinds.iter()).enumerate() {
        if index > 0 {
            out.push_str(", ");
        }
        if fn_ctx.cdylib {
            // Child/Command methods take &mut self in host helpers. JSON/list
            // params are also mutated in place (path assign, push, remove).
            // Int/Bool params that the body reassigns (e.g. `sequence += 1`)
            // also need `mut`.
            // Command stays by-value `mut RhCommand` (host helpers take `&mut`
            // at the call site via `rh_command_*(...)`). Typing params as
            // `&mut RhCommand` double-borrows (`&mut command` on an `&mut`).
            if matches!(
                *kind,
                ValueKind::Child
                    | ValueKind::Command
                    | ValueKind::Stream
                    | ValueKind::WindowControl
                    | ValueKind::Json
                    | ValueKind::StringList
                    | ValueKind::ChildList
                    | ValueKind::Set
            ) || param_assigned_in_body(def, param.as_str())
            {
                out.push_str("mut ");
            }
            out.push_str(param.as_str());
            out.push_str(": ");
            out.push_str(rust_param_type(*kind));
        } else {
            out.push_str("mut ");
            out.push_str(param.as_str());
            out.push_str(": Dynamic");
        }
    }
    out.push_str(") -> ");
    if fn_ctx.cdylib && def.name != "entry" {
        out.push_str(rust_return_type(return_kind));
    } else {
        out.push_str(fn_ctx.value_type());
    }
    out.push_str(" {\n");
    emit_block(out, &def.body, &mut fn_ctx, true)?;
    out.push_str("}\n\n");
    Ok(())
}

fn infer_param_kinds(def: &ScriptFuncDef, ctx: &EmitCtx) -> Vec<ValueKind> {
    def.params
        .iter()
        .map(|param| {
            if param_used_as_output(def, param.as_str()) {
                ValueKind::Output
            } else if param_used_as_stream(def, param.as_str()) {
                ValueKind::Stream
            } else if param_used_as_bytes(def, param.as_str()) {
                ValueKind::Bytes
            } else if param_used_as_child_list(def, param.as_str()) {
                ValueKind::ChildList
            } else if param_used_as_string_list(def, param.as_str()) {
                ValueKind::StringList
            } else if param_used_as_definite_child(def, param.as_str()) {
                ValueKind::Child
            } else if param_used_as_json(def, param.as_str(), ctx) {
                // JSON before Child/Command: `.state`/`.id` are also RhChild members, so a
                // timing/doc object that reads `.state` plus `.setup_ms` must stay Json.
                ValueKind::Json
            } else if param_used_as_child(def, param.as_str()) {
                ValueKind::Child
            } else if param_used_as_command(def, param.as_str()) {
                ValueKind::Command
            } else if param_used_as_string(def, param.as_str(), ctx) {
                ValueKind::String
            } else {
                ValueKind::Int
            }
        })
        .collect()
}

fn infer_return_kind(def: &ScriptFuncDef, ctx: &EmitCtx) -> ValueKind {
    let mut fn_ctx = ctx.clone();
    fn_ctx.scope.clear();
    let param_kinds = ctx
        .local_fn_sigs
        .get(def.name.as_str())
        .cloned()
        .unwrap_or_else(|| infer_param_kinds(def, ctx));
    for (param, kind) in def.params.iter().zip(param_kinds.iter().copied()) {
        fn_ctx = fn_ctx.with_binding(param.as_str(), kind);
    }
    let stmts: Vec<_> = def.body.iter().collect();
    if stmts.is_empty() {
        return ValueKind::Int;
    }
    for stmt in &stmts[..stmts.len().saturating_sub(1)] {
        infer_stmt_scope(stmt, &mut fn_ctx);
    }
    let Some(last) = stmts.last() else {
        return ValueKind::Int;
    };
    if matches!(
        last,
        Stmt::Return(_, flags, ..) if flags.contains(ASTFlags::BREAK)
    ) {
        if let Some(kind) = infer_non_throw_return_kind(&def.body, &mut fn_ctx.clone()) {
            return kind;
        }
        return ValueKind::Int;
    }
    let expr = match last {
        Stmt::Return(Some(expr), ..) => expr.as_ref(),
        Stmt::Expr(expr) => expr.as_ref(),
        _ => return ValueKind::Int,
    };
    // Empty `#{}` binds as Set for membership locals, but a function that returns
    // `#{}` is a JSON object at local-fn / API boundaries (`script_smoke_empty_env`).
    match infer_binding_kind(expr, &fn_ctx) {
        ValueKind::Set => ValueKind::Json,
        other => other,
    }
}

fn infer_non_throw_return_kind(block: &StmtBlock, ctx: &mut EmitCtx) -> Option<ValueKind> {
    for stmt in block.iter() {
        match stmt {
            Stmt::Return(None, ..) => return Some(ValueKind::Int),
            Stmt::Return(Some(expr), flags, ..) if !flags.contains(ASTFlags::BREAK) => {
                return Some(infer_binding_kind(expr, ctx));
            }
            Stmt::If(boxed, ..) => {
                let flow = boxed.as_ref();
                if let Some(kind) = infer_non_throw_return_kind(&flow.body, &mut ctx.clone())
                    .or_else(|| infer_non_throw_return_kind(&flow.branch, &mut ctx.clone()))
                {
                    return Some(kind);
                }
            }
            Stmt::For(boxed, ..) => {
                let (_, _, flow) = boxed.as_ref();
                if let Some(kind) = infer_non_throw_return_kind(&flow.body, &mut ctx.clone()) {
                    return Some(kind);
                }
            }
            Stmt::While(boxed, ..) => {
                if let Some(kind) =
                    infer_non_throw_return_kind(&boxed.as_ref().body, &mut ctx.clone())
                {
                    return Some(kind);
                }
            }
            Stmt::Block(inner) => {
                if let Some(kind) = infer_non_throw_return_kind(inner, &mut ctx.clone()) {
                    return Some(kind);
                }
            }
            Stmt::TryCatch(boxed, ..) => {
                let flow = boxed.as_ref();
                if let Some(kind) = infer_non_throw_return_kind(&flow.body, &mut ctx.clone())
                    .or_else(|| infer_non_throw_return_kind(&flow.branch, &mut ctx.clone()))
                {
                    return Some(kind);
                }
            }
            _ => {}
        }
        infer_stmt_scope(stmt, ctx);
    }
    None
}

fn infer_stmt_scope(stmt: &Stmt, ctx: &mut EmitCtx) {
    match stmt {
        Stmt::Var(boxed, ..) => {
            let (ident, expr, _) = boxed.as_ref();
            let mut kind = infer_binding_kind(expr, ctx);
            if kind == ValueKind::Json
                && matches!(expr, Expr::Array(items, ..) if items.is_empty())
                && ctx.empty_child_lists.contains(ident.name.as_str())
            {
                kind = ValueKind::ChildList;
            }
            *ctx = ctx.clone().with_binding(ident.name.as_str(), kind);
        }
        Stmt::Assignment(boxed, ..) => {
            let (_, bin) = boxed.as_ref();
            if let Expr::Variable(ident, ..) = &bin.lhs {
                let kind = infer_binding_kind(&bin.rhs, ctx);
                *ctx = ctx.clone().with_binding(ident.1.as_str(), kind);
            }
        }
        Stmt::If(boxed, ..) => {
            let flow = boxed.as_ref();
            for inner in flow.body.iter().chain(flow.branch.iter()) {
                infer_stmt_scope(inner, ctx);
            }
        }
        Stmt::For(boxed, ..) => {
            let (_, _, flow) = boxed.as_ref();
            for inner in flow.body.iter() {
                infer_stmt_scope(inner, ctx);
            }
        }
        Stmt::While(boxed, ..) => {
            let flow = boxed.as_ref();
            for inner in flow.body.iter() {
                infer_stmt_scope(inner, ctx);
            }
        }
        Stmt::Block(block) => {
            for inner in block.iter() {
                infer_stmt_scope(inner, ctx);
            }
        }
        Stmt::TryCatch(boxed, ..) => {
            // Assignments in `try` (e.g. `identity_environment = map_environment(...)`)
            // must update the outer scope; otherwise later JSON args stay typed as Set.
            let flow = boxed.as_ref();
            for inner in flow.body.iter() {
                infer_stmt_scope(inner, ctx);
            }
            for inner in flow.branch.iter() {
                infer_stmt_scope(inner, ctx);
            }
        }
        _ => {}
    }
}

fn is_param_kind_upgrade(kind: ValueKind) -> bool {
    matches!(
        kind,
        ValueKind::String
            | ValueKind::Path
            | ValueKind::Json
            | ValueKind::Command
            | ValueKind::Output
            | ValueKind::Child
            | ValueKind::ChildList
            | ValueKind::WindowControl
            | ValueKind::WindowRect
            | ValueKind::StringList
            | ValueKind::Stream
            | ValueKind::Bytes
    )
}

fn apply_param_kind_upgrade(sig: &mut [ValueKind], index: usize, kind: ValueKind) -> bool {
    if index >= sig.len() || !is_param_kind_upgrade(kind) {
        return false;
    }
    let current = sig[index];
    if current == kind {
        return false;
    }
    if current == ValueKind::Int {
        sig[index] = kind;
        return true;
    }
    // Bare `.len` can mis-infer String before call-site ChildList evidence arrives.
    if current == ValueKind::String && kind == ValueKind::ChildList {
        sig[index] = kind;
        return true;
    }
    // Bare `param[key]` evidence is shared by StringList and JSON maps. Index
    // assignment upgrades (and JSON call sites) must win over the StringList
    // default so `states[evidence_key] = #{…}` stays native.
    if current == ValueKind::StringList && kind == ValueKind::Json {
        sig[index] = kind;
        return true;
    }
    false
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
                if apply_param_kind_upgrade(sig, index, kind) {
                    changed = true;
                }
            }
        }
    }
}

/// Upgrade callee params from typed call-site arguments (including `entry`).
/// Existing `propagate_local_fn_param_kinds` only lifts caller params from a
/// known callee signature; this is the reverse edge needed for helpers such as
/// `spec(program, arguments, …)` that place `arguments` into a JSON map without
/// StringList body evidence.
fn propagate_callee_param_kinds_from_call_sites(
    defs: &[&ScriptFuncDef],
    local_fn_sigs: &mut BTreeMap<String, Vec<ValueKind>>,
    base_ctx: &EmitCtx,
) -> bool {
    let mut any = false;
    let mut changed = true;
    while changed {
        changed = false;
        for def in defs {
            let mut ctx = base_ctx
                .clone()
                .with_local_fn_sigs(local_fn_sigs.clone())
                .with_empty_child_lists(discover_empty_child_list_bindings(&def.body, base_ctx));
            if let Some(sig) = local_fn_sigs.get(def.name.as_str()) {
                for (param, kind) in def.params.iter().zip(sig.iter().copied()) {
                    ctx = ctx.with_binding(param.as_str(), kind);
                }
            }
            for stmt in def.body.iter() {
                if apply_callee_param_upgrades_in_stmt(stmt, &mut ctx, local_fn_sigs) {
                    changed = true;
                    any = true;
                }
                infer_stmt_scope(stmt, &mut ctx);
            }
        }
    }
    any
}

fn apply_callee_param_upgrades_in_stmt(
    stmt: &Stmt,
    ctx: &mut EmitCtx,
    local_fn_sigs: &mut BTreeMap<String, Vec<ValueKind>>,
) -> bool {
    match stmt {
        Stmt::Expr(expr) | Stmt::Return(Some(expr), ..) => {
            apply_callee_param_upgrades_in_expr(expr.as_ref(), ctx, local_fn_sigs)
        }
        Stmt::Var(boxed, ..) => apply_callee_param_upgrades_in_expr(&boxed.1, ctx, local_fn_sigs),
        Stmt::Assignment(boxed, ..) => {
            let lhs = apply_callee_param_upgrades_in_expr(&boxed.1.lhs, ctx, local_fn_sigs);
            let rhs = apply_callee_param_upgrades_in_expr(&boxed.1.rhs, ctx, local_fn_sigs);
            lhs || rhs
        }
        Stmt::If(boxed, ..) => {
            let flow = boxed.as_ref();
            let mut changed = apply_callee_param_upgrades_in_expr(&flow.expr, ctx, local_fn_sigs);
            let mut body_ctx = ctx.clone();
            for inner in flow.body.iter() {
                changed |= apply_callee_param_upgrades_in_stmt(inner, &mut body_ctx, local_fn_sigs);
                infer_stmt_scope(inner, &mut body_ctx);
            }
            let mut branch_ctx = ctx.clone();
            for inner in flow.branch.iter() {
                changed |=
                    apply_callee_param_upgrades_in_stmt(inner, &mut branch_ctx, local_fn_sigs);
                infer_stmt_scope(inner, &mut branch_ctx);
            }
            changed
        }
        Stmt::For(boxed, ..) => {
            let (_, _, flow) = boxed.as_ref();
            let mut changed = apply_callee_param_upgrades_in_expr(&flow.expr, ctx, local_fn_sigs);
            let mut body_ctx = ctx.clone();
            for inner in flow.body.iter() {
                changed |= apply_callee_param_upgrades_in_stmt(inner, &mut body_ctx, local_fn_sigs);
                infer_stmt_scope(inner, &mut body_ctx);
            }
            changed
        }
        Stmt::While(boxed, ..) => {
            let flow = boxed.as_ref();
            let mut changed = apply_callee_param_upgrades_in_expr(&flow.expr, ctx, local_fn_sigs);
            let mut body_ctx = ctx.clone();
            for inner in flow.body.iter() {
                changed |= apply_callee_param_upgrades_in_stmt(inner, &mut body_ctx, local_fn_sigs);
                infer_stmt_scope(inner, &mut body_ctx);
            }
            changed
        }
        Stmt::Block(block) => {
            let mut changed = false;
            let mut body_ctx = ctx.clone();
            for inner in block.iter() {
                changed |= apply_callee_param_upgrades_in_stmt(inner, &mut body_ctx, local_fn_sigs);
                infer_stmt_scope(inner, &mut body_ctx);
            }
            changed
        }
        Stmt::TryCatch(boxed, ..) => {
            let flow = boxed.as_ref();
            let mut changed = false;
            let mut body_ctx = ctx.clone();
            for inner in flow.body.iter() {
                changed |= apply_callee_param_upgrades_in_stmt(inner, &mut body_ctx, local_fn_sigs);
                infer_stmt_scope(inner, &mut body_ctx);
            }
            let mut branch_ctx = ctx.clone();
            for inner in flow.branch.iter() {
                changed |=
                    apply_callee_param_upgrades_in_stmt(inner, &mut branch_ctx, local_fn_sigs);
                infer_stmt_scope(inner, &mut branch_ctx);
            }
            changed
        }
        Stmt::FnCall(call, ..) => apply_callee_param_upgrades_in_call(call, ctx, local_fn_sigs),
        _ => false,
    }
}

fn apply_callee_param_upgrades_in_expr(
    expr: &Expr,
    ctx: &EmitCtx,
    local_fn_sigs: &mut BTreeMap<String, Vec<ValueKind>>,
) -> bool {
    match expr {
        Expr::FnCall(call, ..) | Expr::MethodCall(call, ..) => {
            let mut changed = apply_callee_param_upgrades_in_call(call, ctx, local_fn_sigs);
            for arg in &call.args {
                changed |= apply_callee_param_upgrades_in_expr(arg, ctx, local_fn_sigs);
            }
            changed
        }
        Expr::Dot(boxed, ..) | Expr::Index(boxed, ..) => {
            let lhs = apply_callee_param_upgrades_in_expr(&boxed.lhs, ctx, local_fn_sigs);
            let rhs = apply_callee_param_upgrades_in_expr(&boxed.rhs, ctx, local_fn_sigs);
            lhs || rhs
        }
        Expr::Array(items, ..) => {
            let mut changed = false;
            for item in items {
                changed |= apply_callee_param_upgrades_in_expr(item, ctx, local_fn_sigs);
            }
            changed
        }
        Expr::Map(map, ..) => {
            let mut changed = false;
            for (_, value) in &map.0 {
                changed |= apply_callee_param_upgrades_in_expr(value, ctx, local_fn_sigs);
            }
            changed
        }
        Expr::And(args, ..) | Expr::Or(args, ..) => {
            let mut changed = false;
            for arg in args.iter() {
                changed |= apply_callee_param_upgrades_in_expr(arg, ctx, local_fn_sigs);
            }
            changed
        }
        _ => false,
    }
}

/// Call-site kinds used to upgrade callee params. Only stable producers count:
/// variables, array/map/string literals, and known JSON constructors. Dot-path
/// reads like `context.stress_included` infer as Json but are often intish
/// conditions — upgrading `q_require(condition, …)` from those breaks Native.
fn call_site_arg_kind_for_param_upgrade(arg: &Expr, ctx: &EmitCtx) -> Option<ValueKind> {
    let kind = match arg {
        // Variables: allow concrete containers / strings / handles. Do NOT
        // propagate bare Json — `let n = command_spec.timeout_ms` is Json in
        // scope but is an intish leaf; upgrading `spec(..., timeout_ms, …)`
        // from that poisons the timeout parameter.
        Expr::Variable(ident, ..) => match ctx.scope.get(ident.1.as_str()).copied() {
            Some(ValueKind::Set) => ValueKind::Json,
            Some(
                kind @ (ValueKind::String
                | ValueKind::Path
                | ValueKind::Command
                | ValueKind::Output
                | ValueKind::Child
                | ValueKind::ChildList
                | ValueKind::WindowControl
                | ValueKind::WindowRect
                | ValueKind::StringList
                | ValueKind::Stream
                | ValueKind::Bytes),
            ) => kind,
            _ if ctx.empty_child_lists.contains(ident.1.as_str()) => ValueKind::ChildList,
            _ => return None,
        },
        Expr::Array(..) | Expr::Map(..) | Expr::StringConstant(..) => infer_binding_kind(arg, ctx),
        Expr::FnCall(call, ..)
            if call_host_api_module(call) == Some("json")
                && (call.name == "parse" || call.name == "parse_file") =>
        {
            ValueKind::Json
        }
        Expr::FnCall(call, ..) if call.namespace.is_empty() => ctx
            .local_fn_return_kinds
            .get(call.name.as_str())
            .copied()
            .unwrap_or(ValueKind::Int),
        _ if json_path_array_index(arg, ctx).is_some()
            || json_path_key_get(arg, ctx).is_some()
            || json_array_index(arg, ctx).is_some()
            || json_rhai_array_index_property(arg, ctx).is_some() =>
        {
            ValueKind::Json
        }
        _ => return None,
    };
    // Empty `#{}` is Set in bindings but is a JSON object at API boundaries.
    let kind = match kind {
        ValueKind::Set => ValueKind::Json,
        other => other,
    };
    is_param_kind_upgrade(kind).then_some(kind)
}

fn apply_callee_param_upgrades_in_call(
    call: &rhai::FnCallExpr,
    ctx: &EmitCtx,
    local_fn_sigs: &mut BTreeMap<String, Vec<ValueKind>>,
) -> bool {
    if !call.namespace.is_empty() || !local_fn_sigs.contains_key(call.name.as_str()) {
        return false;
    }
    let mut upgrades = Vec::new();
    for (arg_index, arg) in call.args.iter().enumerate() {
        if let Some(kind) = call_site_arg_kind_for_param_upgrade(arg, ctx) {
            upgrades.push((arg_index, kind));
        }
    }
    let Some(sig) = local_fn_sigs.get_mut(call.name.as_str()) else {
        return false;
    };
    let mut changed = false;
    for (index, kind) in upgrades {
        if apply_param_kind_upgrade(sig, index, kind) {
            changed = true;
        }
    }
    changed
}

fn collect_param_kind_upgrades(
    def: &ScriptFuncDef,
    local_fn_sigs: &BTreeMap<String, Vec<ValueKind>>,
) -> Vec<(usize, ValueKind)> {
    let mut upgrades = Vec::new();
    let aliases = param_aliases_in_def(def);
    for stmt in def.body.iter() {
        collect_param_kind_upgrades_in_stmt(stmt, def, local_fn_sigs, &aliases, &mut upgrades);
    }
    upgrades
}

fn param_aliases_in_def(def: &ScriptFuncDef) -> BTreeMap<String, String> {
    let param_names: BTreeSet<&str> = def.params.iter().map(|param| param.as_str()).collect();
    let mut aliases = BTreeMap::new();
    for stmt in def.body.iter() {
        let Stmt::Var(boxed, ..) = stmt else {
            continue;
        };
        let (ident, expr, _) = boxed.as_ref();
        let Expr::Variable(source, ..) = expr else {
            continue;
        };
        if param_names.contains(source.1.as_str()) {
            aliases.insert(ident.name.to_string(), source.1.to_string());
        } else if let Some(param) = aliases.get(source.1.as_str()) {
            aliases.insert(ident.name.to_string(), param.clone());
        }
    }
    aliases
}

fn resolve_param_index(
    name: &str,
    def: &ScriptFuncDef,
    aliases: &BTreeMap<String, String>,
) -> Option<usize> {
    if let Some(index) = def.params.iter().position(|param| param.as_str() == name) {
        return Some(index);
    }
    aliases
        .get(name)
        .and_then(|param| def.params.iter().position(|candidate| candidate == param))
}

fn command_surface_param_upgrade(
    lhs: &Expr,
    rhs: &Expr,
    def: &ScriptFuncDef,
    aliases: &BTreeMap<String, String>,
) -> Option<(usize, ValueKind)> {
    let Expr::Variable(ident, ..) = lhs else {
        return None;
    };
    let param_index = resolve_param_index(ident.1.as_str(), def, aliases)?;
    if let Expr::MethodCall(call, ..) = rhs {
        return match call.name.as_str() {
            "stdin_text" | "stdin_bytes" | "arg" | "args" | "env" | "env_remove" | "timeout"
            | "capture_limit" | "current_dir" | "stdout_file" | "stderr_file"
            | "stderr_inherit" | "output" | "start" => Some((param_index, ValueKind::Command)),
            _ => None,
        };
    }
    match dot_property_name(rhs)? {
        "stdout" => Some((param_index, ValueKind::Child)),
        _ => None,
    }
}

fn collect_param_kind_upgrades_in_stmt(
    stmt: &Stmt,
    def: &ScriptFuncDef,
    local_fn_sigs: &BTreeMap<String, Vec<ValueKind>>,
    aliases: &BTreeMap<String, String>,
    upgrades: &mut Vec<(usize, ValueKind)>,
) {
    match stmt {
        Stmt::Expr(expr) | Stmt::Return(Some(expr), ..) => {
            collect_param_kind_upgrades_in_expr(
                expr.as_ref(),
                def,
                local_fn_sigs,
                aliases,
                upgrades,
            );
        }
        Stmt::Var(boxed, ..) => {
            collect_param_kind_upgrades_in_expr(&boxed.1, def, local_fn_sigs, aliases, upgrades);
        }
        Stmt::Assignment(boxed, ..) => {
            if let Expr::Index(index_box, ..) = &boxed.1.lhs
                && let Expr::Variable(ident, ..) = &index_box.lhs
                && let Some(param_index) = def
                    .params
                    .iter()
                    .position(|param| param.as_str() == ident.1.as_str())
            {
                let probe = EmitCtx::new(true);
                let kind = if ident.1.as_str() == "args"
                    || matches!(boxed.1.rhs, Expr::StringConstant(..))
                    || is_explicit_string_expr(&boxed.1.rhs, &probe)
                {
                    ValueKind::StringList
                } else {
                    ValueKind::Json
                };
                upgrades.push((param_index, kind));
            }
            collect_param_kind_upgrades_in_expr(
                &boxed.1.lhs,
                def,
                local_fn_sigs,
                aliases,
                upgrades,
            );
            collect_param_kind_upgrades_in_expr(
                &boxed.1.rhs,
                def,
                local_fn_sigs,
                aliases,
                upgrades,
            );
        }
        Stmt::If(boxed, ..) => {
            let flow = boxed.as_ref();
            collect_param_kind_upgrades_in_expr(&flow.expr, def, local_fn_sigs, aliases, upgrades);
            for stmt in flow.body.iter() {
                collect_param_kind_upgrades_in_stmt(stmt, def, local_fn_sigs, aliases, upgrades);
            }
            for stmt in flow.branch.iter() {
                collect_param_kind_upgrades_in_stmt(stmt, def, local_fn_sigs, aliases, upgrades);
            }
        }
        Stmt::For(boxed, ..) => {
            let (_, _, flow) = boxed.as_ref();
            collect_param_kind_upgrades_in_expr(&flow.expr, def, local_fn_sigs, aliases, upgrades);
            for stmt in flow.body.iter() {
                collect_param_kind_upgrades_in_stmt(stmt, def, local_fn_sigs, aliases, upgrades);
            }
        }
        Stmt::While(boxed, ..) => {
            let flow = boxed.as_ref();
            collect_param_kind_upgrades_in_expr(&flow.expr, def, local_fn_sigs, aliases, upgrades);
            for stmt in flow.body.iter() {
                collect_param_kind_upgrades_in_stmt(stmt, def, local_fn_sigs, aliases, upgrades);
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
    aliases: &BTreeMap<String, String>,
    upgrades: &mut Vec<(usize, ValueKind)>,
) {
    match expr {
        Expr::FnCall(call, ..) | Expr::MethodCall(call, ..) => {
            if call.name == "args"
                && call.args.len() == 1
                && let Expr::Variable(ident, ..) = &call.args[0]
                && let Some(param_index) = resolve_param_index(ident.1.as_str(), def, aliases)
            {
                upgrades.push((param_index, ValueKind::StringList));
            }
            if let Some(upgrade) = process_command_argv_param_upgrade(call, def) {
                upgrades.push(upgrade);
            }
            collect_param_kind_upgrades_in_call(call, def, local_fn_sigs, upgrades);
            for arg in &call.args {
                collect_param_kind_upgrades_in_expr(arg, def, local_fn_sigs, aliases, upgrades);
            }
        }
        Expr::Dot(boxed, ..) | Expr::Index(boxed, ..) => {
            if let Expr::Dot(boxed, ..) = expr
                && let Some(upgrade) =
                    command_surface_param_upgrade(&boxed.lhs, &boxed.rhs, def, aliases)
            {
                upgrades.push(upgrade);
            }
            collect_param_kind_upgrades_in_expr(&boxed.lhs, def, local_fn_sigs, aliases, upgrades);
            collect_param_kind_upgrades_in_expr(&boxed.rhs, def, local_fn_sigs, aliases, upgrades);
        }
        Expr::Array(items, ..) => {
            for item in items {
                collect_param_kind_upgrades_in_expr(item, def, local_fn_sigs, aliases, upgrades);
            }
        }
        Expr::Map(map, ..) => {
            for (_, value) in &map.0 {
                collect_param_kind_upgrades_in_expr(value, def, local_fn_sigs, aliases, upgrades);
            }
        }
        Expr::And(args, ..) | Expr::Or(args, ..) => {
            for arg in args.iter() {
                collect_param_kind_upgrades_in_expr(arg, def, local_fn_sigs, aliases, upgrades);
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
        if is_param_kind_upgrade(kind) {
            upgrades.push((param_index, kind));
        }
    }
}

fn for_body_uses_char_iteration(counter: &str, body: &StmtBlock) -> bool {
    body.iter()
        .any(|stmt| stmt_uses_char_iteration_counter(stmt, counter))
}

fn stmt_uses_char_iteration_counter(stmt: &Stmt, counter: &str) -> bool {
    match stmt {
        Stmt::Expr(expr) | Stmt::Return(Some(expr), ..) => {
            expr_uses_char_iteration_counter(expr.as_ref(), counter)
        }
        Stmt::Var(boxed, ..) => expr_uses_char_iteration_counter(&boxed.1, counter),
        Stmt::Assignment(boxed, ..) => {
            expr_uses_char_iteration_counter(&boxed.1.lhs, counter)
                || expr_uses_char_iteration_counter(&boxed.1.rhs, counter)
        }
        Stmt::If(boxed, ..) => {
            let flow = boxed.as_ref();
            expr_uses_char_iteration_counter(&flow.expr, counter)
                || block_uses_char_iteration_counter(&flow.body, counter)
                || block_uses_char_iteration_counter(&flow.branch, counter)
        }
        Stmt::For(boxed, ..) => {
            let (_, _, flow) = boxed.as_ref();
            block_uses_char_iteration_counter(&flow.body, counter)
        }
        Stmt::While(boxed, ..) => {
            let flow = boxed.as_ref();
            block_uses_char_iteration_counter(&flow.body, counter)
        }
        _ => false,
    }
}

fn block_uses_char_iteration_counter(body: &StmtBlock, counter: &str) -> bool {
    body.iter()
        .any(|stmt| stmt_uses_char_iteration_counter(stmt, counter))
}

fn expr_uses_char_iteration_counter(expr: &Expr, counter: &str) -> bool {
    if let Expr::Dot(boxed, ..) = expr
        && is_param_var(&boxed.lhs, counter)
        && matches!(
            &boxed.rhs,
            Expr::MethodCall(call, ..) if call.name == "to_string" && call.args.is_empty()
        )
    {
        return true;
    }
    match expr {
        Expr::FnCall(call, ..) | Expr::MethodCall(call, ..) => call
            .args
            .iter()
            .any(|arg| expr_uses_char_iteration_counter(arg, counter)),
        Expr::Dot(boxed, ..) | Expr::Index(boxed, ..) => {
            expr_uses_char_iteration_counter(&boxed.lhs, counter)
                || expr_uses_char_iteration_counter(&boxed.rhs, counter)
        }
        _ => false,
    }
}

fn param_used_as_json_array_for(def: &ScriptFuncDef, param: &str) -> bool {
    def.body.iter().any(|stmt| {
        let Stmt::For(boxed, ..) = stmt else {
            return false;
        };
        let (counter, _, flow) = boxed.as_ref();
        matches!(&flow.expr, Expr::Variable(ident, ..) if ident.1.as_str() == param)
            && !for_body_uses_char_iteration(counter.name.as_str(), &flow.body)
    })
}

fn param_used_as_json(def: &ScriptFuncDef, param: &str, _ctx: &EmitCtx) -> bool {
    param_used_as_json_array_for(def, param)
        || def
            .body
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
            | "sub_string"
            | "len"
    )
}

fn is_json_method_name(name: &str) -> bool {
    matches!(name, "push" | "insert" | "get")
}

fn is_output_member_name(name: &str) -> bool {
    matches!(
        name,
        "success"
            | "exit_code"
            | "stdout"
            | "stderr"
            | "stdout_text"
            | "stderr_text"
            | "require_success"
    )
}

fn is_child_member_name(name: &str) -> bool {
    matches!(
        name,
        "id" | "state"
            | "platform_facts"
            | "stdout"
            | "stderr"
            | "kill"
            | "wait_with_output"
            | "window_key"
            | "window_control"
            | "window_message"
            | "window_pointer"
            | "window_resize"
            | "window_rect"
            | "window_client_rect"
    )
}

fn is_definite_child_member_name(name: &str) -> bool {
    matches!(
        name,
        "platform_facts"
            | "stdout"
            | "stderr"
            | "kill"
            | "wait_with_output"
            | "window_key"
            | "window_control"
            | "window_message"
            | "window_pointer"
            | "window_resize"
            | "window_rect"
            | "window_client_rect"
    )
}

fn is_stream_member_name(name: &str) -> bool {
    name == "read"
}

fn is_bytes_member_name(name: &str) -> bool {
    name == "to_text"
}

fn is_command_member_name(name: &str) -> bool {
    matches!(
        name,
        "arg"
            | "args"
            | "env"
            | "env_remove"
            | "stdin_text"
            | "stdin_bytes"
            | "timeout"
            | "capture_limit"
            | "current_dir"
            | "stdout_file"
            | "stderr_file"
            | "stderr_inherit"
            | "output"
            | "start"
    )
}

fn is_process_member_name(name: &str) -> bool {
    is_output_member_name(name) || is_child_member_name(name) || is_command_member_name(name)
}

fn rust_param_type(kind: ValueKind) -> &'static str {
    match kind {
        ValueKind::String | ValueKind::Path => "String",
        ValueKind::Json => "serde_json::Value",
        ValueKind::Command => "RhCommand",
        ValueKind::Output => "RhOutput",
        ValueKind::Child => "RhChild",
        ValueKind::ChildList => "Vec<RhChild>",
        ValueKind::WindowControl => "RhWindowControl",
        ValueKind::WindowRect => "RhWindowRect",
        ValueKind::Stream => "RhStream",
        ValueKind::Bytes => "RhBytes",
        ValueKind::StringList => "Vec<String>",
        ValueKind::Set => "std::collections::HashSet<String>",
        _ => "INT",
    }
}

fn expr_uses_process_param(expr: &Expr, param: &str, member: fn(&str) -> bool) -> bool {
    let Expr::Dot(boxed, ..) = expr else {
        return false;
    };
    if !is_param_var(&boxed.lhs, param) {
        return false;
    }
    match &boxed.rhs {
        Expr::Property(property, ..) => member(property.2.as_str()),
        Expr::MethodCall(call, ..) => member(call.name.as_str()),
        Expr::Dot(..) => {
            let mut path = Vec::new();
            append_json_properties(&boxed.rhs, &mut path)
                && path.first().is_some_and(|name| member(name))
        }
        _ => false,
    }
}

fn param_used_as_output(def: &ScriptFuncDef, param: &str) -> bool {
    def.body
        .iter()
        .any(|stmt| stmt_uses_param_with_member(stmt, param, is_output_member_name))
}

fn param_used_as_stream(def: &ScriptFuncDef, param: &str) -> bool {
    def.body
        .iter()
        .any(|stmt| stmt_uses_param_with_member(stmt, param, is_stream_member_name))
}

fn param_used_as_bytes(def: &ScriptFuncDef, param: &str) -> bool {
    def.body
        .iter()
        .any(|stmt| stmt_uses_param_with_member(stmt, param, is_bytes_member_name))
}

fn param_used_as_child(def: &ScriptFuncDef, param: &str) -> bool {
    def.body
        .iter()
        .any(|stmt| stmt_uses_param_with_member(stmt, param, is_child_member_name))
}

fn param_used_as_definite_child(def: &ScriptFuncDef, param: &str) -> bool {
    def.body
        .iter()
        .any(|stmt| stmt_uses_param_with_member(stmt, param, is_definite_child_member_name))
}

fn param_used_as_command(def: &ScriptFuncDef, param: &str) -> bool {
    def.body
        .iter()
        .any(|stmt| stmt_uses_param_with_member(stmt, param, is_command_member_name))
}

fn stmt_uses_param_with_member(stmt: &Stmt, param: &str, member: fn(&str) -> bool) -> bool {
    match stmt {
        Stmt::Expr(expr) | Stmt::Return(Some(expr), ..) => {
            expr_uses_process_param(expr.as_ref(), param, member)
                || expr_mentions_param(expr.as_ref(), param, member)
        }
        Stmt::Var(boxed, ..) => expr_mentions_param(&boxed.1, param, member),
        Stmt::Assignment(boxed, ..) => {
            expr_mentions_param(&boxed.1.lhs, param, member)
                || expr_mentions_param(&boxed.1.rhs, param, member)
        }
        Stmt::If(boxed, ..) => {
            let flow = boxed.as_ref();
            expr_mentions_param(&flow.expr, param, member)
                || flow
                    .body
                    .iter()
                    .any(|stmt| stmt_uses_param_with_member(stmt, param, member))
                || flow
                    .branch
                    .iter()
                    .any(|stmt| stmt_uses_param_with_member(stmt, param, member))
        }
        Stmt::For(boxed, ..) => {
            let (_, _, flow) = boxed.as_ref();
            expr_mentions_param(&flow.expr, param, member)
                || flow
                    .body
                    .iter()
                    .any(|stmt| stmt_uses_param_with_member(stmt, param, member))
        }
        Stmt::While(boxed, ..) => {
            let flow = boxed.as_ref();
            expr_mentions_param(&flow.expr, param, member)
                || flow
                    .body
                    .iter()
                    .any(|stmt| stmt_uses_param_with_member(stmt, param, member))
        }
        Stmt::Block(block) => block
            .iter()
            .any(|stmt| stmt_uses_param_with_member(stmt, param, member)),
        Stmt::TryCatch(boxed, ..) => {
            let flow = boxed.as_ref();
            flow.body
                .iter()
                .any(|stmt| stmt_uses_param_with_member(stmt, param, member))
                || flow
                    .branch
                    .iter()
                    .any(|stmt| stmt_uses_param_with_member(stmt, param, member))
        }
        Stmt::FnCall(call, ..) => call
            .args
            .iter()
            .any(|arg| expr_mentions_param(arg, param, member)),
        _ => false,
    }
}

fn expr_mentions_param(expr: &Expr, param: &str, member: fn(&str) -> bool) -> bool {
    match expr {
        Expr::FnCall(call, ..) | Expr::MethodCall(call, ..) => call
            .args
            .iter()
            .any(|arg| expr_uses_process_param(arg, param, member)),
        Expr::Dot(boxed, ..) | Expr::Index(boxed, ..) => {
            expr_uses_process_param(expr, param, member)
                || expr_mentions_param(&boxed.lhs, param, member)
                || expr_mentions_param(&boxed.rhs, param, member)
        }
        Expr::Array(items, ..) => items
            .iter()
            .any(|item| expr_mentions_param(item, param, member)),
        Expr::Map(map, ..) => map
            .0
            .iter()
            .any(|(_, value)| expr_mentions_param(value, param, member)),
        _ => false,
    }
}

fn child_aliases_in_block(counter: &str, body: &StmtBlock) -> BTreeSet<String> {
    let mut aliases = BTreeSet::from([counter.to_string()]);
    for stmt in body.iter() {
        if let Stmt::Var(boxed, ..) = stmt
            && let Expr::Variable(ident, ..) = &boxed.1
            && aliases.contains(ident.1.as_str())
        {
            aliases.insert(boxed.0.name.to_string());
        }
    }
    aliases
}

fn block_uses_child_vars(body: &StmtBlock, vars: &BTreeSet<String>) -> bool {
    body.iter().any(|stmt| stmt_uses_child_binding(stmt, vars))
}

fn stmt_uses_child_binding(stmt: &Stmt, vars: &BTreeSet<String>) -> bool {
    match stmt {
        Stmt::Expr(expr) | Stmt::Return(Some(expr), ..) => {
            expr_uses_child_binding(expr.as_ref(), vars)
        }
        Stmt::Var(boxed, ..) => expr_uses_child_binding(&boxed.1, vars),
        Stmt::Assignment(boxed, ..) => {
            expr_uses_child_binding(&boxed.1.lhs, vars)
                || expr_uses_child_binding(&boxed.1.rhs, vars)
        }
        Stmt::If(boxed, ..) => {
            let flow = boxed.as_ref();
            expr_uses_child_binding(&flow.expr, vars)
                || block_uses_child_vars(&flow.body, vars)
                || block_uses_child_vars(&flow.branch, vars)
        }
        Stmt::For(boxed, ..) => {
            let (_, _, flow) = boxed.as_ref();
            block_uses_child_vars(&flow.body, vars)
        }
        Stmt::While(boxed, ..) => {
            let flow = boxed.as_ref();
            block_uses_child_vars(&flow.body, vars)
        }
        _ => false,
    }
}

fn expr_uses_child_binding(expr: &Expr, vars: &BTreeSet<String>) -> bool {
    for var in vars {
        if expr_uses_process_param(expr, var, is_definite_child_member_name) {
            return true;
        }
    }
    match expr {
        Expr::FnCall(call, ..) | Expr::MethodCall(call, ..) => call
            .args
            .iter()
            .any(|arg| expr_uses_child_binding(arg, vars)),
        Expr::Dot(boxed, ..) | Expr::Index(boxed, ..) => {
            expr_uses_child_binding(&boxed.lhs, vars) || expr_uses_child_binding(&boxed.rhs, vars)
        }
        _ => false,
    }
}

fn index_rhs_is_list_index(rhs: &Expr) -> bool {
    match rhs {
        Expr::IntegerConstant(..) | Expr::BoolConstant(..) => true,
        Expr::Variable(ident, ..) => {
            let name = ident.1.as_str();
            // Common int counters (`gate_index`, `i`, …). Map-key locals such as
            // `profile_key` / `name` must stay JSON evidence instead.
            name.contains("index")
                || name.ends_with("_i")
                || matches!(name, "i" | "j" | "k" | "n" | "idx" | "offset" | "pos")
        }
        _ => false,
    }
}

fn param_used_as_string_list(def: &ScriptFuncDef, param: &str) -> bool {
    def.body
        .iter()
        .any(|stmt| stmt_uses_string_list_param(stmt, param))
}

fn stmt_uses_string_list_param(stmt: &Stmt, param: &str) -> bool {
    match stmt {
        Stmt::Expr(expr) | Stmt::Return(Some(expr), ..) => {
            expr_uses_string_list_param(expr.as_ref(), param)
        }
        Stmt::Var(boxed, ..) => expr_uses_string_list_param(&boxed.1, param),
        Stmt::Assignment(boxed, ..) => {
            expr_uses_string_list_param(&boxed.1.lhs, param)
                || expr_uses_string_list_param(&boxed.1.rhs, param)
        }
        Stmt::If(boxed, ..) => {
            let flow = boxed.as_ref();
            expr_uses_string_list_param(&flow.expr, param)
                || flow
                    .body
                    .iter()
                    .any(|stmt| stmt_uses_string_list_param(stmt, param))
                || flow
                    .branch
                    .iter()
                    .any(|stmt| stmt_uses_string_list_param(stmt, param))
        }
        Stmt::For(boxed, ..) => {
            let (_, _, flow) = boxed.as_ref();
            expr_uses_string_list_param(&flow.expr, param)
                || flow
                    .body
                    .iter()
                    .any(|stmt| stmt_uses_string_list_param(stmt, param))
        }
        Stmt::While(boxed, ..) => {
            let flow = boxed.as_ref();
            expr_uses_string_list_param(&flow.expr, param)
                || flow
                    .body
                    .iter()
                    .any(|stmt| stmt_uses_string_list_param(stmt, param))
        }
        Stmt::Block(block) => block
            .iter()
            .any(|stmt| stmt_uses_string_list_param(stmt, param)),
        Stmt::TryCatch(boxed, ..) => {
            let flow = boxed.as_ref();
            flow.body
                .iter()
                .any(|stmt| stmt_uses_string_list_param(stmt, param))
                || flow
                    .branch
                    .iter()
                    .any(|stmt| stmt_uses_string_list_param(stmt, param))
        }
        Stmt::FnCall(call, ..) => call
            .args
            .iter()
            .any(|arg| expr_uses_string_list_param(arg, param)),
        _ => false,
    }
}

fn expr_uses_string_list_param(expr: &Expr, param: &str) -> bool {
    if let Expr::Dot(boxed, ..) = expr
        && let Expr::MethodCall(call, ..) = &boxed.rhs
    {
        if call.name == "args" && call.args.len() == 1 && is_param_var(&call.args[0], param) {
            return true;
        }
        if call.name == "push" && is_param_var(&boxed.lhs, param) {
            return true;
        }
    }
    if let Expr::FnCall(call, ..) = expr
        && let Some(argv_index) = process_command_argv_arg_index(call)
        && is_param_var(&call.args[argv_index], param)
    {
        return true;
    }
    if let Expr::Index(boxed, ..) = expr
        && is_param_var(&boxed.lhs, param)
    {
        // Only list-style indexes count as StringList evidence. String keys
        // (`emitted[profile_key]` / `emitted["k"]`) are JSON map lookups and must
        // not win over Json param inference (StringList is checked before Json).
        return index_rhs_is_list_index(&boxed.rhs);
    }
    match expr {
        Expr::FnCall(call, ..) | Expr::MethodCall(call, ..) => call
            .args
            .iter()
            .any(|arg| expr_uses_string_list_param(arg, param)),
        Expr::Dot(boxed, ..) | Expr::Index(boxed, ..) => {
            expr_uses_string_list_param(&boxed.lhs, param)
                || expr_uses_string_list_param(&boxed.rhs, param)
        }
        _ => false,
    }
}

fn param_used_as_child_list(def: &ScriptFuncDef, param: &str) -> bool {
    // Only definite Child iteration proves ChildList. Bare `.len` / indexing is
    // shared with StringList and must not win the param-kind race.
    def.body.iter().any(|stmt| {
        let Stmt::For(boxed, ..) = stmt else {
            return false;
        };
        let (counter, _, flow) = boxed.as_ref();
        matches!(&flow.expr, Expr::Variable(ident, ..) if ident.1.as_str() == param)
            && block_uses_child_vars(
                &flow.body,
                &child_aliases_in_block(counter.name.as_str(), &flow.body),
            )
    })
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
            if ((call_host_api_module(call) == Some("json")
                && (call.name == "stringify_pretty" || call.name == "stringify"))
                || (call.namespace.is_empty()
                    && (call.name == "stringify_pretty" || call.name == "stringify")))
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
        Expr::And(args, ..) | Expr::Or(args, ..) => {
            args.iter().any(|arg| expr_uses_json_param(arg, param))
        }
        Expr::Dot(boxed, ..) => {
            if is_param_var(&boxed.lhs, param) {
                match &boxed.rhs {
                    // Rhai parses `param.field[index]` as Dot(param, Index(Property, …)).
                    Expr::Index(index_box, ..) if matches!(&index_box.lhs, Expr::Property(..)) => {
                        true
                    }
                    // `param.len` is shared by strings/arrays — not JSON-only.
                    Expr::Property(property, ..) if property.2.as_str() == "len" => false,
                    Expr::Property(property, ..) => !is_process_member_name(property.2.as_str()),
                    Expr::MethodCall(call, ..) if is_json_method_name(call.name.as_str()) => true,
                    Expr::MethodCall(call, ..) if is_process_member_name(call.name.as_str()) => {
                        false
                    }
                    Expr::MethodCall(call, ..) if is_stringish_method_name(call.name.as_str()) => {
                        // Bare `param.contains` / `param.split` are string surfaces.
                        call.args.iter().any(|arg| expr_uses_json_param(arg, param))
                    }
                    // Rhai may group `param.a.b` / `param.a.b.contains(x)` as
                    // `param . (Property(a) . …)`. Nested rhs under a param root is
                    // JSON path evidence (append_json_properties alone misses a
                    // trailing MethodCall such as contains/push).
                    Expr::Dot(..) => true,
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
        // Rhai lowers `throw message` to Return+BREAK; a thrown param is a string.
        Stmt::Return(Some(expr), flags, ..) if flags.contains(ASTFlags::BREAK) => {
            is_param_var(expr.as_ref(), param) || expr_uses_string_param(expr.as_ref(), param)
        }
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
            let (counter, _, flow) = boxed.as_ref();
            (matches!(&flow.expr, Expr::Variable(ident, ..) if ident.1.as_str() == param)
                && for_body_uses_char_iteration(counter.name.as_str(), &flow.body))
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
        || std_fs_remove_dir_all_arg(expr).is_some_and(|path| is_param_var(path, param))
        || std_fs_rename_args(expr)
            .is_some_and(|(src, dst)| is_param_var(src, param) || is_param_var(dst, param))
        || std_fs_try_rename_args(expr)
            .is_some_and(|(src, dst)| is_param_var(src, param) || is_param_var(dst, param))
        || std_fs_symlink_metadata_arg(expr).is_some_and(|path| is_param_var(path, param))
        || std_fs_metadata_arg(expr).is_some_and(|path| is_param_var(path, param))
        || path_join_display_args(expr)
            .is_some_and(|(base, child)| is_param_var(base, param) || is_param_var(child, param))
        || path_parent_display_arg(expr).is_some_and(|path| is_param_var(path, param))
        || path_buf_from_arg(expr).is_some_and(|path| is_param_var(path, param))
        || path_buf_from_display_arg(expr).is_some_and(|path| is_param_var(path, param))
        || path_buf_from_is_absolute_arg(expr).is_some_and(|path| is_param_var(path, param))
        || json_parse_file_arg(expr).is_some_and(|path| is_param_var(path, param))
        || rh_fail_arg(expr).is_some_and(|message| is_param_var(message, param))
        || std_env_get_arg(expr).is_some_and(|name| is_param_var(name, param))
        || std_env_has_arg(expr).is_some_and(|name| is_param_var(name, param))
    {
        return true;
    }
    if let Expr::Dot(boxed, ..) = expr
        && is_param_var(&boxed.lhs, param)
        && matches!(
            &boxed.rhs,
            Expr::Property(property, ..) if property.2.as_str() == "is_absolute"
        )
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
        Expr::Array(items, ..) => items.iter().any(|item| expr_uses_string_param(item, param)),
        Expr::Map(map, ..) => map
            .0
            .iter()
            .any(|(_, value)| expr_uses_string_param(value, param)),
        _ => false,
    }
}

fn is_param_var(expr: &Expr, param: &str) -> bool {
    matches!(expr, Expr::Variable(ident, ..) if ident.1.as_str() == param)
}

fn param_assigned_in_body(def: &ScriptFuncDef, param: &str) -> bool {
    def.body.iter().any(|stmt| stmt_assigns_param(stmt, param))
}

fn stmt_assigns_param(stmt: &Stmt, param: &str) -> bool {
    match stmt {
        Stmt::Assignment(boxed, ..) => is_param_var(&boxed.1.lhs, param),
        Stmt::If(boxed, ..) => {
            let flow = boxed.as_ref();
            flow.body
                .iter()
                .chain(flow.branch.iter())
                .any(|inner| stmt_assigns_param(inner, param))
        }
        Stmt::For(boxed, ..) => {
            let (_, _, flow) = boxed.as_ref();
            flow.body
                .iter()
                .any(|inner| stmt_assigns_param(inner, param))
        }
        Stmt::While(boxed, ..) => {
            let flow = boxed.as_ref();
            flow.body
                .iter()
                .any(|inner| stmt_assigns_param(inner, param))
        }
        Stmt::Block(block) => block.iter().any(|inner| stmt_assigns_param(inner, param)),
        Stmt::TryCatch(boxed, ..) => {
            let flow = boxed.as_ref();
            flow.body
                .iter()
                .chain(flow.branch.iter())
                .any(|inner| stmt_assigns_param(inner, param))
        }
        _ => false,
    }
}

fn json_string_field_path<'a>(expr: &'a Expr, ctx: &'a EmitCtx) -> Option<(&'a str, Vec<&'a str>)> {
    let (binding, path) = json_value_path(expr, ctx)?;
    if path.is_empty() {
        return None;
    }
    matches!(
        path.last().copied(),
        Some(
            "stdout"
                | "stderr"
                | "state"
                | "text"
                | "executable_name"
                | "file_name"
                | "path"
                | "status"
                | "entry"
                | "evidence"
                | "stable_id"
                | "module"
                | "default_profile"
                | "execution_model"
                | "job_object"
                | "ambient_authority"
                | "run_id"
                | "address"
                | "run_directory"
                | "failure_directory"
                | "project_id"
        )
    )
    .then_some((binding, path))
}

fn string_list_compare_pair<'a>(
    lhs: &'a Expr,
    rhs: &'a Expr,
    ctx: &EmitCtx,
) -> Option<(&'a str, &'a Expr)> {
    let Expr::Variable(ident, ..) = lhs else {
        return None;
    };
    if ctx.scope.get(ident.1.as_str()).copied() != Some(ValueKind::StringList) {
        return None;
    }
    Some((ident.1.as_str(), rhs))
}

fn stmt_always_returns(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Return(..) => true,
        Stmt::BreakLoop(_, flags, ..) if flags.contains(ASTFlags::BREAK) => true,
        Stmt::FnCall(call, ..) if call.name == "throw" => true,
        _ => false,
    }
}

fn emit_return_expr(out: &mut String, expr: &Expr, ctx: &mut EmitCtx) -> Result<(), RhError> {
    match ctx.current_return_kind {
        ValueKind::StringList => {
            if let Expr::Array(items, ..) = expr
                && !items.is_empty()
                && items.iter().all(|item| is_stringish_array_item(item, ctx))
            {
                out.push_str("vec![");
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        out.push_str(", ");
                    }
                    if !emit_owned_string_element(out, item, ctx)? {
                        return Err(RhError::Transpile(
                            "string list return items must be stringish".into(),
                        ));
                    }
                }
                out.push(']');
                return Ok(());
            }
            if let Expr::Variable(ident, ..) = expr
                && ctx.scope.get(ident.1.as_str()).copied() == Some(ValueKind::StringList)
            {
                out.push_str(ident.1.as_str());
                return Ok(());
            }
        }
        ValueKind::String | ValueKind::Path => {
            emit_stringish(out, expr, ctx)?;
            return Ok(());
        }
        ValueKind::Json => {
            // Child/WindowControl property reads (e.g. `child.platform_facts`) are
            // host calls, not JSON literals; route through emit_expr.
            if matches!(expr, Expr::Dot(..)) && emit_child_property(out, expr, ctx)? {
                return Ok(());
            }
            emit_json_value_expr(out, expr, ctx)?;
            return Ok(());
        }
        ValueKind::Int | ValueKind::Bool => {
            if let Expr::Variable(ident, ..) = expr
                && ctx.scope.get(ident.1.as_str()).copied() == Some(ValueKind::Json)
            {
                out.push_str("rh_json_as_i64(&");
                out.push_str(ident.1.as_str());
                out.push(')');
                return Ok(());
            }
            emit_intish(out, expr, ctx)?;
            return Ok(());
        }
        ValueKind::Output => {
            if let Some(binding) = child_wait_with_output_call(expr, ctx) {
                out.push_str("rh_child_wait_with_output(&mut ");
                out.push_str(ctx.resolve_binding(binding));
                out.push_str(", 0)");
                return Ok(());
            }
        }
        _ => {}
    }
    emit_expr(out, expr, ctx)
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
    if implicit_return
        && ctx.cdylib
        && ctx.current_return_kind == ValueKind::Int
        && !stmts.is_empty()
        && !stmt_always_returns(stmts.last().expect("non-empty"))
    {
        out.push_str("    return 0;\n");
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
            let mut kind = infer_binding_kind(expr, ctx);
            if kind == ValueKind::Set && ctx.set_map_bindings.contains(ident.name.as_str()) {
                kind = ValueKind::Json;
            }
            if kind == ValueKind::Json
                && matches!(expr, Expr::Array(items, ..) if items.is_empty())
                && ctx.empty_child_lists.contains(ident.name.as_str())
            {
                kind = ValueKind::ChildList;
            }
            if kind == ValueKind::Json
                && matches!(expr, Expr::Array(items, ..) if items.is_empty())
                && ctx.empty_string_lists.contains(ident.name.as_str())
            {
                kind = ValueKind::StringList;
            }
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
            } else if kind == ValueKind::ChildList
                && matches!(expr, Expr::Array(items, ..) if items.is_empty())
            {
                out.push_str("Vec::new()");
            } else if kind == ValueKind::ChildList
                && let Expr::Array(items, ..) = expr
            {
                out.push_str("vec![");
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        out.push_str(", ");
                    }
                    let Expr::Variable(child_ident, ..) = item else {
                        return Err(RhError::Transpile(
                            "child list literal items must be Child bindings".into(),
                        ));
                    };
                    out.push_str("rh_child_share(&mut ");
                    out.push_str(child_ident.1.as_str());
                    out.push(')');
                }
                out.push(']');
            } else if kind == ValueKind::Json
                && matches!(expr, Expr::Array(items, ..) if items.is_empty())
            {
                out.push_str("serde_json::Value::Array(Vec::new())");
            } else if kind == ValueKind::Json
                && let Expr::Array(items, ..) = expr
                && !items.is_empty()
                && items
                    .iter()
                    .all(|item| is_native_json_value_item(item, ctx))
            {
                emit_json_array_value_literal(out, items, ctx)?;
            } else if kind == ValueKind::Json && matches!(expr, Expr::Map(..)) {
                emit_json_map_literal(out, expr, ctx)?;
            } else if kind == ValueKind::Json
                && matches!(
                    expr,
                    Expr::Variable(ident, ..)
                        if ctx.scope.get(ident.1.as_str()).copied() == Some(ValueKind::Json)
                )
            {
                // `let smoke_context = context` must clone Json Values.
                if let Expr::Variable(ident, ..) = expr {
                    out.push_str(ident.1.as_str());
                    out.push_str(".clone()");
                }
            } else if kind == ValueKind::StringList {
                if let Some((receiver, json, separator)) = string_split_parts(expr, ctx) {
                    emit_string_split_call(out, receiver, json, separator, ctx)?;
                } else if let Expr::Array(items, ..) = expr {
                    out.push_str("vec![");
                    for (index, item) in items.iter().enumerate() {
                        if index > 0 {
                            out.push_str(", ");
                        }
                        // Owned Vec<String> elements must clone bindings; bare
                        // `emit_stringish` would move String variables into the vec.
                        if !emit_owned_string_element(out, item, ctx)? {
                            return Err(RhError::Transpile(
                                "string list literal items must be stringish values".into(),
                            ));
                        }
                    }
                    out.push(']');
                } else if let Expr::Variable(ident, ..) = expr
                    && ctx.scope.get(ident.1.as_str()).copied() == Some(ValueKind::StringList)
                {
                    out.push_str(ident.1.as_str());
                    out.push_str(".clone()");
                } else if emit_string_list_producing_call(out, expr, ctx)? {
                    // `let args = evidence_list_arguments(...)` / local StringList helpers.
                } else {
                    return Err(RhError::Transpile(
                        "string list binding requires .split(\"…\"), a string array literal, \
                         or a StringList-returning local call"
                            .into(),
                    ));
                }
            } else if kind == ValueKind::Set {
                out.push_str("std::collections::HashSet::<String>::new()");
            } else if matches!(kind, ValueKind::String | ValueKind::Path)
                && matches!(expr, Expr::StringConstant(..))
            {
                out.push_str("String::from(");
                emit_native_string(out, expr, ctx)?;
                out.push(')');
            } else if matches!(kind, ValueKind::String | ValueKind::Path)
                && matches!(
                    expr,
                    Expr::Variable(ident, ..)
                        if matches!(
                            ctx.scope.get(ident.1.as_str()).copied(),
                            Some(ValueKind::String | ValueKind::Path)
                        )
                )
            {
                emit_expr(out, expr, ctx)?;
                out.push_str(".clone()");
            } else if matches!(kind, ValueKind::String | ValueKind::Path) {
                // Concat / JSON string fields / path displays — never fall through to
                // emit_expr's INT Plus lane.
                emit_stringish(out, expr, ctx)?;
            } else if matches!(kind, ValueKind::Child)
                && matches!(
                    expr,
                    Expr::Variable(ident, ..)
                        if ctx.scope.get(ident.1.as_str()).copied() == Some(ValueKind::Child)
                )
            {
                emit_expr(out, expr, ctx)?;
                out.push_str(".clone()");
            } else if matches!(kind, ValueKind::Int | ValueKind::Bool) {
                emit_intish(out, expr, ctx)?;
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
            if let Some((name, rhs)) = string_plus_int_assign(boxed, ctx) {
                out.push_str("    ");
                out.push_str(name);
                out.push_str(" = format!(\"{}{}\", ");
                out.push_str(name);
                out.push_str(", ");
                emit_stringish(out, rhs, ctx)?;
                out.push_str(");\n");
                return Ok(());
            }
            if emit_string_list_assign_stmt(out, boxed, ctx)? {
                return Ok(());
            }
            if emit_json_assign_stmt(out, boxed, ctx)? {
                return Ok(());
            }
            if json_assign_target(&bin.lhs, ctx).is_some() {
                let rendered =
                    crate::expr_print::expr_to_rhai(&bin.lhs).unwrap_or_else(|_| "<lhs>".into());
                let rhs =
                    crate::expr_print::expr_to_rhai(&bin.rhs).unwrap_or_else(|_| "<rhs>".into());
                return Err(RhError::Transpile(format!(
                    "unsupported json assignment value: {rendered} = {rhs}"
                )));
            }
            let Expr::Variable(ident, ..) = &bin.lhs else {
                return Err(RhError::Transpile(
                    "assignment lhs must be a variable".into(),
                ));
            };
            // `s += stringish` → push_str; Rust `String += String` does not compile.
            if let Some((_, _, _, syntax, _, _)) = op.get_op_assignment_info()
                && syntax == "+="
                && matches!(
                    ctx.scope.get(ident.1.as_str()).copied(),
                    Some(ValueKind::String | ValueKind::Path)
                )
                && is_explicit_string_expr(&bin.rhs, ctx)
            {
                out.push_str("    ");
                out.push_str(ident.1.as_str());
                out.push_str(".push_str(&");
                emit_stringish(out, &bin.rhs, ctx)?;
                out.push_str(");\n");
                return Ok(());
            }
            out.push_str("    ");
            out.push_str(ident.1.as_str());
            if let Some((_, _, _, syntax, _, _)) = op.get_op_assignment_info() {
                out.push(' ');
                out.push_str(syntax);
                out.push(' ');
            } else {
                out.push_str(" = ");
            }
            // String binding assigned from JSON path / stringish → prefer stringish emit.
            if op.get_op_assignment_info().is_none()
                && matches!(
                    ctx.scope.get(ident.1.as_str()).copied(),
                    Some(ValueKind::String | ValueKind::Path)
                )
            {
                let mut stringish = String::new();
                if emit_stringish(&mut stringish, &bin.rhs, ctx).is_ok() {
                    out.push_str(&stringish);
                } else {
                    emit_expr(out, &bin.rhs, ctx)?;
                }
            } else if op.get_op_assignment_info().is_none()
                && matches!(
                    ctx.scope.get(ident.1.as_str()).copied(),
                    Some(ValueKind::Int | ValueKind::Bool)
                )
            {
                // INT binding ← JSON field must use rh_json_int_path, not host-eval.
                emit_intish(out, &bin.rhs, ctx)?;
            } else {
                emit_expr(out, &bin.rhs, ctx)?;
            }
            out.push_str(";\n");
            // Plain `=` may rebind tracked kind (Rhai is dynamic). Never clobber
            // string/path/json/child/command/list/set with a bare int/bool — that
            // regresses later path/string uses of the same name (e.g. `output`).
            if op.get_op_assignment_info().is_none() {
                let new_kind = infer_binding_kind(&bin.rhs, ctx);
                let old = ctx.scope.get(ident.1.as_str()).copied();
                let clobber = matches!(
                    (old, new_kind),
                    (
                        Some(
                            ValueKind::String
                                | ValueKind::Path
                                | ValueKind::Json
                                | ValueKind::Child
                                | ValueKind::ChildList
                                | ValueKind::Command
                                | ValueKind::StringList
                                | ValueKind::Set
                                | ValueKind::Output
                        ),
                        ValueKind::Int | ValueKind::Bool
                    )
                );
                if !clobber {
                    *ctx = ctx.clone().with_binding(ident.1.as_str(), new_kind);
                }
            }
        }
        Stmt::Return(Some(expr), flags, ..) if flags.contains(ASTFlags::BREAK) => {
            // Rhai lowers `throw expr` to `Return` with BREAK (not FnCall("throw")).
            emit_fail_return(out, expr, ctx)?;
        }
        Stmt::Return(Some(expr), ..) => {
            out.push_str("    return ");
            emit_return_expr(out, expr, ctx)?;
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
            reject_oversized_int_for(&flow.expr)?;
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
                merge_loop_binding_upgrades(ctx, &loop_ctx, counter.name.as_str());
                out.push_str("    }\n");
            } else if let Some((binding, path)) = json_object_keys_path(&flow.expr, ctx) {
                out.push_str("    for ");
                out.push_str(counter.name.as_str());
                out.push_str(" in rh_json_object_keys(&");
                out.push_str(binding);
                out.push_str(", ");
                emit_json_path(out, &path);
                out.push_str(") {\n");
                let mut loop_ctx = ctx
                    .clone()
                    .with_binding(counter.name.as_str(), ValueKind::String);
                emit_block(out, &flow.body, &mut loop_ctx, false)?;
                merge_loop_binding_upgrades(ctx, &loop_ctx, counter.name.as_str());
                out.push_str("    }\n");
            } else if let Some(binding) = set_keys_for_path(&flow.expr, ctx) {
                out.push_str("    for ");
                out.push_str(counter.name.as_str());
                out.push_str(" in ");
                out.push_str(binding);
                out.push_str(".iter().cloned() {\n");
                let mut loop_ctx = ctx
                    .clone()
                    .with_binding(counter.name.as_str(), ValueKind::String);
                emit_block(out, &flow.body, &mut loop_ctx, false)?;
                merge_loop_binding_upgrades(ctx, &loop_ctx, counter.name.as_str());
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
                merge_loop_binding_upgrades(ctx, &loop_ctx, counter.name.as_str());
                out.push_str("    }\n");
            } else if let Some((receiver, json, separator)) = string_split_parts(&flow.expr, ctx) {
                out.push_str("    for ");
                out.push_str(counter.name.as_str());
                out.push_str(" in ");
                emit_string_split_call(out, receiver, json, separator, ctx)?;
                out.push_str(" {\n");
                let mut loop_ctx = ctx
                    .clone()
                    .with_binding(counter.name.as_str(), ValueKind::String);
                emit_block(out, &flow.body, &mut loop_ctx, false)?;
                merge_loop_binding_upgrades(ctx, &loop_ctx, counter.name.as_str());
                out.push_str("    }\n");
            } else if let Expr::FnCall(call, ..) = &flow.expr
                && call.op_token.is_none()
                && call.namespace.is_empty()
                && is_local_fn_call(call.name.as_str(), ctx)
                && matches!(
                    ctx.local_fn_return_kinds
                        .get(
                            resolve_local_fn_name(call.name.as_str(), ctx)
                                .expect("checked local fn")
                                .as_str(),
                        )
                        .copied(),
                    Some(ValueKind::StringList | ValueKind::Json)
                )
            {
                let return_kind = ctx
                    .local_fn_return_kinds
                    .get(
                        resolve_local_fn_name(call.name.as_str(), ctx)
                            .expect("checked local fn")
                            .as_str(),
                    )
                    .copied()
                    .unwrap_or(ValueKind::Int);
                out.push_str("    for ");
                out.push_str(counter.name.as_str());
                if return_kind == ValueKind::Json {
                    // Local fn returns serde_json::Value array — iterate items.
                    out.push_str(" in rh_json_array_items(&(");
                    emit_call(out, call, ctx)?;
                    out.push_str("), &[]) {\n");
                    let mut loop_ctx = ctx
                        .clone()
                        .with_binding(counter.name.as_str(), ValueKind::Json);
                    emit_block(out, &flow.body, &mut loop_ctx, false)?;
                    merge_loop_binding_upgrades(ctx, &loop_ctx, counter.name.as_str());
                } else {
                    out.push_str(" in ");
                    emit_call(out, call, ctx)?;
                    out.push_str(" {\n");
                    let mut loop_ctx = ctx
                        .clone()
                        .with_binding(counter.name.as_str(), ValueKind::String);
                    emit_block(out, &flow.body, &mut loop_ctx, false)?;
                    merge_loop_binding_upgrades(ctx, &loop_ctx, counter.name.as_str());
                }
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
                merge_loop_binding_upgrades(ctx, &loop_ctx, counter.name.as_str());
                out.push_str("    }\n");
            } else if let Expr::Variable(ident, ..) = &flow.expr
                && ctx.scope.get(ident.1.as_str()).copied() == Some(ValueKind::ChildList)
            {
                out.push_str("    for ");
                out.push_str(counter.name.as_str());
                out.push_str(" in ");
                out.push_str(ident.1.as_str());
                out.push_str(".iter() {\n");
                let mut loop_ctx = ctx
                    .clone()
                    .with_binding(counter.name.as_str(), ValueKind::Child);
                emit_block(out, &flow.body, &mut loop_ctx, false)?;
                merge_loop_binding_upgrades(ctx, &loop_ctx, counter.name.as_str());
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
                merge_loop_binding_upgrades(ctx, &loop_ctx, counter.name.as_str());
                out.push_str("    }\n");
            } else if let Expr::Array(items, ..) = &flow.expr
                && !items.is_empty()
                && items.iter().all(|item| {
                    matches!(
                        item,
                        Expr::Variable(ident, ..)
                            if matches!(
                                ctx.scope.get(ident.1.as_str()).copied(),
                                Some(ValueKind::String | ValueKind::Path)
                            )
                    )
                })
            {
                out.push_str("    for ");
                out.push_str(counter.name.as_str());
                out.push_str(" in [");
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        out.push_str(", ");
                    }
                    if let Expr::Variable(ident, ..) = item {
                        out.push_str(ident.1.as_str());
                        out.push_str(".clone()");
                    }
                }
                out.push_str("].iter().cloned() {\n");
                let mut loop_ctx = ctx
                    .clone()
                    .with_binding(counter.name.as_str(), ValueKind::String);
                emit_block(out, &flow.body, &mut loop_ctx, false)?;
                merge_loop_binding_upgrades(ctx, &loop_ctx, counter.name.as_str());
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
                    merge_loop_binding_upgrades(ctx, &loop_ctx, counter.name.as_str());
                    out.push_str("    }\n");
                } else {
                    return Err(RhError::Transpile(format!(
                        "unsupported for-loop iterable in native pack: {:?}",
                        flow.expr
                    )));
                }
            } else {
                return Err(RhError::Transpile(format!(
                    "unsupported for-loop iterable in native pack: {:?}",
                    flow.expr
                )));
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
                return Err(RhError::Transpile(format!(
                    "unsupported while condition in native pack: {:?}",
                    flow.expr
                )));
            }
        }
        Stmt::TryCatch(boxed, ..) if ctx.cdylib => {
            let flow = boxed.as_ref();
            // Statement-form try must discard the INT result; a bare `match`
            // arm value is not a valid Rust statement expression.
            out.push_str("    let _ = match (|| -> Result<INT, INT> {\n");
            let mut try_ctx = ctx.clone().enter_try();
            emit_try_block(out, &flow.body, &mut try_ctx)?;
            out.push_str("    })() {\n");
            out.push_str("        Ok(__rh_try_v) => __rh_try_v,\n");
            let catch_name = match &flow.expr {
                Expr::Variable(ident, ..) if !ident.1.as_str().is_empty() => {
                    Some(ident.1.as_str().to_string())
                }
                _ => None,
            };
            if let Some(name) = &catch_name {
                out.push_str("        Err(");
                out.push_str(name);
                out.push_str(") => {\n");
                let mut catch_ctx = ctx.clone().with_binding(name.as_str(), ValueKind::Int);
                emit_catch_block_stmts(out, &flow.branch, &mut catch_ctx)?;
                out.push_str("            0\n");
                out.push_str("        }\n");
            } else {
                out.push_str("        Err(_) => {\n");
                emit_catch_block_stmts(out, &flow.branch, ctx)?;
                out.push_str("            0\n");
                out.push_str("        }\n");
            }
            out.push_str("    };\n");
        }
        Stmt::TryCatch(..) => {
            return Err(RhError::Transpile(
                "try/catch requires native cdylib lowering".into(),
            ));
        }
        Stmt::Block(boxed) => {
            out.push_str("    {\n");
            emit_block(out, boxed, ctx, implicit_return)?;
            out.push_str("    }\n");
        }
        Stmt::Expr(expr) if ctx.cdylib && emit_string_mut_stmt(out, expr, ctx)? => {}
        Stmt::Expr(expr) if ctx.cdylib && emit_command_mut_stmt(out, expr, ctx)? => {}
        Stmt::Expr(expr) if ctx.cdylib && emit_bytes_mut_stmt(out, expr, ctx)? => {}
        Stmt::Expr(expr) if ctx.cdylib && emit_child_mut_stmt(out, expr, ctx)? => {}
        Stmt::Expr(expr) if ctx.cdylib && emit_window_control_mut_stmt(out, expr, ctx)? => {}
        Stmt::Expr(expr) if ctx.cdylib && emit_string_list_push_stmt(out, expr, ctx)? => {}
        Stmt::Expr(expr) if ctx.cdylib && emit_child_list_push_stmt(out, expr, ctx)? => {}
        Stmt::Expr(expr) if ctx.cdylib && emit_json_array_push_stmt(out, expr, ctx)? => {}
        Stmt::Expr(expr) if ctx.cdylib && emit_json_root_mutation_stmt(out, expr, ctx)? => {}
        Stmt::Expr(expr) if ctx.cdylib && emit_task_sleep_stmt(out, expr, ctx)? => {}
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
            emit_return_expr(out, expr, ctx)?;
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
            if ctx.current_return_kind == ValueKind::Int
                && call.namespace.is_empty()
                && call.name == "print"
                && call.args.len() == 1
            {
                out.push_str("    ");
                emit_call(out, call, ctx)?;
                out.push_str(";\n    return 0;\n");
            } else {
                out.push_str("    return ");
                emit_call(out, call, ctx)?;
                out.push_str(";\n");
            }
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

fn emit_catch_block_stmts(
    out: &mut String,
    block: &StmtBlock,
    ctx: &mut EmitCtx,
) -> Result<(), RhError> {
    for stmt in block.iter() {
        let mut inner = String::new();
        emit_stmt(&mut inner, stmt, ctx, false)?;
        for line in inner.lines() {
            out.push_str("            ");
            out.push_str(line);
            out.push('\n');
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
            if let Expr::FnCall(call, ..) = expr.as_ref()
                && call.namespace.is_empty()
                && call.name == "print"
                && call.args.len() == 1
            {
                out.push_str("        ");
                emit_call(out, call, ctx)?;
                out.push_str(";\n        Ok(0)\n");
            } else {
                out.push_str("        return Ok(");
                emit_intish(out, expr, ctx)?;
                out.push_str(");\n");
            }
        }
        Stmt::Return(Some(expr), flags, ..) if flags.contains(ASTFlags::BREAK) => {
            // Rhai parses `throw msg` as a BREAK-flagged return. Keep Err arm INT.
            out.push_str("        return Err(");
            if is_pure_int_expr(expr) {
                emit_expr(out, expr, ctx)?;
            } else {
                emit_rh_fail(out, expr, ctx)?;
            }
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
    _implicit_return: bool,
) -> Result<(), RhError> {
    if ctx.cdylib && !ctx.in_try() && call.args.len() == 1 {
        emit_fail_return(out, &call.args[0], ctx)?;
        return Ok(());
    }
    if ctx.in_try() {
        out.push_str("    ");
        emit_throw_expr(out, call, ctx)?;
        out.push('\n');
    } else {
        return Err(RhError::Transpile(
            "throw outside try requires native cdylib lowering".into(),
        ));
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
    // Indent one level deeper than emit_fail_return's default body indent.
    let mut fail = String::new();
    emit_fail_return(&mut fail, message, ctx)?;
    for line in fail.lines() {
        out.push_str("    ");
        out.push_str(line);
        out.push('\n');
    }
    out.push_str("    }\n");
    Ok(())
}

fn fail_return_default(kind: ValueKind) -> Option<&'static str> {
    match kind {
        ValueKind::Int | ValueKind::Bool => None,
        ValueKind::String | ValueKind::Path => Some("String::new()"),
        ValueKind::Json => Some("serde_json::Value::Null"),
        ValueKind::StringList | ValueKind::ChildList => Some("Vec::new()"),
        ValueKind::Set => Some("std::collections::HashSet::<String>::new()"),
        // Typed placeholders after rh_fail so throw keeps the function return kind.
        ValueKind::Output => Some(
            "RhOutput { success: 0, exit_code: -1, stdout: String::new(), stderr: String::new() }",
        ),
        ValueKind::Child => Some("RhChild::exited(0, 64 * 1024)"),
        ValueKind::WindowControl => {
            Some("RhWindowControl { child: RhChild::exited(0, 64 * 1024), id: 0 }")
        }
        ValueKind::WindowRect => Some("RhWindowRect { left: 0, top: 0, right: 0, bottom: 0 }"),
        ValueKind::Bytes => Some("RhBytes { bytes: Vec::new() }"),
        ValueKind::Command => None,
        ValueKind::Char
        | ValueKind::Metadata
        | ValueKind::SystemTime
        | ValueKind::DirEntry
        | ValueKind::Stream => None,
    }
}

fn emit_fail_return(out: &mut String, message: &Expr, ctx: &mut EmitCtx) -> Result<(), RhError> {
    if let Some(default) = fail_return_default(ctx.current_return_kind) {
        out.push_str("    let _ = ");
        emit_rh_fail(out, message, ctx)?;
        out.push_str(";\n    return ");
        out.push_str(default);
        out.push_str(";\n");
    } else {
        out.push_str("    return ");
        emit_rh_fail(out, message, ctx)?;
        out.push_str(";\n");
    }
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
        out.push_str("return Err(");
        emit_rh_fail(out, &call.args[0], ctx)?;
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
        Expr::Array(items, ..)
            if !items.is_empty()
                && items.iter().all(|item| {
                    matches!(
                        item,
                        Expr::Variable(ident, ..)
                            if ctx.scope.get(ident.1.as_str()).copied() == Some(ValueKind::Child)
                    )
                }) =>
        {
            ValueKind::ChildList
        }
        Expr::Array(items, ..)
            if !items.is_empty() && items.iter().all(|item| is_stringish_array_item(item, ctx)) =>
        {
            ValueKind::StringList
        }
        Expr::Array(items, ..)
            if !items.is_empty()
                && items
                    .iter()
                    .all(|item| is_native_json_value_item(item, ctx)) =>
        {
            ValueKind::Json
        }
        Expr::Variable(ident, ..) => match ctx.scope.get(ident.1.as_str()).copied() {
            Some(kind) => kind,
            None => ValueKind::Int,
        },
        _ if bytes_from_array_items(expr).is_some() || bytes_from_text_arg(expr).is_some() => {
            ValueKind::Bytes
        }
        _ if std_process_kill_arg(expr).is_some() => ValueKind::Int,
        _ if std_fs_write_args(expr).is_some() => ValueKind::Int,
        _ if parse_string_method_call(expr, ctx)
            .is_some_and(|(_, call)| call.name == "index_of" && call.args.len() == 1) =>
        {
            ValueKind::Int
        }
        _ if json_parse_arg(expr).is_some() => ValueKind::Json,
        _ if json_string_field_path(expr, ctx).is_some() => ValueKind::String,
        _ if json_value_path(expr, ctx).is_some_and(|(_, path)| !path.is_empty()) => {
            ValueKind::Json
        }
        _ if json_array_index(expr, ctx).is_some() => ValueKind::Json,
        _ if json_path_array_index(expr, ctx).is_some() => ValueKind::Json,
        _ if json_path_key_get(expr, ctx).is_some() => ValueKind::Json,
        _ if json_rhai_array_index_property(expr, ctx).is_some() => ValueKind::Json,
        _ if child_list_index(expr, ctx).is_some() => ValueKind::Child,
        _ if string_list_index(expr, ctx).is_some() => ValueKind::String,
        _ if string_split_parts(expr, ctx).is_some() => ValueKind::StringList,
        _ if json_stringify_pretty_arg(expr).is_some() => ValueKind::String,
        _ if json_stringify_arg(expr).is_some() => ValueKind::String,
        _ if string_sub_string_arg(expr, ctx).is_some() => ValueKind::String,
        _ if string_concat_args(expr, ctx).is_some() => ValueKind::String,
        _ if args_index_expr(expr).is_some() => ValueKind::String,
        _ if std_fs_read_to_string_arg(expr).is_some() => ValueKind::String,
        _ if path_join_display_args(expr).is_some() => ValueKind::String,
        _ if path_parent_display_arg(expr).is_some() => ValueKind::String,
        _ if path_absolute_display_arg(expr).is_some() => ValueKind::String,
        _ if path_buf_from_display_arg(expr).is_some() => ValueKind::String,
        _ if path_buf_from_file_name_arg(expr).is_some() => ValueKind::String,
        _ if env_current_dir_display(expr) => ValueKind::String,
        _ if string_method_on_path_display(expr, ctx).is_some() => ValueKind::String,
        _ if path_buf_from_arg(expr).is_some() => ValueKind::Path,
        _ if std_fs_symlink_metadata_arg(expr).is_some() => ValueKind::Metadata,
        _ if std_fs_metadata_arg(expr).is_some() => ValueKind::Metadata,
        _ if dir_entry_metadata_binding(expr, ctx).is_some() => ValueKind::Metadata,
        _ if metadata_modified_binding(expr, ctx).is_some()
            || fs_metadata_modified_arg(expr).is_some() =>
        {
            ValueKind::SystemTime
        }
        _ if system_time_unix_millis_binding(expr, ctx).is_some()
            || fs_metadata_modified_unix_millis(expr).is_some()
            || dir_entry_metadata_modified_unix_millis(expr, ctx).is_some()
            || dir_entry_metadata_len(expr, ctx).is_some()
            || fs_metadata_len_arg(expr).is_some()
            || metadata_property_binding(expr, ctx).is_some_and(|(_, name)| name == "len") =>
        {
            ValueKind::Int
        }
        _ if system_time_rfc3339_binding(expr, ctx).is_some()
            || fs_metadata_modified_rfc3339(expr).is_some()
            || dir_entry_metadata_modified_rfc3339(expr, ctx).is_some() =>
        {
            ValueKind::String
        }
        _ if json_parse_file_arg(expr).is_some() => ValueKind::Json,
        _ if image_inspect_png_arg(expr).is_some() => ValueKind::Json,
        _ if clipboard_get_text_arg(expr) => ValueKind::String,
        _ if std_time_system_time_now_unix_millis(expr) => ValueKind::Int,
        _ if std_process_list(expr) => ValueKind::Json,
        _ if std_process_id(expr) => ValueKind::Int,
        _ if std_process_command_arg(expr).is_some() => ValueKind::Command,
        _ if command_output_call(expr, ctx).is_some() => ValueKind::Output,
        _ if command_start_call(expr, ctx).is_some() => ValueKind::Child,
        _ if local_command_receiver_call(expr, ctx, "output").is_some() => ValueKind::Output,
        _ if local_command_receiver_call(expr, ctx, "start").is_some() => ValueKind::Child,
        _ if child_wait_with_output_call(expr, ctx).is_some() => ValueKind::Output,
        _ if output_stdout_text_call(expr, ctx).is_some() => ValueKind::String,
        _ if output_stderr_text_call(expr, ctx).is_some() => ValueKind::String,
        _ if bytes_to_text_call(expr, ctx).is_some() => ValueKind::String,
        _ if stream_method_call(expr, ctx)
            .is_some_and(|(_, call)| call.name == "read" && call.args.len() == 2) =>
        {
            ValueKind::Bytes
        }
        _ if bytes_property_binding(expr, ctx).is_some() => ValueKind::Int,
        _ if char_to_string_binding(expr, ctx).is_some() => ValueKind::String,
        _ if child_property_binding(expr, ctx)
            .is_some_and(|(_, property)| property == "platform_facts") =>
        {
            ValueKind::Json
        }
        _ if child_property_binding(expr, ctx)
            .is_some_and(|(_, property)| property == "stderr") =>
        {
            ValueKind::Stream
        }
        _ if child_property_binding(expr, ctx)
            .is_some_and(|(_, property)| property == "stdout") =>
        {
            ValueKind::Stream
        }
        _ if child_property_binding(expr, ctx).is_some_and(|(_, property)| property == "id") => {
            ValueKind::Int
        }
        _ if child_state_binding(expr, ctx).is_some() => ValueKind::String,
        _ if child_window_control_call(expr, ctx).is_some() => ValueKind::WindowControl,
        _ if child_window_client_rect_call(expr, ctx).is_some()
            || child_window_rect_call(expr, ctx).is_some() =>
        {
            ValueKind::WindowRect
        }
        _ if window_control_property_binding(expr, ctx)
            .is_some_and(|(_, property)| property == "visible") =>
        {
            ValueKind::Bool
        }
        _ if window_control_property_binding(expr, ctx)
            .is_some_and(|(_, property)| property == "text") =>
        {
            ValueKind::String
        }
        _ if window_rect_property_binding(expr, ctx).is_some() => ValueKind::Int,
        _ if child_method_call(expr, ctx)
            .is_some_and(|(_, call)| call.name == "window_message" && call.args.len() == 3) =>
        {
            ValueKind::Int
        }
        _ if std_time_system_time_now_rfc3339(expr) => ValueKind::String,
        _ if std_env_get_arg(expr).is_some() => ValueKind::String,
        _ if parse_string_method_call(expr, ctx)
            .is_some_and(|(_, call)| call.name == "parse_int" && call.args.is_empty()) =>
        {
            ValueKind::Int
        }
        _ if crypto_sha256_file_arg(expr).is_some() => ValueKind::String,
        _ if hash_fnv1a64_arg(expr).is_some() => ValueKind::String,
        _ if dir_entry_path_display_binding(expr, ctx).is_some() => ValueKind::String,
        _ if dir_entry_string_field(expr, ctx).is_some() => ValueKind::String,
        // Local calls must win over `uses_host_surface`: otherwise
        // `new_context(args[0], …)` is mis-typed as Bool because `args[0]` is a
        // host surface nested in the call arguments.
        Expr::FnCall(call, ..)
            if call.namespace.is_empty()
                && call.op_token.is_none()
                && is_local_fn_call(call.name.as_str(), ctx) =>
        {
            resolve_local_fn_name(call.name.as_str(), ctx)
                .and_then(|name| ctx.local_fn_return_kinds.get(name.as_str()).copied())
                .unwrap_or(ValueKind::Int)
        }
        _ if parse_string_method_call(expr, ctx).is_some_and(|(_, call)| {
            matches!(call.name.as_str(), "trim" | "to_lower" | "to_string") && call.args.is_empty()
        }) =>
        {
            ValueKind::String
        }
        _ if json_string_field_path(expr, ctx).is_some() => ValueKind::String,
        _ if output_property_binding(expr, ctx)
            .is_some_and(|(_, property)| property == "stdout" || property == "stderr") =>
        {
            ValueKind::Bytes
        }
        _ if parse_string_method_call(expr, ctx).is_some_and(|(_, call)| {
            matches!(call.name.as_str(), "trim" | "to_lower" | "to_string") && call.args.is_empty()
        }) =>
        {
            ValueKind::String
        }
        _ if output_property_binding(expr, ctx)
            .is_some_and(|(_, property)| property == "stdout" || property == "stderr") =>
        {
            ValueKind::Bytes
        }
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
    let lhs = &call.args[0];
    let rhs = &call.args[1];
    let has_len_operand = json_array_len_path(lhs, ctx).is_some()
        || json_array_len_path(rhs, ctx).is_some()
        || json_object_keys_len_path(lhs, ctx).is_some()
        || json_object_keys_len_path(rhs, ctx).is_some()
        || is_var_len_expr(lhs)
        || is_var_len_expr(rhs);
    let is_string_concat = prefers_string_ops(lhs, rhs, ctx)
        || (has_len_operand
            && (is_explicit_string_expr(lhs, ctx) || is_explicit_string_expr(rhs, ctx)));
    is_string_concat.then_some((lhs, rhs))
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

fn reject_oversized_int_for(iterable: &Expr) -> Result<(), RhError> {
    let Expr::FnCall(call, ..) = iterable else {
        return Ok(());
    };
    if call.args.len() != 2 {
        return Ok(());
    }
    let Some(IntForBound::Const(start)) = int_for_bound(&call.args[0]) else {
        return Ok(());
    };
    let Some(IntForBound::Const(end)) = int_for_bound(&call.args[1]) else {
        return Ok(());
    };
    let is_exclusive = call.name.as_str() == Token::ExclusiveRange.literal_syntax();
    let is_inclusive = call.name.as_str() == Token::InclusiveRange.literal_syntax();
    if !is_exclusive && !is_inclusive {
        return Ok(());
    }
    let within_limit = if is_exclusive {
        end <= start || bounded_exclusive_span(start, end).is_some()
    } else {
        bounded_inclusive_span(start, end).is_some()
    };
    if !within_limit {
        return Err(RhError::Transpile(format!(
            "for-loop span exceeds native limit of {MAX_NATIVE_FOR_SPAN} elements"
        )));
    }
    Ok(())
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

fn path_parent_display_arg(expr: &Expr) -> Option<&Expr> {
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
    if call.namespace.to_string() != "std::path" || call.name != "parent" || call.args.len() != 1 {
        return None;
    }
    Some(&call.args[0])
}

/// Path/display forms that already emit as owned `String` in native packs.
fn path_display_string_expr(expr: &Expr, ctx: &EmitCtx) -> bool {
    path_join_display_args(expr).is_some()
        || path_parent_display_arg(expr).is_some()
        || path_absolute_display_arg(expr).is_some()
        || path_buf_from_display_arg(expr).is_some()
        || path_binding_display(expr, ctx).is_some()
        || env_current_dir_display(expr)
        || dir_entry_path_display_binding(expr, ctx).is_some()
}

fn is_display_property_or_method(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::Property(property, ..) if property.2.as_str() == "display"
    ) || matches!(
        expr,
        Expr::MethodCall(call, ..) if call.name == "display" && call.args.is_empty()
    )
}

fn path_parent_fn_arg(expr: &Expr) -> Option<&Expr> {
    let Expr::FnCall(call, ..) = expr else {
        return None;
    };
    if call.namespace.to_string() != "std::path" || call.name != "parent" || call.args.len() != 1 {
        return None;
    }
    Some(&call.args[0])
}

fn path_absolute_fn_arg(expr: &Expr) -> Option<&Expr> {
    let Expr::FnCall(call, ..) = expr else {
        return None;
    };
    if call.namespace.to_string() != "std::path" || call.name != "absolute" || call.args.len() != 1
    {
        return None;
    }
    Some(&call.args[0])
}

fn path_join_fn_args(expr: &Expr) -> Option<(&Expr, &Expr)> {
    let Expr::FnCall(call, ..) = expr else {
        return None;
    };
    if call.namespace.to_string() != "std::path" || call.name != "join" || call.args.len() != 2 {
        return None;
    }
    Some((&call.args[0], &call.args[1]))
}

fn env_current_dir_fn(expr: &Expr) -> bool {
    let Expr::FnCall(call, ..) = expr else {
        return false;
    };
    call.namespace.to_string() == "std::env" && call.name == "current_dir" && call.args.is_empty()
}

/// Rhai nests `path.display.to_lower()` as `Dot(path, Dot(display, to_lower))`.
fn string_method_on_path_display<'a>(
    expr: &'a Expr,
    ctx: &EmitCtx,
) -> Option<&'a rhai::FnCallExpr> {
    let Expr::Dot(boxed, ..) = expr else {
        return None;
    };
    // Shape A: Dot(path.display, MethodCall(to_lower))
    if let Expr::MethodCall(call, ..) = &boxed.rhs {
        if call.args.is_empty()
            && matches!(call.name.as_str(), "to_lower" | "trim" | "to_string")
            && path_display_string_expr(&boxed.lhs, ctx)
        {
            return Some(call);
        }
        return None;
    }
    // Shape B: Dot(path_fn|binding, Dot(Property(display), MethodCall(to_lower)))
    let Expr::Dot(inner, ..) = &boxed.rhs else {
        return None;
    };
    let Expr::MethodCall(call, ..) = &inner.rhs else {
        return None;
    };
    if !call.args.is_empty()
        || !matches!(call.name.as_str(), "to_lower" | "trim" | "to_string")
        || !is_display_property_or_method(&inner.lhs)
    {
        return None;
    }
    if path_parent_fn_arg(&boxed.lhs).is_some()
        || path_absolute_fn_arg(&boxed.lhs).is_some()
        || path_join_fn_args(&boxed.lhs).is_some()
        || path_buf_from_arg(&boxed.lhs).is_some()
        || env_current_dir_fn(&boxed.lhs)
        || matches!(
            &boxed.lhs,
            Expr::Variable(ident, ..)
                if matches!(
                    ctx.scope.get(ident.1.as_str()).copied(),
                    Some(ValueKind::String | ValueKind::Path)
                )
        )
    {
        return Some(call);
    }
    None
}

fn emit_string_method_on_path_display(
    out: &mut String,
    expr: &Expr,
    ctx: &mut EmitCtx,
) -> Result<bool, RhError> {
    let Some(call) = string_method_on_path_display(expr, ctx) else {
        return Ok(false);
    };
    let Expr::Dot(boxed, ..) = expr else {
        return Ok(false);
    };
    if matches!(&boxed.rhs, Expr::MethodCall(..)) {
        // Shape A: lhs is already a path.display string expr.
        emit_stringish(out, &boxed.lhs, ctx)?;
    } else {
        // Shape B: emit path.display from lhs + nested display property.
        if let Some(path) = path_parent_fn_arg(&boxed.lhs) {
            if !emit_path_parent(out, path, ctx)? {
                return Ok(false);
            }
        } else if let Some(path) = path_absolute_fn_arg(&boxed.lhs) {
            if !emit_path_absolute(out, path, ctx)? {
                return Ok(false);
            }
        } else if let Some((base, child)) = path_join_fn_args(&boxed.lhs) {
            if !emit_path_join(out, base, child, ctx)? {
                return Ok(false);
            }
        } else if let Some(path) = path_buf_from_arg(&boxed.lhs) {
            if !emit_path_buf_from(out, path, ctx)? {
                return Ok(false);
            }
        } else if env_current_dir_fn(&boxed.lhs) {
            out.push_str("rh_env_current_dir()");
        } else if let Expr::Variable(ident, ..) = &boxed.lhs {
            out.push_str(ident.1.as_str());
        } else {
            return Ok(false);
        }
    }
    match call.name.as_str() {
        "to_lower" => out.push_str(".to_ascii_lowercase()"),
        "trim" => out.push_str(".trim().to_string()"),
        "to_string" => {}
        _ => return Ok(false),
    }
    Ok(true)
}

fn std_fs_symlink_metadata_arg(expr: &Expr) -> Option<&Expr> {
    std_fs_single_arg(expr, "symlink_metadata")
}

fn std_fs_metadata_arg(expr: &Expr) -> Option<&Expr> {
    std_fs_single_arg(expr, "metadata")
}

fn path_buf_from_arg(expr: &Expr) -> Option<&Expr> {
    let Expr::FnCall(call, ..) = expr else {
        return None;
    };
    if call.namespace.to_string() != "std::path::PathBuf"
        || call.name != "from"
        || call.args.len() != 1
    {
        return None;
    }
    Some(&call.args[0])
}

fn path_buf_from_display_arg(expr: &Expr) -> Option<&Expr> {
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
    path_buf_from_arg(&boxed.lhs)
}

fn path_buf_from_is_absolute_arg(expr: &Expr) -> Option<&Expr> {
    let Expr::Dot(boxed, ..) = expr else {
        return None;
    };
    let absolute = matches!(
        &boxed.rhs,
        Expr::Property(property, ..) if property.2.as_str() == "is_absolute"
    ) || matches!(
        &boxed.rhs,
        Expr::MethodCall(call, ..) if call.name == "is_absolute" && call.args.is_empty()
    );
    if !absolute {
        return None;
    }
    path_buf_from_arg(&boxed.lhs)
}

fn path_buf_from_file_name_arg(expr: &Expr) -> Option<&Expr> {
    let Expr::Dot(boxed, ..) = expr else {
        return None;
    };
    let file_name = matches!(
        &boxed.rhs,
        Expr::Property(property, ..) if property.2.as_str() == "file_name"
    ) || matches!(
        &boxed.rhs,
        Expr::MethodCall(call, ..) if call.name == "file_name" && call.args.is_empty()
    );
    if !file_name {
        return None;
    }
    path_buf_from_arg(&boxed.lhs)
}

fn path_binding_is_absolute<'a>(expr: &'a Expr, ctx: &EmitCtx) -> Option<&'a str> {
    let Expr::Dot(boxed, ..) = expr else {
        return None;
    };
    let Expr::Variable(ident, ..) = &boxed.lhs else {
        return None;
    };
    if ctx.scope.get(ident.1.as_str()).copied() != Some(ValueKind::Path) {
        return None;
    }
    let absolute = matches!(
        &boxed.rhs,
        Expr::Property(property, ..) if property.2.as_str() == "is_absolute"
    ) || matches!(
        &boxed.rhs,
        Expr::MethodCall(call, ..) if call.name == "is_absolute" && call.args.is_empty()
    );
    absolute.then_some(ident.1.as_str())
}

fn path_binding_display<'a>(expr: &'a Expr, ctx: &EmitCtx) -> Option<&'a str> {
    let Expr::Dot(boxed, ..) = expr else {
        return None;
    };
    let Expr::Variable(ident, ..) = &boxed.lhs else {
        return None;
    };
    if ctx.scope.get(ident.1.as_str()).copied() != Some(ValueKind::Path) {
        return None;
    }
    let display = matches!(
        &boxed.rhs,
        Expr::Property(property, ..) if property.2.as_str() == "display"
    ) || matches!(
        &boxed.rhs,
        Expr::MethodCall(call, ..) if call.name == "display" && call.args.is_empty()
    );
    display.then_some(ident.1.as_str())
}

fn path_binding_file_name<'a>(expr: &'a Expr, ctx: &EmitCtx) -> Option<&'a str> {
    let Expr::Dot(boxed, ..) = expr else {
        return None;
    };
    let Expr::Variable(ident, ..) = &boxed.lhs else {
        return None;
    };
    if ctx.scope.get(ident.1.as_str()).copied() != Some(ValueKind::Path) {
        return None;
    }
    let file_name = matches!(
        &boxed.rhs,
        Expr::Property(property, ..) if property.2.as_str() == "file_name"
    ) || matches!(
        &boxed.rhs,
        Expr::MethodCall(call, ..) if call.name == "file_name" && call.args.is_empty()
    );
    file_name.then_some(ident.1.as_str())
}

fn env_current_dir_display(expr: &Expr) -> bool {
    let Expr::Dot(boxed, ..) = expr else {
        return false;
    };
    let display = matches!(
        &boxed.rhs,
        Expr::Property(property, ..) if property.2.as_str() == "display"
    ) || matches!(
        &boxed.rhs,
        Expr::MethodCall(call, ..) if call.name == "display" && call.args.is_empty()
    );
    if !display {
        return false;
    }
    let Expr::FnCall(call, ..) = &boxed.lhs else {
        return false;
    };
    call.namespace.to_string() == "std::env" && call.name == "current_dir" && call.args.is_empty()
}

fn call_host_api_module(call: &rhai::FnCallExpr) -> Option<&str> {
    host_api_module(&call.namespace.to_string())
}

fn is_host_api_call(call: &rhai::FnCallExpr, module: &str, name: &str, arity: usize) -> bool {
    call_host_api_module(call) == Some(module) && call.name == name && call.args.len() == arity
}

fn json_parse_file_arg(expr: &Expr) -> Option<&Expr> {
    let Expr::FnCall(call, ..) = expr else {
        return None;
    };
    if !is_host_api_call(call, "json", "parse_file", 1) {
        return None;
    }
    Some(&call.args[0])
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

fn std_fs_remove_dir_all_arg(expr: &Expr) -> Option<&Expr> {
    std_fs_single_arg(expr, "remove_dir_all")
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

fn std_env_get_parse_int_arg(expr: &Expr) -> Option<&Expr> {
    let Expr::Dot(boxed, ..) = expr else {
        return None;
    };
    let Expr::MethodCall(call, ..) = &boxed.rhs else {
        return None;
    };
    if call.name != "parse_int" || !call.args.is_empty() {
        return None;
    }
    std_env_get_arg(&boxed.lhs)
}

fn crypto_sha256_file_arg(expr: &Expr) -> Option<&Expr> {
    let Expr::FnCall(call, ..) = expr else {
        return None;
    };
    if !is_host_api_call(call, "crypto", "sha256_file", 1) {
        return None;
    }
    Some(&call.args[0])
}

fn hash_fnv1a64_arg(expr: &Expr) -> Option<&Expr> {
    let Expr::FnCall(call, ..) = expr else {
        return None;
    };
    if !is_host_api_call(call, "hash", "fnv1a64", 1) {
        return None;
    }
    Some(&call.args[0])
}

fn bytes_from_text_arg(expr: &Expr) -> Option<&Expr> {
    let Expr::FnCall(call, ..) = expr else {
        return None;
    };
    if !is_host_api_call(call, "bytes", "from_text", 1) {
        return None;
    }
    Some(&call.args[0])
}

fn bytes_from_array_items(expr: &Expr) -> Option<&[Expr]> {
    let Expr::FnCall(call, ..) = expr else {
        return None;
    };
    if !is_host_api_call(call, "bytes", "from_array", 1) {
        return None;
    }
    let Expr::Array(items, ..) = &call.args[0] else {
        return None;
    };
    if !items
        .iter()
        .all(|item| matches!(item, Expr::IntegerConstant(value, ..) if (0..=255).contains(value)))
    {
        return None;
    }
    Some(items.as_slice())
}

fn std_process_kill_arg(expr: &Expr) -> Option<&Expr> {
    let Expr::FnCall(call, ..) = expr else {
        return None;
    };
    if call.namespace.to_string() != "std::process" || call.name != "kill" || call.args.len() != 1 {
        return None;
    }
    Some(&call.args[0])
}

fn std_fs_write_args(expr: &Expr) -> Option<(&Expr, &Expr)> {
    std_fs_two_arg(expr, "write")
}

fn runtime_atomic_write_args(expr: &Expr) -> Option<(&Expr, &Expr)> {
    let Expr::FnCall(call, ..) = expr else {
        return None;
    };
    if !is_host_api_call(call, "runtime", "atomic_write", 2) {
        return None;
    }
    Some((&call.args[0], &call.args[1]))
}

fn json_stringify_pretty_arg(expr: &Expr) -> Option<&Expr> {
    let Expr::FnCall(call, ..) = expr else {
        return None;
    };
    if !is_host_api_call(call, "json", "stringify_pretty", 1) {
        return None;
    }
    Some(&call.args[0])
}

fn json_stringify_arg(expr: &Expr) -> Option<&Expr> {
    let Expr::FnCall(call, ..) = expr else {
        return None;
    };
    if !is_host_api_call(call, "json", "stringify", 1) {
        return None;
    }
    Some(&call.args[0])
}

fn runtime_append_sync_args(expr: &Expr) -> Option<(&Expr, &Expr)> {
    let Expr::FnCall(call, ..) = expr else {
        return None;
    };
    if !is_host_api_call(call, "runtime", "append_sync", 2) {
        return None;
    }
    Some((&call.args[0], &call.args[1]))
}

fn std_process_id(expr: &Expr) -> bool {
    let Expr::FnCall(call, ..) = expr else {
        return false;
    };
    call.namespace.to_string() == "std::process" && call.name == "id" && call.args.is_empty()
}

fn std_process_list(expr: &Expr) -> bool {
    let Expr::FnCall(call, ..) = expr else {
        return false;
    };
    call.namespace.to_string() == "std::process" && call.name == "list" && call.args.is_empty()
}

fn image_inspect_png_arg(expr: &Expr) -> Option<&Expr> {
    let Expr::FnCall(call, ..) = expr else {
        return None;
    };
    is_host_api_call(call, "image", "inspect_png", 1).then(|| &call.args[0])
}

fn clipboard_get_text_arg(expr: &Expr) -> bool {
    let Expr::FnCall(call, ..) = expr else {
        return false;
    };
    is_host_api_call(call, "clipboard", "get_text", 0)
}

fn clipboard_set_text_arg(expr: &Expr) -> Option<&Expr> {
    let Expr::FnCall(call, ..) = expr else {
        return None;
    };
    is_host_api_call(call, "clipboard", "set_text", 1).then(|| &call.args[0])
}

fn std_process_command_arg(expr: &Expr) -> Option<&Expr> {
    let Expr::FnCall(call, ..) = expr else {
        return None;
    };
    if call.namespace.to_string() != "std::process"
        || call.name != "command"
        || call.args.len() != 1
    {
        return None;
    }
    Some(&call.args[0])
}

fn command_binding_method<'a>(
    expr: &'a Expr,
    ctx: &EmitCtx,
    kind: ValueKind,
) -> Option<(&'a str, &'a rhai::FnCallExpr)> {
    let Expr::Dot(boxed, ..) = expr else {
        return None;
    };
    let Expr::Variable(ident, ..) = &boxed.lhs else {
        return None;
    };
    if ctx.scope.get(ident.1.as_str()).copied() != Some(kind) {
        return None;
    }
    let call = match &boxed.rhs {
        Expr::MethodCall(call, ..) => call,
        _ => return None,
    };
    Some((ident.1.as_str(), call))
}

fn is_local_fn_call(name: &str, ctx: &EmitCtx) -> bool {
    resolve_local_fn_name(name, ctx).is_some()
}

fn resolve_local_fn_name(name: &str, ctx: &EmitCtx) -> Option<String> {
    if ctx.local_fns.contains(name) {
        return Some(name.to_owned());
    }
    let suffix = format!("__{name}");
    ctx.local_fns
        .iter()
        .find(|mangled| mangled.ends_with(suffix.as_str()))
        .cloned()
}

fn string_list_push_call<'a>(expr: &'a Expr, ctx: &EmitCtx) -> Option<(&'a str, &'a Expr)> {
    let Expr::Dot(boxed, ..) = expr else {
        return None;
    };
    let Expr::Variable(ident, ..) = &boxed.lhs else {
        return None;
    };
    if ctx.scope.get(ident.1.as_str()).copied() != Some(ValueKind::StringList) {
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

fn emit_string_list_push_stmt(
    out: &mut String,
    expr: &Expr,
    ctx: &mut EmitCtx,
) -> Result<bool, RhError> {
    let Some((binding, item)) = string_list_push_call(expr, ctx) else {
        return Ok(false);
    };
    out.push_str("    ");
    out.push_str(binding);
    out.push_str(".push(");
    // Clone string bindings so `list.push(x); other.push(x)` stays valid.
    if !emit_owned_string_element(out, item, ctx)? {
        return Err(RhError::Transpile(
            "string list push argument must be a stringish value".into(),
        ));
    }
    out.push_str(
        ");
",
    );
    Ok(true)
}

fn child_list_push_call<'a>(expr: &'a Expr, ctx: &EmitCtx) -> Option<(&'a str, &'a Expr)> {
    let Expr::Dot(boxed, ..) = expr else {
        return None;
    };
    let Expr::Variable(ident, ..) = &boxed.lhs else {
        return None;
    };
    if ctx.scope.get(ident.1.as_str()).copied() != Some(ValueKind::ChildList) {
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

fn emit_child_list_push_stmt(
    out: &mut String,
    expr: &Expr,
    ctx: &mut EmitCtx,
) -> Result<bool, RhError> {
    let Some((binding, item)) = child_list_push_call(expr, ctx) else {
        return Ok(false);
    };
    match item {
        Expr::Variable(child_ident, ..)
            if ctx.scope.get(child_ident.1.as_str()).copied() == Some(ValueKind::Child) =>
        {
            out.push_str("    ");
            out.push_str(binding);
            out.push_str(".push(rh_child_share(&mut ");
            out.push_str(ctx.resolve_binding(child_ident.1.as_str()));
            out.push_str("));\n");
            Ok(true)
        }
        _ if expr_produces_child(item, ctx) => {
            out.push_str("    ");
            out.push_str(binding);
            out.push_str(".push(rh_child_share(&mut {");
            emit_expr(out, item, ctx)?;
            out.push_str("}));\n");
            Ok(true)
        }
        _ => Err(RhError::Transpile(format!(
            "child list push argument must be a Child binding (expr={item:?})"
        ))),
    }
}

/// Locals bound as `let name = []` that later `name.push(child)` where `child` is a
/// Child binding from `command.start()` (smoke `owned_children` pattern).
fn discover_empty_child_list_bindings(block: &StmtBlock, ctx: &EmitCtx) -> BTreeSet<String> {
    let mut empty_arrays = BTreeSet::new();
    let mut child_names = BTreeSet::new();
    collect_empty_array_and_child_bindings(block, &mut empty_arrays, &mut child_names, ctx);
    let mut result = BTreeSet::new();
    collect_child_list_pushes(block, &empty_arrays, &child_names, ctx, &mut result);
    result
}

fn discover_empty_string_list_bindings(block: &StmtBlock, ctx: &EmitCtx) -> BTreeSet<String> {
    let mut empty_arrays = BTreeSet::new();
    collect_empty_array_bindings(block, &mut empty_arrays);
    let mut result = BTreeSet::new();
    collect_string_list_pushes(block, &empty_arrays, ctx, &mut result);
    result
}

fn collect_empty_array_bindings(block: &StmtBlock, empty_arrays: &mut BTreeSet<String>) {
    for stmt in block.iter() {
        match stmt {
            Stmt::Var(boxed, ..) => {
                let (ident, expr, _) = boxed.as_ref();
                if matches!(expr, Expr::Array(items, ..) if items.is_empty()) {
                    empty_arrays.insert(ident.name.to_string());
                }
            }
            Stmt::If(boxed, ..) => {
                let flow = boxed.as_ref();
                collect_empty_array_bindings(&flow.body, empty_arrays);
                collect_empty_array_bindings(&flow.branch, empty_arrays);
            }
            Stmt::For(boxed, ..) => {
                collect_empty_array_bindings(&boxed.as_ref().2.body, empty_arrays);
            }
            Stmt::While(boxed, ..) => {
                collect_empty_array_bindings(&boxed.as_ref().body, empty_arrays);
            }
            Stmt::Block(inner) => collect_empty_array_bindings(inner, empty_arrays),
            Stmt::TryCatch(boxed, ..) => {
                let flow = boxed.as_ref();
                collect_empty_array_bindings(&flow.body, empty_arrays);
                collect_empty_array_bindings(&flow.branch, empty_arrays);
            }
            _ => {}
        }
    }
}

fn collect_string_list_pushes(
    block: &StmtBlock,
    empty_arrays: &BTreeSet<String>,
    ctx: &EmitCtx,
    result: &mut BTreeSet<String>,
) {
    for stmt in block.iter() {
        match stmt {
            Stmt::Expr(expr) => {
                if let Some((list, _)) = string_list_push_call(expr, ctx)
                    && empty_arrays.contains(list)
                {
                    result.insert(list.to_string());
                }
            }
            Stmt::If(boxed, ..) => {
                let flow = boxed.as_ref();
                collect_string_list_pushes(&flow.body, empty_arrays, ctx, result);
                collect_string_list_pushes(&flow.branch, empty_arrays, ctx, result);
            }
            Stmt::For(boxed, ..) => {
                collect_string_list_pushes(&boxed.as_ref().2.body, empty_arrays, ctx, result);
            }
            Stmt::While(boxed, ..) => {
                collect_string_list_pushes(&boxed.as_ref().body, empty_arrays, ctx, result);
            }
            Stmt::Block(inner) => collect_string_list_pushes(inner, empty_arrays, ctx, result),
            Stmt::TryCatch(boxed, ..) => {
                let flow = boxed.as_ref();
                collect_string_list_pushes(&flow.body, empty_arrays, ctx, result);
                collect_string_list_pushes(&flow.branch, empty_arrays, ctx, result);
            }
            _ => {}
        }
    }
}

fn is_empty_set_literal(expr: &Expr) -> bool {
    matches!(expr, Expr::Map(map, ..) if map.0.is_empty())
}

fn collect_empty_set_bindings(block: &StmtBlock, result: &mut BTreeSet<String>) {
    for stmt in block.iter() {
        match stmt {
            Stmt::Var(boxed, ..) => {
                let (ident, expr, _) = boxed.as_ref();
                if is_empty_set_literal(expr) {
                    result.insert(ident.name.to_string());
                }
            }
            Stmt::If(boxed, ..) => {
                let flow = boxed.as_ref();
                collect_empty_set_bindings(&flow.body, result);
                collect_empty_set_bindings(&flow.branch, result);
            }
            Stmt::For(boxed, ..) => {
                let (_, _, flow) = boxed.as_ref();
                collect_empty_set_bindings(&flow.body, result);
            }
            Stmt::While(boxed, ..) => {
                let flow = boxed.as_ref();
                collect_empty_set_bindings(&flow.body, result);
            }
            Stmt::Block(inner) => collect_empty_set_bindings(inner, result),
            Stmt::TryCatch(boxed, ..) => {
                let flow = boxed.as_ref();
                collect_empty_set_bindings(&flow.body, result);
                collect_empty_set_bindings(&flow.branch, result);
            }
            _ => {}
        }
    }
}

fn collect_set_map_assignments(
    block: &StmtBlock,
    empty_sets: &BTreeSet<String>,
    result: &mut BTreeSet<String>,
) {
    for stmt in block.iter() {
        match stmt {
            Stmt::Assignment(boxed, ..) => {
                let (op, bin) = boxed.as_ref();
                if op.get_op_assignment_info().is_some() {
                    continue;
                }
                let Expr::Index(index_box, ..) = &bin.lhs else {
                    continue;
                };
                let Expr::Variable(ident, ..) = &index_box.lhs else {
                    continue;
                };
                if !empty_sets.contains(ident.1.as_str()) {
                    continue;
                }
                if !matches!(bin.rhs, Expr::BoolConstant(true, ..)) {
                    result.insert(ident.1.to_string());
                }
            }
            Stmt::If(boxed, ..) => {
                let flow = boxed.as_ref();
                collect_set_map_assignments(&flow.body, empty_sets, result);
                collect_set_map_assignments(&flow.branch, empty_sets, result);
            }
            Stmt::For(boxed, ..) => {
                let (_, _, flow) = boxed.as_ref();
                collect_set_map_assignments(&flow.body, empty_sets, result);
            }
            Stmt::While(boxed, ..) => {
                let flow = boxed.as_ref();
                collect_set_map_assignments(&flow.body, empty_sets, result);
            }
            Stmt::Block(inner) => collect_set_map_assignments(inner, empty_sets, result),
            Stmt::TryCatch(boxed, ..) => {
                let flow = boxed.as_ref();
                collect_set_map_assignments(&flow.body, empty_sets, result);
                collect_set_map_assignments(&flow.branch, empty_sets, result);
            }
            _ => {}
        }
    }
}

/// Empty `#{}` bindings that store arbitrary JSON values under dynamic keys.
fn discover_set_map_bindings(block: &StmtBlock) -> BTreeSet<String> {
    let mut empty_sets = BTreeSet::new();
    collect_empty_set_bindings(block, &mut empty_sets);
    let mut result = BTreeSet::new();
    collect_set_map_assignments(block, &empty_sets, &mut result);
    result
}

fn is_json_map_binding(name: &str, kind: Option<ValueKind>, ctx: &EmitCtx) -> bool {
    matches!(kind, Some(ValueKind::Json)) || ctx.set_map_bindings.contains(name)
}

fn expr_is_command_start(expr: &Expr) -> bool {
    let Expr::Dot(boxed, ..) = expr else {
        return false;
    };
    matches!(
        &boxed.rhs,
        Expr::MethodCall(call, ..) if call.name == "start" && call.args.is_empty()
    )
}

fn expr_produces_child(expr: &Expr, ctx: &EmitCtx) -> bool {
    matches!(infer_binding_kind(expr, ctx), ValueKind::Child)
}

fn collect_empty_array_and_child_bindings(
    block: &StmtBlock,
    empty_arrays: &mut BTreeSet<String>,
    child_names: &mut BTreeSet<String>,
    ctx: &EmitCtx,
) {
    for stmt in block.iter() {
        match stmt {
            Stmt::Var(boxed, ..) => {
                let (ident, expr, _) = boxed.as_ref();
                if matches!(expr, Expr::Array(items, ..) if items.is_empty()) {
                    empty_arrays.insert(ident.name.to_string());
                }
                if expr_is_command_start(expr) || expr_produces_child(expr, ctx) {
                    child_names.insert(ident.name.to_string());
                }
            }
            Stmt::If(boxed, ..) => {
                let flow = boxed.as_ref();
                collect_empty_array_and_child_bindings(&flow.body, empty_arrays, child_names, ctx);
                collect_empty_array_and_child_bindings(
                    &flow.branch,
                    empty_arrays,
                    child_names,
                    ctx,
                );
            }
            Stmt::For(boxed, ..) => {
                let (_, _, flow) = boxed.as_ref();
                collect_empty_array_and_child_bindings(&flow.body, empty_arrays, child_names, ctx);
            }
            Stmt::While(boxed, ..) => {
                let flow = boxed.as_ref();
                collect_empty_array_and_child_bindings(&flow.body, empty_arrays, child_names, ctx);
            }
            Stmt::Block(inner) => {
                collect_empty_array_and_child_bindings(inner, empty_arrays, child_names, ctx);
            }
            Stmt::TryCatch(boxed, ..) => {
                let flow = boxed.as_ref();
                collect_empty_array_and_child_bindings(&flow.body, empty_arrays, child_names, ctx);
                collect_empty_array_and_child_bindings(
                    &flow.branch,
                    empty_arrays,
                    child_names,
                    ctx,
                );
            }
            _ => {}
        }
    }
}

fn collect_child_list_pushes(
    block: &StmtBlock,
    empty_arrays: &BTreeSet<String>,
    child_names: &BTreeSet<String>,
    ctx: &EmitCtx,
    result: &mut BTreeSet<String>,
) {
    for stmt in block.iter() {
        match stmt {
            Stmt::Expr(expr) => {
                if let Expr::Dot(boxed, ..) = expr.as_ref()
                    && let Expr::Variable(ident, ..) = &boxed.lhs
                    && empty_arrays.contains(ident.1.as_str())
                    && let Expr::MethodCall(call, ..) = &boxed.rhs
                    && call.name == "push"
                    && call.args.len() == 1
                    && call.args.len() == 1
                {
                    let ok = match &call.args[0] {
                        Expr::Variable(child_ident, ..) => {
                            child_names.contains(child_ident.1.as_str())
                        }
                        other => expr_produces_child(other, ctx),
                    };
                    if ok {
                        result.insert(ident.1.to_string());
                    }
                }
            }
            Stmt::If(boxed, ..) => {
                let flow = boxed.as_ref();
                collect_child_list_pushes(&flow.body, empty_arrays, child_names, ctx, result);
                collect_child_list_pushes(&flow.branch, empty_arrays, child_names, ctx, result);
            }
            Stmt::For(boxed, ..) => {
                let (_, _, flow) = boxed.as_ref();
                collect_child_list_pushes(&flow.body, empty_arrays, child_names, ctx, result);
            }
            Stmt::While(boxed, ..) => {
                let flow = boxed.as_ref();
                collect_child_list_pushes(&flow.body, empty_arrays, child_names, ctx, result);
            }
            Stmt::Block(inner) => {
                collect_child_list_pushes(inner, empty_arrays, child_names, ctx, result);
            }
            Stmt::TryCatch(boxed, ..) => {
                let flow = boxed.as_ref();
                collect_child_list_pushes(&flow.body, empty_arrays, child_names, ctx, result);
                collect_child_list_pushes(&flow.branch, empty_arrays, child_names, ctx, result);
            }
            _ => {}
        }
    }
}

fn is_json_bool_comparison(expr: &Expr, ctx: &EmitCtx) -> bool {
    let Expr::FnCall(call, ..) = expr else {
        return false;
    };
    let Some(op) = &call.op_token else {
        return false;
    };
    if call.args.len() != 2 || prefers_string_ops(&call.args[0], &call.args[1], ctx) {
        return false;
    }
    matches!(
        op,
        Token::Equals
            | Token::EqualsTo
            | Token::NotEqualsTo
            | Token::GreaterThan
            | Token::GreaterThanEqualsTo
            | Token::LessThan
            | Token::LessThanEqualsTo
    )
}

fn emit_json_bool_comparison(
    out: &mut String,
    expr: &Expr,
    ctx: &mut EmitCtx,
) -> Result<(), RhError> {
    let Expr::FnCall(call, ..) = expr else {
        return Err(RhError::Transpile(
            "emit_json_bool_comparison expected comparison call".into(),
        ));
    };
    let op = call.op_token.as_ref().expect("checked comparison");
    let rust_op = match op {
        Token::Equals | Token::EqualsTo => "==",
        Token::NotEqualsTo => "!=",
        Token::GreaterThan => ">",
        Token::GreaterThanEqualsTo => ">=",
        Token::LessThan => "<",
        Token::LessThanEqualsTo => "<=",
        _ => {
            return Err(RhError::Transpile(
                "emit_json_bool_comparison expected comparison operator".into(),
            ));
        }
    };
    out.push_str("serde_json::Value::Bool(");
    out.push('(');
    emit_intish(out, &call.args[0], ctx)?;
    out.push(' ');
    out.push_str(rust_op);
    out.push(' ');
    emit_intish(out, &call.args[1], ctx)?;
    out.push_str("))");
    Ok(())
}

fn command_method_call<'a>(
    expr: &'a Expr,
    ctx: &EmitCtx,
) -> Option<(&'a str, &'a rhai::FnCallExpr)> {
    command_binding_method(expr, ctx, ValueKind::Command)
}

fn output_method_call<'a>(
    expr: &'a Expr,
    ctx: &EmitCtx,
) -> Option<(&'a str, &'a rhai::FnCallExpr)> {
    command_binding_method(expr, ctx, ValueKind::Output)
}

fn child_method_call<'a>(expr: &'a Expr, ctx: &EmitCtx) -> Option<(&'a str, &'a rhai::FnCallExpr)> {
    command_binding_method(expr, ctx, ValueKind::Child)
}

fn stream_method_call<'a>(
    expr: &'a Expr,
    ctx: &EmitCtx,
) -> Option<(&'a str, &'a rhai::FnCallExpr)> {
    command_binding_method(expr, ctx, ValueKind::Stream)
}

fn bytes_method_call<'a>(expr: &'a Expr, ctx: &EmitCtx) -> Option<(&'a str, &'a rhai::FnCallExpr)> {
    command_binding_method(expr, ctx, ValueKind::Bytes)
}

fn window_control_method_call<'a>(
    expr: &'a Expr,
    ctx: &EmitCtx,
) -> Option<(&'a str, &'a rhai::FnCallExpr)> {
    command_binding_method(expr, ctx, ValueKind::WindowControl)
}

fn child_window_control_call<'a>(expr: &'a Expr, ctx: &EmitCtx) -> Option<&'a str> {
    let (binding, call) = child_method_call(expr, ctx)?;
    (call.name == "window_control" && call.args.len() == 1 && is_pure_int_expr(&call.args[0]))
        .then_some(binding)
}

fn child_window_client_rect_call<'a>(expr: &'a Expr, ctx: &EmitCtx) -> Option<&'a str> {
    let (binding, call) = child_method_call(expr, ctx)?;
    (call.name == "window_client_rect" && call.args.is_empty()).then_some(binding)
}

fn child_window_rect_call<'a>(expr: &'a Expr, ctx: &EmitCtx) -> Option<&'a str> {
    let (binding, call) = child_method_call(expr, ctx)?;
    (call.name == "window_rect" && call.args.is_empty()).then_some(binding)
}

fn window_control_property_binding<'a>(
    expr: &'a Expr,
    ctx: &EmitCtx,
) -> Option<(&'a str, &'a str)> {
    let Expr::Dot(boxed, ..) = expr else {
        return None;
    };
    let Expr::Variable(ident, ..) = &boxed.lhs else {
        return None;
    };
    if ctx.scope.get(ident.1.as_str()).copied() != Some(ValueKind::WindowControl) {
        return None;
    }
    let property = dot_property_name(&boxed.rhs)?;
    matches!(property, "visible" | "text").then_some((ident.1.as_str(), property))
}

fn window_rect_property_binding<'a>(expr: &'a Expr, ctx: &EmitCtx) -> Option<(&'a str, &'a str)> {
    let Expr::Dot(boxed, ..) = expr else {
        return None;
    };
    let Expr::Variable(ident, ..) = &boxed.lhs else {
        return None;
    };
    if ctx.scope.get(ident.1.as_str()).copied() != Some(ValueKind::WindowRect) {
        return None;
    }
    let property = dot_property_name(&boxed.rhs)?;
    matches!(property, "left" | "top" | "right" | "bottom").then_some((ident.1.as_str(), property))
}

fn command_output_call<'a>(expr: &'a Expr, ctx: &EmitCtx) -> Option<&'a str> {
    let (binding, call) = command_method_call(expr, ctx)?;
    (call.name == "output" && call.args.is_empty()).then_some(binding)
}

fn local_command_receiver_call<'a>(
    expr: &'a Expr,
    ctx: &EmitCtx,
    method: &str,
) -> Option<&'a rhai::FnCallExpr> {
    let Expr::Dot(boxed, ..) = expr else {
        return None;
    };
    let Expr::FnCall(receiver, ..) = &boxed.lhs else {
        return None;
    };
    if receiver.namespace.is_empty() {
        let resolved = resolve_local_fn_name(receiver.name.as_str(), ctx)?;
        if ctx.local_fn_return_kinds.get(resolved.as_str()).copied() != Some(ValueKind::Command) {
            return None;
        }
    } else {
        return None;
    }
    let Expr::MethodCall(call, ..) = &boxed.rhs else {
        return None;
    };
    (call.name == method && call.args.is_empty()).then_some(receiver)
}

fn command_start_call<'a>(expr: &'a Expr, ctx: &EmitCtx) -> Option<&'a str> {
    let (binding, call) = command_method_call(expr, ctx)?;
    (call.name == "start" && call.args.is_empty()).then_some(binding)
}

fn output_stdout_text_call<'a>(expr: &'a Expr, ctx: &EmitCtx) -> Option<&'a str> {
    let (binding, call) = output_method_call(expr, ctx)?;
    (call.name == "stdout_text" && call.args.is_empty()).then_some(binding)
}

fn output_stderr_text_call<'a>(expr: &'a Expr, ctx: &EmitCtx) -> Option<&'a str> {
    let (binding, call) = output_method_call(expr, ctx)?;
    (call.name == "stderr_text" && call.args.is_empty()).then_some(binding)
}

fn child_wait_with_output_call<'a>(expr: &'a Expr, ctx: &EmitCtx) -> Option<&'a str> {
    let (binding, call) = child_method_call(expr, ctx)?;
    (call.name == "wait_with_output" && call.args.len() == 1).then_some(binding)
}

fn output_property_binding<'a>(expr: &'a Expr, ctx: &EmitCtx) -> Option<(&'a str, &'a str)> {
    let Expr::Dot(boxed, ..) = expr else {
        return None;
    };
    let Expr::Variable(ident, ..) = &boxed.lhs else {
        return None;
    };
    if ctx.scope.get(ident.1.as_str()).copied() != Some(ValueKind::Output) {
        return None;
    }
    let property = dot_property_name(&boxed.rhs)?;
    matches!(property, "success" | "exit_code" | "stdout" | "stderr")
        .then_some((ident.1.as_str(), property))
}

fn child_property_binding<'a>(expr: &'a Expr, ctx: &EmitCtx) -> Option<(&'a str, &'a str)> {
    let Expr::Dot(boxed, ..) = expr else {
        return None;
    };
    let Expr::Variable(ident, ..) = &boxed.lhs else {
        return None;
    };
    if ctx.scope.get(ident.1.as_str()).copied() != Some(ValueKind::Child) {
        return None;
    }
    let property = dot_property_name(&boxed.rhs)?;
    matches!(
        property,
        "id" | "state" | "platform_facts" | "stdout" | "stderr"
    )
    .then_some((ident.1.as_str(), property))
}

fn child_state_binding<'a>(expr: &'a Expr, ctx: &EmitCtx) -> Option<&'a str> {
    child_property_binding(expr, ctx)
        .filter(|(_, property)| *property == "state")
        .map(|(binding, _)| binding)
}

fn bytes_property_binding<'a>(expr: &'a Expr, ctx: &EmitCtx) -> Option<(&'a str, &'a str)> {
    let Expr::Dot(boxed, ..) = expr else {
        return None;
    };
    let Expr::Variable(ident, ..) = &boxed.lhs else {
        return None;
    };
    if ctx.scope.get(ident.1.as_str()).copied() != Some(ValueKind::Bytes) {
        return None;
    }
    let property = dot_property_name(&boxed.rhs)?;
    (property == "len").then_some((ident.1.as_str(), property))
}

fn bytes_to_text_call<'a>(expr: &'a Expr, ctx: &EmitCtx) -> Option<&'a str> {
    let (binding, call) = bytes_method_call(expr, ctx)?;
    (call.name == "to_text" && call.args.is_empty()).then_some(binding)
}

fn emit_command_string_args(
    out: &mut String,
    arguments: &[Expr],
    ctx: &mut EmitCtx,
) -> Result<bool, RhError> {
    out.push_str("&[");
    for (index, argument) in arguments.iter().enumerate() {
        if index > 0 {
            out.push_str(", ");
        }
        let mut argument_expr = String::new();
        if !emit_native_string(&mut argument_expr, argument, ctx)? {
            return Ok(false);
        }
        out.push_str("String::from(");
        out.push_str(&argument_expr);
        out.push(')');
    }
    out.push(']');
    Ok(true)
}

fn emit_command_args_from(
    out: &mut String,
    binding: &str,
    arg: &Expr,
    ctx: &mut EmitCtx,
    indent: &str,
) -> Result<bool, RhError> {
    let Some(arguments) = process_arguments_arg(arg, ctx) else {
        return Ok(false);
    };
    out.push_str(indent);
    out.push_str("rh_command_args(&mut ");
    out.push_str(binding);
    out.push_str(", ");
    match arguments {
        ProcessArguments::Literal(items) => {
            if !emit_command_string_args(out, items, ctx)? {
                return Ok(false);
            }
        }
        ProcessArguments::StringList(arg_binding) => {
            out.push('&');
            out.push_str(arg_binding);
        }
        ProcessArguments::JsonArray(arg_binding) => {
            out.push_str("&rh_json_string_argv(&");
            out.push_str(arg_binding);
            out.push(')');
        }
    }
    out.push_str(");\n");
    Ok(true)
}

fn emit_duration_ms(out: &mut String, expr: &Expr, ctx: &mut EmitCtx) -> Result<bool, RhError> {
    if let Expr::FnCall(call, ..) = expr
        && call.namespace.to_string() == "std::time::Duration"
    {
        if call.name == "from_secs" && call.args.len() == 1 && is_pure_int_expr(&call.args[0]) {
            emit_expr(out, &call.args[0], ctx)?;
            out.push_str(" * 1000");
            return Ok(true);
        }
        if call.name == "from_millis" && call.args.len() == 1 && is_pure_int_expr(&call.args[0]) {
            emit_expr(out, &call.args[0], ctx)?;
            return Ok(true);
        }
    }
    if is_pure_int_expr(expr) {
        emit_expr(out, expr, ctx)?;
        return Ok(true);
    }
    Ok(false)
}

fn task_sleep_arg(expr: &Expr) -> Option<&Expr> {
    let Expr::FnCall(call, ..) = expr else {
        return None;
    };
    if is_host_api_call(call, "task", "sleep", 1) {
        Some(&call.args[0])
    } else {
        None
    }
}

fn emit_task_sleep_stmt(out: &mut String, expr: &Expr, ctx: &mut EmitCtx) -> Result<bool, RhError> {
    let Some(duration) = task_sleep_arg(expr) else {
        return Ok(false);
    };
    let mut ms = String::new();
    if !emit_duration_ms(&mut ms, duration, ctx)? {
        return Ok(false);
    }
    out.push_str("    std::thread::sleep(std::time::Duration::from_millis((");
    out.push_str(&ms);
    out.push_str(").max(0) as u64));\n");
    Ok(true)
}

fn emit_std_process_command(
    out: &mut String,
    program: &Expr,
    ctx: &mut EmitCtx,
) -> Result<bool, RhError> {
    let mut program_expr = String::new();
    if emit_native_string(&mut program_expr, program, ctx)? {
        out.push_str("rh_command_new(");
        out.push_str(&program_expr);
        out.push(')');
        return Ok(true);
    }
    if string_concat_args(program, ctx).is_some() || is_explicit_string_expr(program, ctx) {
        out.push_str("rh_command_new_owned(");
        emit_stringish(out, program, ctx)?;
        out.push(')');
        return Ok(true);
    }
    Ok(false)
}

fn emit_command_method(out: &mut String, expr: &Expr, ctx: &mut EmitCtx) -> Result<bool, RhError> {
    let Some((binding, call)) = command_method_call(expr, ctx) else {
        return Ok(false);
    };
    match call.name.as_str() {
        "output" if call.args.is_empty() => {
            out.push_str("rh_command_output(&mut ");
            out.push_str(binding);
            out.push(')');
            Ok(true)
        }
        "start" if call.args.is_empty() => {
            out.push_str("rh_command_start(&mut ");
            out.push_str(binding);
            out.push(')');
            Ok(true)
        }
        "args" if call.args.len() == 1 => {
            let mut block = String::new();
            if emit_command_args_from(&mut block, binding, &call.args[0], ctx, "        ")? {
                out.push_str("{\n");
                out.push_str(&block);
                out.push_str("        0\n    }");
                return Ok(true);
            }
            Ok(false)
        }
        "stdin_text" if call.args.len() == 1 => {
            out.push_str("{\n        rh_command_stdin_text(&mut ");
            out.push_str(binding);
            out.push_str(", &");
            emit_stringish(out, &call.args[0], ctx)?;
            out.push_str(");\n        0\n    }");
            Ok(true)
        }
        "arg" if call.args.len() == 1 => {
            out.push_str("{\n        rh_command_arg(&mut ");
            out.push_str(binding);
            out.push_str(", &");
            emit_stringish(out, &call.args[0], ctx)?;
            out.push_str(");\n        0\n    }");
            Ok(true)
        }
        "env" if call.args.len() == 2 => {
            out.push_str("{\n        rh_command_env(&mut ");
            out.push_str(binding);
            out.push_str(", &");
            emit_stringish(out, &call.args[0], ctx)?;
            out.push_str(", &");
            emit_stringish(out, &call.args[1], ctx)?;
            out.push_str(");\n        0\n    }");
            Ok(true)
        }
        "timeout" if call.args.len() == 1 => {
            out.push_str("{\n        rh_command_timeout_ms(&mut ");
            out.push_str(binding);
            out.push_str(", ");
            if !emit_duration_ms(out, &call.args[0], ctx)? {
                return Ok(false);
            }
            out.push_str(");\n        0\n    }");
            Ok(true)
        }
        "capture_limit" if call.args.len() == 1 && is_pure_int_expr(&call.args[0]) => {
            out.push_str("{\n        rh_command_capture_limit(&mut ");
            out.push_str(binding);
            out.push_str(", ");
            emit_expr(out, &call.args[0], ctx)?;
            out.push_str(");\n        0\n    }");
            Ok(true)
        }
        "current_dir" if call.args.len() == 1 => {
            out.push_str("{\n        rh_command_current_dir(&mut ");
            out.push_str(binding);
            out.push_str(", &");
            emit_stringish(out, &call.args[0], ctx)?;
            out.push_str(");\n        0\n    }");
            Ok(true)
        }
        "stdout_file" if call.args.len() == 1 => {
            out.push_str("{\n        rh_command_stdout_file(&mut ");
            out.push_str(binding);
            out.push_str(", &");
            emit_stringish(out, &call.args[0], ctx)?;
            out.push_str(");\n        0\n    }");
            Ok(true)
        }
        "stderr_file" if call.args.len() == 1 => {
            out.push_str("{\n        rh_command_stderr_file(&mut ");
            out.push_str(binding);
            out.push_str(", &");
            emit_stringish(out, &call.args[0], ctx)?;
            out.push_str(");\n        0\n    }");
            Ok(true)
        }
        "stderr_inherit" if call.args.is_empty() => {
            out.push_str("{\n        rh_command_stderr_inherit(&mut ");
            out.push_str(binding);
            out.push_str(");\n        0\n    }");
            Ok(true)
        }
        _ => Ok(false),
    }
}

fn emit_local_command_receiver_method(
    out: &mut String,
    expr: &Expr,
    ctx: &mut EmitCtx,
) -> Result<bool, RhError> {
    let (receiver, helper) =
        if let Some(receiver) = local_command_receiver_call(expr, ctx, "output") {
            (receiver, "rh_command_output")
        } else if let Some(receiver) = local_command_receiver_call(expr, ctx, "start") {
            (receiver, "rh_command_start")
        } else {
            return Ok(false);
        };
    out.push_str("{ let mut command = ");
    emit_call(out, receiver, ctx)?;
    out.push_str("; ");
    out.push_str(helper);
    out.push_str("(&mut command) }");
    Ok(true)
}

fn emit_output_method(out: &mut String, expr: &Expr, ctx: &mut EmitCtx) -> Result<bool, RhError> {
    let Some((binding, call)) = output_method_call(expr, ctx) else {
        return Ok(false);
    };
    match call.name.as_str() {
        "stdout_text" if call.args.is_empty() => {
            out.push_str("rh_output_stdout_text(&");
            out.push_str(binding);
            out.push(')');
            Ok(true)
        }
        "stderr_text" if call.args.is_empty() => {
            out.push_str("rh_output_stderr_text(&");
            out.push_str(binding);
            out.push(')');
            Ok(true)
        }
        "require_success" if call.args.len() == 1 => {
            out.push_str("rh_output_require_success(&");
            out.push_str(binding);
            out.push_str(", &");
            emit_stringish(out, &call.args[0], ctx)?;
            out.push(')');
            Ok(true)
        }
        _ => Ok(false),
    }
}

fn emit_child_method(out: &mut String, expr: &Expr, ctx: &mut EmitCtx) -> Result<bool, RhError> {
    let Some((binding, call)) = child_method_call(expr, ctx) else {
        return Ok(false);
    };
    match call.name.as_str() {
        "kill" if call.args.is_empty() => {
            out.push_str("rh_child_kill(&mut ");
            out.push_str(binding);
            out.push(')');
            Ok(true)
        }
        "wait_with_output" if call.args.len() == 1 => {
            out.push_str("rh_child_wait_with_output(&mut ");
            out.push_str(binding);
            out.push_str(", ");
            if !emit_duration_ms(out, &call.args[0], ctx)? {
                return Ok(false);
            }
            out.push(')');
            Ok(true)
        }
        "window_key" if call.args.len() == 1 => {
            out.push_str("rh_child_window_key(&mut ");
            out.push_str(binding);
            out.push_str(", &");
            emit_stringish(out, &call.args[0], ctx)?;
            out.push(')');
            Ok(true)
        }
        "window_control" if call.args.len() == 1 && is_pure_int_expr(&call.args[0]) => {
            out.push_str("rh_child_window_control(&mut ");
            out.push_str(binding);
            out.push_str(", ");
            emit_expr(out, &call.args[0], ctx)?;
            out.push(')');
            Ok(true)
        }
        "window_message" if call.args.len() == 3 => {
            out.push_str("rh_child_window_message(&mut ");
            out.push_str(binding);
            out.push_str(", ");
            emit_intish(out, &call.args[0], ctx)?;
            out.push_str(", ");
            emit_intish(out, &call.args[1], ctx)?;
            out.push_str(", ");
            emit_intish(out, &call.args[2], ctx)?;
            out.push(')');
            Ok(true)
        }
        "window_pointer" if call.args.len() == 3 => {
            out.push_str("rh_child_window_pointer(&mut ");
            out.push_str(binding);
            out.push_str(", &");
            emit_stringish(out, &call.args[0], ctx)?;
            out.push_str(", ");
            emit_intish(out, &call.args[1], ctx)?;
            out.push_str(", ");
            emit_intish(out, &call.args[2], ctx)?;
            out.push(')');
            Ok(true)
        }
        "window_resize" if call.args.len() == 2 => {
            out.push_str("rh_child_window_resize(&mut ");
            out.push_str(binding);
            out.push_str(", ");
            emit_intish(out, &call.args[0], ctx)?;
            out.push_str(", ");
            emit_intish(out, &call.args[1], ctx)?;
            out.push(')');
            Ok(true)
        }
        "window_client_rect" if call.args.is_empty() => {
            out.push_str("rh_child_window_rect(&mut ");
            out.push_str(binding);
            out.push_str(", true)");
            Ok(true)
        }
        "window_rect" if call.args.is_empty() => {
            out.push_str("rh_child_window_rect(&mut ");
            out.push_str(binding);
            out.push_str(", false)");
            Ok(true)
        }
        _ => Ok(false),
    }
}

fn emit_window_control_method(
    out: &mut String,
    expr: &Expr,
    ctx: &mut EmitCtx,
) -> Result<bool, RhError> {
    let Some((binding, call)) = window_control_method_call(expr, ctx) else {
        return Ok(false);
    };
    match call.name.as_str() {
        "click" if call.args.is_empty() => {
            out.push_str("rh_window_control_click(&mut ");
            out.push_str(binding);
            out.push(')');
            Ok(true)
        }
        "set_text" if call.args.len() == 1 => {
            out.push_str("rh_window_control_set_text(&mut ");
            out.push_str(binding);
            out.push_str(", &");
            emit_stringish(out, &call.args[0], ctx)?;
            out.push(')');
            Ok(true)
        }
        _ => Ok(false),
    }
}

fn emit_stream_method(out: &mut String, expr: &Expr, ctx: &mut EmitCtx) -> Result<bool, RhError> {
    let Some((binding, call)) = stream_method_call(expr, ctx) else {
        return Ok(false);
    };
    match call.name.as_str() {
        "read" if call.args.len() == 2 => {
            out.push_str("rh_stream_read(&mut ");
            out.push_str(binding);
            out.push_str(", ");
            emit_intish(out, &call.args[0], ctx)?;
            out.push_str(", ");
            if !emit_duration_ms(out, &call.args[1], ctx)? {
                return Ok(false);
            }
            out.push(')');
            Ok(true)
        }
        _ => Ok(false),
    }
}

fn emit_bytes_property(out: &mut String, expr: &Expr, ctx: &EmitCtx) -> Result<bool, RhError> {
    let Some((binding, property)) = bytes_property_binding(expr, ctx) else {
        return Ok(false);
    };
    debug_assert_eq!(property, "len");
    out.push_str("rh_bytes_len(&");
    out.push_str(binding);
    out.push(')');
    Ok(true)
}

fn emit_bytes_from_array(
    out: &mut String,
    items: &[Expr],
    ctx: &mut EmitCtx,
) -> Result<(), RhError> {
    out.push_str("rh_bytes_from_array(&[");
    for (index, item) in items.iter().enumerate() {
        if index > 0 {
            out.push_str(", ");
        }
        emit_expr(out, item, ctx)?;
    }
    out.push_str("])");
    Ok(())
}

fn emit_bytes_from_text(out: &mut String, text: &Expr, ctx: &mut EmitCtx) -> Result<bool, RhError> {
    out.push_str("rh_bytes_from_text(&");
    emit_stringish(out, text, ctx)?;
    out.push(')');
    Ok(true)
}

fn emit_bytes_value(out: &mut String, expr: &Expr, ctx: &mut EmitCtx) -> Result<bool, RhError> {
    if let Some(items) = bytes_from_array_items(expr) {
        emit_bytes_from_array(out, items, ctx)?;
        return Ok(true);
    }
    if let Some(text) = bytes_from_text_arg(expr) {
        return emit_bytes_from_text(out, text, ctx);
    }
    if let Expr::Variable(ident, ..) = expr
        && ctx.scope.get(ident.1.as_str()).copied() == Some(ValueKind::Bytes)
    {
        out.push_str(ident.1.as_str());
        out.push_str(".clone()");
        return Ok(true);
    }
    Ok(false)
}

fn emit_bytes_method(out: &mut String, expr: &Expr, ctx: &mut EmitCtx) -> Result<bool, RhError> {
    if let Some(binding) = bytes_to_text_call(expr, ctx) {
        out.push_str("rh_bytes_to_text(&");
        out.push_str(binding);
        out.push(')');
        return Ok(true);
    }
    let Some((binding, call)) = bytes_method_call(expr, ctx) else {
        return Ok(false);
    };
    if call.name != "append" || call.args.len() != 1 {
        return Ok(false);
    }
    let mut other = String::new();
    if !emit_bytes_value(&mut other, &call.args[0], ctx)? {
        return Ok(false);
    }
    out.push_str("{\n        rh_bytes_append(&mut ");
    out.push_str(binding);
    out.push_str(", &");
    out.push_str(&other);
    out.push_str(");\n        0\n    }");
    Ok(true)
}

fn emit_bytes_mut_stmt(out: &mut String, expr: &Expr, ctx: &mut EmitCtx) -> Result<bool, RhError> {
    let Some((binding, call)) = bytes_method_call(expr, ctx) else {
        return Ok(false);
    };
    if call.name != "append" || call.args.len() != 1 {
        return Ok(false);
    }
    let mut other = String::new();
    if !emit_bytes_value(&mut other, &call.args[0], ctx)? {
        return Ok(false);
    }
    out.push_str("    rh_bytes_append(&mut ");
    out.push_str(binding);
    out.push_str(", &");
    out.push_str(&other);
    out.push_str(");\n");
    Ok(true)
}

fn emit_command_mut_stmt(
    out: &mut String,
    expr: &Expr,
    ctx: &mut EmitCtx,
) -> Result<bool, RhError> {
    let Some((binding, call)) = command_method_call(expr, ctx) else {
        return Ok(false);
    };
    match call.name.as_str() {
        "args" if call.args.len() == 1 => {
            emit_command_args_from(out, binding, &call.args[0], ctx, "    ")
        }
        "stdin_text" if call.args.len() == 1 => {
            out.push_str("    rh_command_stdin_text(&mut ");
            out.push_str(binding);
            out.push_str(", &");
            emit_stringish(out, &call.args[0], ctx)?;
            out.push_str(");\n");
            Ok(true)
        }
        "arg" if call.args.len() == 1 => {
            out.push_str("    rh_command_arg(&mut ");
            out.push_str(binding);
            out.push_str(", &");
            emit_stringish(out, &call.args[0], ctx)?;
            out.push_str(");\n");
            Ok(true)
        }
        "env" if call.args.len() == 2 => {
            out.push_str("    rh_command_env(&mut ");
            out.push_str(binding);
            out.push_str(", &");
            emit_stringish(out, &call.args[0], ctx)?;
            out.push_str(", &");
            emit_stringish(out, &call.args[1], ctx)?;
            out.push_str(");\n");
            Ok(true)
        }
        "env_remove" if call.args.len() == 1 => {
            out.push_str("    rh_command_env_remove(&mut ");
            out.push_str(binding);
            out.push_str(", &");
            emit_stringish(out, &call.args[0], ctx)?;
            out.push_str(");\n");
            Ok(true)
        }
        "timeout" if call.args.len() == 1 => {
            out.push_str("    rh_command_timeout_ms(&mut ");
            out.push_str(binding);
            out.push_str(", ");
            if !emit_duration_ms(out, &call.args[0], ctx)? {
                return Ok(false);
            }
            out.push_str(");\n");
            Ok(true)
        }
        "capture_limit" if call.args.len() == 1 && is_pure_int_expr(&call.args[0]) => {
            out.push_str("    rh_command_capture_limit(&mut ");
            out.push_str(binding);
            out.push_str(", ");
            emit_expr(out, &call.args[0], ctx)?;
            out.push_str(");\n");
            Ok(true)
        }
        "current_dir" if call.args.len() == 1 => {
            out.push_str("    rh_command_current_dir(&mut ");
            out.push_str(binding);
            out.push_str(", &");
            emit_stringish(out, &call.args[0], ctx)?;
            out.push_str(");\n");
            Ok(true)
        }
        _ => Ok(false),
    }
}

fn emit_child_mut_stmt(out: &mut String, expr: &Expr, ctx: &mut EmitCtx) -> Result<bool, RhError> {
    let Some((binding, call)) = child_method_call(expr, ctx) else {
        return Ok(false);
    };
    match call.name.as_str() {
        "kill" if call.args.is_empty() => {
            out.push_str("    rh_child_kill(&mut ");
            out.push_str(binding);
            out.push_str(");\n");
            Ok(true)
        }
        "wait_with_output" if call.args.len() == 1 => {
            out.push_str("    ");
            if ctx.current_return_kind == ValueKind::Output {
                out.push_str("return rh_child_wait_with_output(&mut ");
            } else {
                out.push_str("let _ = rh_child_wait_with_output(&mut ");
            }
            out.push_str(ctx.resolve_binding(binding));
            out.push_str(", ");
            if !emit_duration_ms(out, &call.args[0], ctx)? {
                return Ok(false);
            }
            out.push_str(");\n");
            Ok(true)
        }
        "window_key" if call.args.len() == 1 => {
            out.push_str("    let _ = rh_child_window_key(&mut ");
            out.push_str(binding);
            out.push_str(", &");
            emit_stringish(out, &call.args[0], ctx)?;
            out.push_str(");\n");
            Ok(true)
        }
        "window_message" if call.args.len() == 3 => {
            out.push_str("    let _ = rh_child_window_message(&mut ");
            out.push_str(binding);
            out.push_str(", ");
            emit_intish(out, &call.args[0], ctx)?;
            out.push_str(", ");
            emit_intish(out, &call.args[1], ctx)?;
            out.push_str(", ");
            emit_intish(out, &call.args[2], ctx)?;
            out.push_str(");\n");
            Ok(true)
        }
        "window_pointer" if call.args.len() == 3 => {
            out.push_str("    let _ = rh_child_window_pointer(&mut ");
            out.push_str(binding);
            out.push_str(", &");
            emit_stringish(out, &call.args[0], ctx)?;
            out.push_str(", ");
            emit_intish(out, &call.args[1], ctx)?;
            out.push_str(", ");
            emit_intish(out, &call.args[2], ctx)?;
            out.push_str(");\n");
            Ok(true)
        }
        "window_resize" if call.args.len() == 2 => {
            out.push_str("    let _ = rh_child_window_resize(&mut ");
            out.push_str(binding);
            out.push_str(", ");
            emit_intish(out, &call.args[0], ctx)?;
            out.push_str(", ");
            emit_intish(out, &call.args[1], ctx)?;
            out.push_str(");\n");
            Ok(true)
        }
        _ => Ok(false),
    }
}

fn emit_window_control_mut_stmt(
    out: &mut String,
    expr: &Expr,
    ctx: &mut EmitCtx,
) -> Result<bool, RhError> {
    let Some((binding, call)) = window_control_method_call(expr, ctx) else {
        return Ok(false);
    };
    match call.name.as_str() {
        "click" if call.args.is_empty() => {
            out.push_str("    rh_window_control_click(&mut ");
            out.push_str(binding);
            out.push_str(");\n");
            Ok(true)
        }
        "set_text" if call.args.len() == 1 => {
            out.push_str("    rh_window_control_set_text(&mut ");
            out.push_str(binding);
            out.push_str(", &");
            emit_stringish(out, &call.args[0], ctx)?;
            out.push_str(");\n");
            Ok(true)
        }
        _ => Ok(false),
    }
}

fn emit_window_control_property(
    out: &mut String,
    expr: &Expr,
    ctx: &EmitCtx,
) -> Result<bool, RhError> {
    let Some((binding, property)) = window_control_property_binding(expr, ctx) else {
        return Ok(false);
    };
    if property == "visible" {
        out.push_str("rh_window_control_visible(&mut ");
        out.push_str(binding);
        out.push(')');
    } else {
        out.push_str("rh_window_control_text(&mut ");
        out.push_str(binding);
        out.push(')');
    }
    Ok(true)
}

fn emit_window_rect_property(
    out: &mut String,
    expr: &Expr,
    ctx: &EmitCtx,
) -> Result<bool, RhError> {
    let Some((binding, property)) = window_rect_property_binding(expr, ctx) else {
        return Ok(false);
    };
    out.push_str(binding);
    out.push('.');
    out.push_str(property);
    Ok(true)
}

fn emit_output_property(out: &mut String, expr: &Expr, ctx: &EmitCtx) -> Result<bool, RhError> {
    let Some((binding, property)) = output_property_binding(expr, ctx) else {
        return Ok(false);
    };
    out.push_str(binding);
    match property {
        "stdout" | "stderr" => {
            out.push('.');
            out.push_str(property);
            out.push_str(".clone()");
        }
        _ => {
            out.push('.');
            out.push_str(property);
        }
    }
    Ok(true)
}

fn emit_child_property(out: &mut String, expr: &Expr, ctx: &EmitCtx) -> Result<bool, RhError> {
    let Some((binding, property)) = child_property_binding(expr, ctx) else {
        return Ok(false);
    };
    let binding = ctx.resolve_binding(binding);
    if property == "state" {
        out.push_str("rh_child_state(&mut ");
        out.push_str(binding);
        out.push(')');
    } else if property == "platform_facts" {
        out.push_str("rh_child_platform_facts(&mut ");
        out.push_str(binding);
        out.push(')');
    } else if property == "stderr" {
        out.push_str("rh_child_stderr(&mut ");
        out.push_str(binding);
        out.push(')');
    } else if property == "stdout" {
        out.push_str("rh_child_stdout(&mut ");
        out.push_str(binding);
        out.push(')');
    } else if property == "id" {
        out.push_str(binding);
        out.push_str(".inner.borrow().pid");
    } else {
        out.push_str(binding);
        out.push('.');
        out.push_str(property);
    }
    Ok(true)
}

fn string_sub_string_arg<'a>(expr: &'a Expr, ctx: &EmitCtx) -> Option<&'a rhai::FnCallExpr> {
    let (_, call) = parse_string_method_call(expr, ctx)?;
    if call.name != "sub_string" || call.args.is_empty() || call.args.len() > 2 {
        return None;
    }
    if !is_pure_int_expr(&call.args[0])
        || call
            .args
            .get(1)
            .is_some_and(|argument| !is_pure_int_expr(argument))
    {
        return None;
    }
    Some(call)
}

fn split_separator_ok(expr: &Expr, ctx: &EmitCtx) -> bool {
    matches!(expr, Expr::StringConstant(..))
        || matches!(
            expr,
            Expr::Variable(ident, ..)
                if matches!(
                    ctx.scope.get(ident.1.as_str()).copied(),
                    Some(ValueKind::String | ValueKind::Path | ValueKind::Json)
                )
        )
        || string_concat_args(expr, ctx).is_some()
        || args_index_expr(expr).is_some()
        || json_value_path(expr, ctx).is_some_and(|(_, path)| !path.is_empty())
}

fn emit_split_separator(
    out: &mut String,
    separator: &Expr,
    ctx: &mut EmitCtx,
) -> Result<(), RhError> {
    let mut native = String::new();
    if emit_native_string(&mut native, separator, ctx)? {
        out.push_str(&native);
        return Ok(());
    }
    out.push('&');
    emit_stringish(out, separator, ctx)
}

type StringSplitParts<'a> = (Option<&'a Expr>, Option<(&'a str, Vec<&'a str>)>, &'a Expr);

/// `text.split(sep)` / `doc.field.split(sep)` — mirrors `json_contains_path` nesting.
fn string_split_parts<'a>(expr: &'a Expr, ctx: &EmitCtx) -> Option<StringSplitParts<'a>> {
    let Expr::Dot(boxed, ..) = expr else {
        return None;
    };
    // Shape A: Dot(receiver, MethodCall(split))
    if let Expr::MethodCall(call, ..) = &boxed.rhs {
        if call.name != "split" || call.args.len() != 1 || !split_separator_ok(&call.args[0], ctx) {
            return None;
        }
        if let Some((binding, path)) = json_value_path(&boxed.lhs, ctx) {
            return Some((None, Some((binding, path)), &call.args[0]));
        }
        let receiver_ok = match &boxed.lhs {
            Expr::Variable(ident, ..) => matches!(
                ctx.scope.get(ident.1.as_str()).copied(),
                Some(ValueKind::String | ValueKind::Path)
            ),
            Expr::StringConstant(..) => true,
            _ => {
                string_concat_args(&boxed.lhs, ctx).is_some()
                    || args_index_expr(&boxed.lhs).is_some()
                    || std_fs_read_to_string_arg(&boxed.lhs).is_some()
                    || std_env_get_arg(&boxed.lhs).is_some()
                    || crypto_sha256_file_arg(&boxed.lhs).is_some()
                    || json_stringify_pretty_arg(&boxed.lhs).is_some()
                    || json_stringify_arg(&boxed.lhs).is_some()
            }
        };
        return receiver_ok.then_some((Some(&boxed.lhs), None, &call.args[0]));
    }
    // Shape B: Dot(json_root, Dot(Property…, MethodCall(split)))
    let (binding, mut path) = json_value_path(&boxed.lhs, ctx)?;
    let sep = append_json_split(&boxed.rhs, &mut path)?;
    if !split_separator_ok(sep, ctx) {
        return None;
    }
    Some((None, Some((binding, path)), sep))
}

fn append_json_split<'a>(expr: &'a Expr, path: &mut Vec<&'a str>) -> Option<&'a Expr> {
    match expr {
        Expr::MethodCall(call, ..) if call.name == "split" && call.args.len() == 1 => {
            Some(&call.args[0])
        }
        Expr::Dot(boxed, ..) => {
            if !append_json_properties(&boxed.lhs, path) {
                return None;
            }
            append_json_split(&boxed.rhs, path)
        }
        _ => None,
    }
}

/// `text.split(sep).len` / `doc.field.split(sep).len`.
fn string_split_len_parts<'a>(expr: &'a Expr, ctx: &EmitCtx) -> Option<StringSplitParts<'a>> {
    let Expr::Dot(boxed, ..) = expr else {
        return None;
    };
    // Shape A: Dot(split_expr, Property(len))
    if is_len_property(&boxed.rhs) {
        return string_split_parts(&boxed.lhs, ctx);
    }
    // Shape B: Dot(json_root, Dot(Property…, Dot(MethodCall(split), len)))
    if let Some((binding, mut path)) = json_value_path(&boxed.lhs, ctx)
        && let Some(sep) = append_json_split_len(&boxed.rhs, &mut path)
        && split_separator_ok(sep, ctx)
    {
        return Some((None, Some((binding, path)), sep));
    }
    // Shape C: Dot(string_receiver, Dot(MethodCall(split), len))
    if let Expr::Dot(inner, ..) = &boxed.rhs
        && is_len_property(&inner.rhs)
        && let Expr::MethodCall(call, ..) = &inner.lhs
        && call.name == "split"
        && call.args.len() == 1
        && split_separator_ok(&call.args[0], ctx)
    {
        let receiver_ok = match &boxed.lhs {
            Expr::Variable(ident, ..) => matches!(
                ctx.scope.get(ident.1.as_str()).copied(),
                Some(ValueKind::String | ValueKind::Path)
            ),
            Expr::StringConstant(..) => true,
            _ => {
                string_concat_args(&boxed.lhs, ctx).is_some()
                    || args_index_expr(&boxed.lhs).is_some()
            }
        };
        if receiver_ok {
            return Some((Some(&boxed.lhs), None, &call.args[0]));
        }
    }
    None
}

fn append_json_split_len<'a>(expr: &'a Expr, path: &mut Vec<&'a str>) -> Option<&'a Expr> {
    match expr {
        Expr::Dot(boxed, ..) if is_len_property(&boxed.rhs) => append_json_split(&boxed.lhs, path),
        Expr::Dot(boxed, ..) => {
            if !append_json_properties(&boxed.lhs, path) {
                return None;
            }
            append_json_split_len(&boxed.rhs, path)
        }
        _ => None,
    }
}

fn emit_string_split_call(
    out: &mut String,
    receiver: Option<&Expr>,
    json: Option<(&str, Vec<&str>)>,
    separator: &Expr,
    ctx: &mut EmitCtx,
) -> Result<(), RhError> {
    out.push_str("rh_string_split(&");
    if let Some((binding, path)) = json {
        out.push_str("rh_json_string_path(&");
        out.push_str(binding);
        out.push_str(", ");
        emit_json_path(out, &path);
        out.push(')');
    } else if let Some(receiver) = receiver {
        emit_stringish(out, receiver, ctx)?;
    } else {
        return Err(RhError::Transpile("split receiver missing".into()));
    }
    out.push_str(", ");
    emit_split_separator(out, separator, ctx)?;
    out.push(')');
    Ok(())
}

fn is_len_property(expr: &Expr) -> bool {
    matches!(expr, Expr::Property(prop, ..) if prop.2.as_str() == "len")
        || matches!(
            expr,
            Expr::MethodCall(call, ..) if call.name == "len" && call.args.is_empty()
        )
}

/// Rhai parses `parts[0].len` as `parts[0.len]`; recover the intended element length.
fn string_list_index_misparse_len<'a>(expr: &'a Expr, ctx: &EmitCtx) -> Option<(&'a str, i64)> {
    let Expr::Index(boxed, ..) = expr else {
        return None;
    };
    let Expr::Variable(ident, ..) = &boxed.lhs else {
        return None;
    };
    if ctx.scope.get(ident.1.as_str()).copied() != Some(ValueKind::StringList) {
        return None;
    }
    let Expr::Dot(inner, ..) = &boxed.rhs else {
        return None;
    };
    let Expr::IntegerConstant(index, ..) = &inner.lhs else {
        return None;
    };
    is_len_property(&inner.rhs).then_some((ident.1.as_str(), *index))
}

fn string_list_index_rhs(rhs: &Expr) -> Option<&Expr> {
    if let Expr::Dot(inner, ..) = rhs
        && matches!(&inner.lhs, Expr::IntegerConstant(..))
        && is_len_property(&inner.rhs)
    {
        return None;
    }
    Some(rhs)
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
    let rhs = string_list_index_rhs(&boxed.rhs)?;
    Some((ident.1.as_str(), rhs))
}

fn child_list_index<'a>(expr: &'a Expr, ctx: &EmitCtx) -> Option<(&'a str, &'a Expr)> {
    let Expr::Index(boxed, ..) = expr else {
        return None;
    };
    let Expr::Variable(ident, ..) = &boxed.lhs else {
        return None;
    };
    if ctx.scope.get(ident.1.as_str()).copied() != Some(ValueKind::ChildList) {
        return None;
    }
    Some((ident.1.as_str(), &boxed.rhs))
}

fn is_stringish_array_item(expr: &Expr, ctx: &EmitCtx) -> bool {
    match expr {
        Expr::StringConstant(..) => true,
        Expr::Variable(ident, ..) => matches!(
            ctx.scope.get(ident.1.as_str()),
            Some(ValueKind::String | ValueKind::Path)
        ),
        _ => {
            json_value_path(expr, ctx).is_some()
                || path_join_display_args(expr).is_some()
                || path_parent_display_arg(expr).is_some()
                || path_absolute_display_arg(expr).is_some()
                || path_buf_from_display_arg(expr).is_some()
                || string_concat_args(expr, ctx).is_some()
                || args_index_expr(expr).is_some()
                || std_env_get_arg(expr).is_some()
                || dir_entry_path_display_binding(expr, ctx).is_some()
        }
    }
}

fn is_native_json_value_item(expr: &Expr, ctx: &EmitCtx) -> bool {
    match expr {
        Expr::StringConstant(..)
        | Expr::IntegerConstant(..)
        | Expr::BoolConstant(..)
        | Expr::Map(..)
        | Expr::Unit(..) => true,
        Expr::Array(items, ..) if items.is_empty() => true,
        Expr::Array(items, ..) => {
            !items.is_empty()
                && items
                    .iter()
                    .all(|item| is_native_json_value_item(item, ctx))
        }
        Expr::Variable(ident, ..) => matches!(
            ctx.scope.get(ident.1.as_str()),
            Some(
                ValueKind::Json
                    | ValueKind::String
                    | ValueKind::Bool
                    | ValueKind::Int
                    | ValueKind::Output
            )
        ),
        Expr::FnCall(call, ..)
            if call.op_token.is_none()
                && call.namespace.is_empty()
                && is_local_fn_call(call.name.as_str(), ctx) =>
        {
            true
        }
        _ => {
            json_parse_arg(expr).is_some()
                || json_value_path(expr, ctx).is_some()
                || json_array_index(expr, ctx).is_some()
                || json_path_array_index(expr, ctx).is_some()
                || string_concat_args(expr, ctx).is_some()
                || path_join_display_args(expr).is_some()
                || path_parent_display_arg(expr).is_some()
                || path_absolute_display_arg(expr).is_some()
                || path_buf_from_display_arg(expr).is_some()
                || path_buf_from_file_name_arg(expr).is_some()
                || std_env_get_arg(expr).is_some()
                || std_time_system_time_now_rfc3339(expr)
                || output_property_binding(expr, ctx).is_some()
                || output_stdout_text_call(expr, ctx).is_some()
                || output_stderr_text_call(expr, ctx).is_some()
                || std_process_id(expr)
                || std_time_system_time_now_unix_millis(expr)
                || is_pure_int_expr(expr)
                || is_json_bool_comparison(expr, ctx)
        }
    }
}

fn emit_json_array_value_literal(
    out: &mut String,
    items: &[Expr],
    ctx: &mut EmitCtx,
) -> Result<(), RhError> {
    out.push_str("serde_json::Value::Array(vec![");
    for (index, item) in items.iter().enumerate() {
        if index > 0 {
            out.push_str(", ");
        }
        emit_json_value_expr(out, item, ctx)?;
    }
    out.push_str("])");
    Ok(())
}

fn emit_string_list_index(
    out: &mut String,
    binding: &str,
    index: &Expr,
    ctx: &mut EmitCtx,
) -> Result<(), RhError> {
    out.push_str("rh_string_list_get(&");
    out.push_str(binding);
    out.push_str(", ");
    if let Some(value) = int_const(index) {
        out.push_str(&value.to_string());
    } else {
        emit_expr(out, index, ctx)?;
    }
    out.push(')');
    Ok(())
}

fn emit_string_list_index_misparse_len(
    out: &mut String,
    binding: &str,
    index: i64,
) -> Result<(), RhError> {
    out.push('(');
    out.push_str("rh_string_list_get(&");
    out.push_str(binding);
    out.push_str(", ");
    out.push_str(&index.to_string());
    out.push_str(").chars().count() as INT)");
    Ok(())
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
    if !is_json_array_index_key(&boxed.rhs, ctx) {
        return None;
    }
    Some((ident.1.as_str(), &boxed.rhs))
}

/// `obj.field.path[index]` — Rhai nests property Dots and terminates with Index:
/// `Dot { doc, Dot { nested, Index { Property(probe), 0 } } }` or
/// `Dot { doc, Index { Property(items), 0 } }`.
fn json_path_array_index<'a>(
    expr: &'a Expr,
    ctx: &EmitCtx,
) -> Option<(&'a str, Vec<&'a str>, &'a Expr)> {
    let Expr::Dot(outer, ..) = expr else {
        // Alternate shape: `(obj.field)[index]`.
        if let Expr::Index(boxed, ..) = expr {
            if matches!(&boxed.lhs, Expr::Variable(..)) {
                return None;
            }
            if !is_json_array_index_key(&boxed.rhs, ctx) {
                return None;
            }
            let (binding, path) = json_value_path(&boxed.lhs, ctx)?;
            if path.is_empty() {
                return None;
            }
            return Some((binding, path, &boxed.rhs));
        }
        return None;
    };
    let (binding, mut path) = json_value_path(&outer.lhs, ctx)?;
    let mut cursor = &outer.rhs;
    loop {
        match cursor {
            Expr::Index(index_box, ..) => {
                if !is_json_array_index_key(&index_box.rhs, ctx) {
                    return None;
                }
                if !append_json_properties(&index_box.lhs, &mut path) {
                    return None;
                }
                if path.is_empty() {
                    return None;
                }
                return Some((binding, path, &index_box.rhs));
            }
            Expr::Dot(inner, ..) => {
                if !append_json_properties(&inner.lhs, &mut path) {
                    return None;
                }
                cursor = &inner.rhs;
            }
            _ => return None,
        }
    }
}

enum JsonAssignTarget<'a> {
    Path {
        binding: &'a str,
        path: Vec<&'a str>,
    },
    PathKey {
        binding: &'a str,
        path: Vec<&'a str>,
        key: &'a Expr,
    },
    PathIndex {
        binding: &'a str,
        path: Vec<&'a str>,
        index: &'a Expr,
    },
    PathIndexField {
        binding: &'a str,
        path: Vec<&'a str>,
        index: &'a Expr,
        field: &'a str,
    },
}

fn is_json_array_index_key(expr: &Expr, ctx: &EmitCtx) -> bool {
    // `is_pure_int_expr` treats every Variable as int, and `is_native_json_int_expr`
    // also accepts String/Json locals (JSON numeric coercion helpers). Dynamic map
    // keys like `identities[id]` / `states[evidence_key]` must stay PathKey.
    match expr {
        Expr::IntegerConstant(..) | Expr::BoolConstant(..) => true,
        Expr::StringConstant(..) => false,
        Expr::Variable(ident, ..) => matches!(
            ctx.scope.get(ident.1.as_str()).copied(),
            Some(ValueKind::Int | ValueKind::Bool)
        ),
        Expr::FnCall(call, ..) if call.op_token.is_some() => call
            .args
            .iter()
            .all(|argument| is_json_array_index_key(argument, ctx)),
        // Definite int producers only. Do NOT use bare `is_native_json_int_expr`:
        // it treats every JSON field path as int, mis-classifying map keys like
        // `tabs[index.id]` / `results[gate_id]` as array indexes.
        Expr::Dot(..) | Expr::MethodCall(..) => {
            string_split_len_parts(expr, ctx).is_some()
                || json_array_len_path(expr, ctx).is_some()
                || json_object_keys_len_path(expr, ctx).is_some()
                || set_keys_len_path(expr, ctx).is_some()
                || std_time_system_time_now_unix_millis(expr)
                || std_process_id(expr)
                || fs_metadata_len_arg(expr).is_some()
                || dir_entry_metadata_len(expr, ctx).is_some()
                || metadata_property_binding(expr, ctx).is_some_and(|(_, name)| name == "len")
                || output_property_binding(expr, ctx).is_some()
                || system_time_unix_millis_binding(expr, ctx).is_some()
                || fs_metadata_modified_unix_millis(expr).is_some()
                || dir_entry_metadata_modified_unix_millis(expr, ctx).is_some()
        }
        _ => false,
    }
}

fn json_index_or_key_assign_target<'a>(
    binding: &'a str,
    path: Vec<&'a str>,
    key_or_index: &'a Expr,
    ctx: &EmitCtx,
) -> JsonAssignTarget<'a> {
    if is_json_array_index_key(key_or_index, ctx) {
        JsonAssignTarget::PathIndex {
            binding,
            path,
            index: key_or_index,
        }
    } else {
        JsonAssignTarget::PathKey {
            binding,
            path,
            key: key_or_index,
        }
    }
}

fn json_assign_value_path(expr: &Expr) -> Option<(&str, Vec<&str>)> {
    match expr {
        Expr::Variable(ident, ..) => Some((ident.1.as_str(), Vec::new())),
        Expr::Dot(boxed, ..) => {
            let (binding, mut path) = json_assign_value_path(&boxed.lhs)?;
            if !append_json_properties(&boxed.rhs, &mut path) {
                return None;
            }
            Some((binding, path))
        }
        _ => None,
    }
}

fn json_assign_target<'a>(lhs: &'a Expr, ctx: &EmitCtx) -> Option<JsonAssignTarget<'a>> {
    if let Expr::Index(boxed, ..) = lhs {
        // Rhai `arr[i].field = …` misparse is not a map-key assign.
        if json_rhai_array_index_property_from_index(boxed, ctx).is_some() {
            return None;
        }
        if let Expr::Variable(ident, ..) = &boxed.lhs {
            if is_json_map_binding(
                ident.1.as_str(),
                ctx.scope.get(ident.1.as_str()).copied(),
                ctx,
            ) {
                return Some(json_index_or_key_assign_target(
                    ident.1.as_str(),
                    Vec::new(),
                    &boxed.rhs,
                    ctx,
                ));
            }
            return None;
        }
        let (binding, path) = json_assign_value_path(&boxed.lhs)?;
        if path.is_empty() {
            return None;
        }
        return Some(json_index_or_key_assign_target(
            binding, path, &boxed.rhs, ctx,
        ));
    }
    let Expr::Dot(boxed, ..) = lhs else {
        return None;
    };
    if let Expr::Index(index_box, ..) = &boxed.rhs {
        let (binding, mut path) = json_assign_value_path(&boxed.lhs)?;
        if !append_json_properties(&index_box.lhs, &mut path) {
            return None;
        }
        if let Expr::Dot(inner, ..) = &index_box.rhs {
            let field = dot_property_name(&inner.rhs)?;
            return Some(JsonAssignTarget::PathIndexField {
                binding,
                path,
                index: &inner.lhs,
                field,
            });
        }
        return Some(json_index_or_key_assign_target(
            binding,
            path,
            &index_box.rhs,
            ctx,
        ));
    }
    if let Some((binding, path)) = json_assign_value_path(lhs)
        && !path.is_empty()
    {
        return Some(JsonAssignTarget::Path { binding, path });
    }
    None
}

fn emit_json_map_key(out: &mut String, key: &Expr, ctx: &mut EmitCtx) -> Result<(), RhError> {
    emit_set_key(out, key, ctx)
}

fn emit_string_list_assign_stmt(
    out: &mut String,
    assign: &(OpAssignment, BinaryExpr),
    ctx: &mut EmitCtx,
) -> Result<bool, RhError> {
    if assign.0.get_op_assignment_info().is_some() {
        return Ok(false);
    }
    let Expr::Index(index_box, ..) = &assign.1.lhs else {
        return Ok(false);
    };
    let Expr::Variable(ident, ..) = &index_box.lhs else {
        return Ok(false);
    };
    if ctx.scope.get(ident.1.as_str()).copied() != Some(ValueKind::StringList) {
        return Ok(false);
    }
    let mut value = String::new();
    if emit_stringish(&mut value, &assign.1.rhs, ctx).is_err() {
        return Ok(false);
    }
    out.push_str("    rh_string_list_set(&mut ");
    out.push_str(ident.1.as_str());
    out.push_str(", ");
    emit_intish(out, &index_box.rhs, ctx)?;
    out.push_str(", &");
    out.push_str(&value);
    out.push_str(");\n");
    Ok(true)
}

fn emit_json_assign_stmt(
    out: &mut String,
    assign: &(OpAssignment, BinaryExpr),
    ctx: &mut EmitCtx,
) -> Result<bool, RhError> {
    if let Some((target, rhs)) = json_path_int_plus_assign(assign, ctx) {
        let mut rhs_rust = String::new();
        if emit_intish(&mut rhs_rust, rhs, ctx).is_err() {
            return Ok(false);
        }
        let JsonAssignTarget::Path { binding, path } = target else {
            return Ok(false);
        };
        out.push_str("    let _ = rh_json_set_path(&mut ");
        out.push_str(binding);
        out.push_str(", ");
        emit_json_path(out, &path);
        out.push_str(", serde_json::json!(rh_json_int_path(&");
        out.push_str(binding);
        out.push_str(", ");
        emit_json_path(out, &path);
        out.push_str(") + ");
        out.push_str(&rhs_rust);
        out.push_str("));\n");
        return Ok(true);
    }
    if assign.0.get_op_assignment_info().is_some() {
        return Ok(false);
    }
    let Some(target) = json_assign_target(&assign.1.lhs, ctx) else {
        return Ok(false);
    };
    let mut value_rust = String::new();
    if emit_json_value_expr(&mut value_rust, &assign.1.rhs, ctx).is_err() {
        return Ok(false);
    }
    out.push_str("    let _ = ");
    match target {
        JsonAssignTarget::Path { binding, path } => {
            out.push_str("rh_json_set_path(&mut ");
            out.push_str(binding);
            out.push_str(", ");
            emit_json_path(out, &path);
            out.push_str(", ");
            out.push_str(&value_rust);
            out.push_str(");\n");
        }
        JsonAssignTarget::PathKey { binding, path, key } => {
            out.push_str("rh_json_set_path_key(&mut ");
            out.push_str(binding);
            out.push_str(", ");
            emit_json_path(out, &path);
            out.push_str(", &");
            emit_json_map_key(out, key, ctx)?;
            out.push_str(", ");
            out.push_str(&value_rust);
            out.push_str(");\n");
        }
        JsonAssignTarget::PathIndex {
            binding,
            path,
            index,
        } => {
            out.push_str("rh_json_set_path_index(&mut ");
            out.push_str(binding);
            out.push_str(", ");
            emit_json_path(out, &path);
            out.push_str(", ");
            emit_intish(out, index, ctx)?;
            out.push_str(", ");
            out.push_str(&value_rust);
            out.push_str(");\n");
        }
        JsonAssignTarget::PathIndexField {
            binding,
            path,
            index,
            field,
        } => {
            out.push_str("rh_json_set_path_index_field(&mut ");
            out.push_str(binding);
            out.push_str(", ");
            emit_json_path(out, &path);
            out.push_str(", ");
            emit_intish(out, index, ctx)?;
            out.push_str(", ");
            out.push_str(&format!("{field:?}"));
            out.push_str(", ");
            out.push_str(&value_rust);
            out.push_str(");\n");
        }
    }
    Ok(true)
}

fn json_path_int_plus_assign<'a>(
    assign: &'a (OpAssignment, BinaryExpr),
    ctx: &EmitCtx,
) -> Option<(JsonAssignTarget<'a>, &'a Expr)> {
    let (op, bin) = assign;
    let (_, _, _, syntax, _, _) = op.get_op_assignment_info()?;
    if syntax != "+=" {
        return None;
    }
    let target = json_assign_target(&bin.lhs, ctx)?;
    matches!(target, JsonAssignTarget::Path { .. }).then_some((target, &bin.rhs))
}

fn json_path_array_push_call<'a>(
    expr: &'a Expr,
    ctx: &EmitCtx,
) -> Option<(&'a str, Vec<&'a str>, &'a Expr)> {
    let Expr::Dot(boxed, ..) = expr else {
        return None;
    };
    let (binding, mut path) = json_value_path(&boxed.lhs, ctx)?;
    let item = append_json_push(&boxed.rhs, &mut path)?;
    Some((binding, path, item))
}

fn append_json_push<'a>(expr: &'a Expr, path: &mut Vec<&'a str>) -> Option<&'a Expr> {
    match expr {
        Expr::MethodCall(call, ..) if call.name == "push" && call.args.len() == 1 => {
            Some(&call.args[0])
        }
        Expr::Dot(boxed, ..) => {
            if !append_json_properties(&boxed.lhs, path) {
                return None;
            }
            append_json_push(&boxed.rhs, path)
        }
        _ => None,
    }
}

fn json_path_key_get<'a>(
    expr: &'a Expr,
    ctx: &EmitCtx,
) -> Option<(&'a str, Vec<&'a str>, &'a Expr)> {
    if let Expr::Index(boxed, ..) = expr {
        return json_path_key_get_index(boxed, ctx);
    }
    let Expr::Dot(boxed, ..) = expr else {
        return None;
    };
    let (binding, mut path) = json_value_path(&boxed.lhs, ctx)?;
    let Expr::Index(index_box, ..) = &boxed.rhs else {
        return None;
    };
    if !append_json_properties(&index_box.lhs, &mut path) {
        return None;
    }
    Some((binding, path, &index_box.rhs))
}

/// Rhai parses `arr[i].field` as `Index { arr, Dot { i, Property(field) } }`
/// (not `Dot { Index { arr, i }, field }`). Detect that shape when `i` is Int.
fn json_rhai_array_index_property<'a>(
    expr: &'a Expr,
    ctx: &EmitCtx,
) -> Option<(&'a str, Vec<&'a str>, &'a Expr, &'a str)> {
    let Expr::Index(boxed, ..) = expr else {
        return None;
    };
    json_rhai_array_index_property_from_index(boxed, ctx)
}

fn json_rhai_array_index_property_from_index<'a>(
    boxed: &'a BinaryExpr,
    ctx: &EmitCtx,
) -> Option<(&'a str, Vec<&'a str>, &'a Expr, &'a str)> {
    let Expr::Dot(inner, ..) = &boxed.rhs else {
        return None;
    };
    // Rhai may use a Variable int counter or a bare IntegerConstant (`arr[0].id`).
    let index_is_int = match &inner.lhs {
        Expr::IntegerConstant(..) | Expr::BoolConstant(..) => true,
        Expr::Variable(index, ..) => matches!(
            ctx.scope.get(index.1.as_str()).copied(),
            Some(ValueKind::Int | ValueKind::Bool)
        ),
        _ => false,
    };
    if !index_is_int {
        return None;
    }
    let field = dot_property_name(&inner.rhs)?;
    match &boxed.lhs {
        Expr::Variable(arr, ..)
            if ctx.scope.get(arr.1.as_str()).copied() == Some(ValueKind::Json)
                || ctx.set_map_bindings.contains(arr.1.as_str()) =>
        {
            Some((arr.1.as_str(), Vec::new(), &inner.lhs, field))
        }
        _ => {
            let (binding, path) = json_value_path(&boxed.lhs, ctx)?;
            if path.is_empty() {
                return None;
            }
            Some((binding, path, &inner.lhs, field))
        }
    }
}

fn emit_json_rhai_array_index_property_string(
    out: &mut String,
    binding: &str,
    path: &[&str],
    index: &Expr,
    field: &str,
    ctx: &mut EmitCtx,
) -> Result<(), RhError> {
    out.push_str("rh_json_string_path(&rh_json_get_path_index(&");
    out.push_str(binding);
    out.push_str(", ");
    emit_json_path(out, path);
    out.push_str(", ");
    emit_intish(out, index, ctx)?;
    out.push_str("), &[");
    out.push_str(&format!("{field:?}"));
    out.push_str("])");
    Ok(())
}

fn emit_json_rhai_array_index_property_int(
    out: &mut String,
    binding: &str,
    path: &[&str],
    index: &Expr,
    field: &str,
    ctx: &mut EmitCtx,
) -> Result<(), RhError> {
    out.push_str("rh_json_int_path(&rh_json_get_path_index(&");
    out.push_str(binding);
    out.push_str(", ");
    emit_json_path(out, path);
    out.push_str(", ");
    emit_intish(out, index, ctx)?;
    out.push_str("), &[");
    out.push_str(&format!("{field:?}"));
    out.push_str("])");
    Ok(())
}

fn json_path_key_get_index<'a>(
    boxed: &'a BinaryExpr,
    ctx: &EmitCtx,
) -> Option<(&'a str, Vec<&'a str>, &'a Expr)> {
    // Prefer the Rhai `arr[i].field` misparse handler over map-key treatment.
    if json_rhai_array_index_property_from_index(boxed, ctx).is_some() {
        return None;
    }
    if is_json_array_index_key(&boxed.rhs, ctx) {
        return None;
    }
    match &boxed.lhs {
        Expr::Variable(ident, ..)
            if is_json_map_binding(
                ident.1.as_str(),
                ctx.scope.get(ident.1.as_str()).copied(),
                ctx,
            ) =>
        {
            Some((ident.1.as_str(), Vec::new(), &boxed.rhs))
        }
        _ => {
            let (binding, path) = json_value_path(&boxed.lhs, ctx)?;
            if path.is_empty() {
                return None;
            }
            Some((binding, path, &boxed.rhs))
        }
    }
}

fn json_path_key_field_get<'a>(
    expr: &'a Expr,
    ctx: &EmitCtx,
) -> Option<(&'a str, Vec<&'a str>, &'a Expr, &'a str)> {
    let Expr::Dot(boxed, ..) = expr else {
        return None;
    };
    let field = dot_property_name(&boxed.rhs)?;
    let Expr::Index(index_box, ..) = &boxed.lhs else {
        return None;
    };
    if is_json_array_index_key(&index_box.rhs, ctx) {
        return None;
    }
    match &index_box.lhs {
        Expr::Variable(ident, ..)
            if is_json_map_binding(
                ident.1.as_str(),
                ctx.scope.get(ident.1.as_str()).copied(),
                ctx,
            ) =>
        {
            Some((ident.1.as_str(), Vec::new(), &index_box.rhs, field))
        }
        _ => {
            let (binding, path) = json_assign_value_path(&index_box.lhs)?;
            if path.is_empty() {
                return None;
            }
            Some((binding, path, &index_box.rhs, field))
        }
    }
}

fn emit_json_path_key_field_int(
    out: &mut String,
    binding: &str,
    path: &[&str],
    key: &Expr,
    field: &str,
    ctx: &mut EmitCtx,
) -> Result<(), RhError> {
    out.push_str("rh_json_int_path_key_field(&");
    out.push_str(binding);
    out.push_str(", ");
    emit_json_path(out, path);
    out.push_str(", &");
    emit_json_map_key(out, key, ctx)?;
    out.push_str(", ");
    out.push_str(&format!("{field:?}"));
    out.push(')');
    Ok(())
}

fn emit_json_path_key_get(
    out: &mut String,
    binding: &str,
    path: &[&str],
    key: &Expr,
    ctx: &mut EmitCtx,
) -> Result<(), RhError> {
    out.push_str("rh_json_get_path_key(&");
    out.push_str(binding);
    out.push_str(", ");
    emit_json_path(out, path);
    out.push_str(", &");
    emit_json_map_key(out, key, ctx)?;
    out.push(')');
    Ok(())
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

fn symlink_metadata_property(expr: &Expr) -> Option<(&Expr, &str)> {
    fs_metadata_property(expr, "symlink_metadata")
}

fn metadata_call_property(expr: &Expr) -> Option<(&Expr, &str)> {
    fs_metadata_property(expr, "metadata")
}

fn fs_metadata_property<'a>(expr: &'a Expr, call_name: &str) -> Option<(&'a Expr, &'a str)> {
    let Expr::Dot(boxed, ..) = expr else {
        return None;
    };
    let path = std_fs_single_arg(&boxed.lhs, call_name)?;
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

fn dir_entry_field_name(rhs: &Expr) -> Option<&str> {
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

fn system_time_property_binding<'a>(expr: &'a Expr, ctx: &EmitCtx) -> Option<(&'a str, &'a str)> {
    let Expr::Dot(boxed, ..) = expr else {
        return None;
    };
    let Expr::Variable(ident, ..) = &boxed.lhs else {
        return None;
    };
    if ctx.scope.get(ident.1.as_str()).copied() != Some(ValueKind::SystemTime) {
        return None;
    }
    let name = match &boxed.rhs {
        Expr::Property(property, ..) => property.2.as_str(),
        Expr::MethodCall(call, ..) if call.args.is_empty() => call.name.as_str(),
        _ => return None,
    };
    matches!(name, "unix_millis" | "rfc3339").then_some((ident.1.as_str(), name))
}

fn dot_property_name(rhs: &Expr) -> Option<&str> {
    match rhs {
        Expr::Property(property, ..) => Some(property.2.as_str()),
        Expr::MethodCall(call, ..) if call.args.is_empty() => Some(call.name.as_str()),
        _ => None,
    }
}

fn dir_entry_metadata_binding<'a>(expr: &'a Expr, ctx: &EmitCtx) -> Option<&'a str> {
    let binding = dir_entry_variable(expr, ctx)?;
    let Expr::Dot(boxed, ..) = expr else {
        return None;
    };
    (dot_property_name(&boxed.rhs)? == "metadata").then_some(binding)
}

fn metadata_modified_binding<'a>(expr: &'a Expr, ctx: &EmitCtx) -> Option<&'a str> {
    let Expr::Dot(boxed, ..) = expr else {
        return None;
    };
    let Expr::Variable(ident, ..) = &boxed.lhs else {
        return None;
    };
    if ctx.scope.get(ident.1.as_str()).copied() != Some(ValueKind::Metadata) {
        return None;
    }
    (dot_property_name(&boxed.rhs)? == "modified").then_some(ident.1.as_str())
}

fn system_time_binding<'a>(expr: &'a Expr, ctx: &EmitCtx) -> Option<&'a str> {
    let Expr::Dot(boxed, ..) = expr else {
        return None;
    };
    let Expr::Variable(ident, ..) = &boxed.lhs else {
        return None;
    };
    (ctx.scope.get(ident.1.as_str()).copied() == Some(ValueKind::SystemTime))
        .then_some(ident.1.as_str())
}

fn system_time_unix_millis_binding<'a>(expr: &'a Expr, ctx: &EmitCtx) -> Option<&'a str> {
    let binding = system_time_binding(expr, ctx)?;
    let Expr::Dot(boxed, ..) = expr else {
        return None;
    };
    (dot_property_name(&boxed.rhs)? == "unix_millis").then_some(binding)
}

fn system_time_rfc3339_binding<'a>(expr: &'a Expr, ctx: &EmitCtx) -> Option<&'a str> {
    let binding = system_time_binding(expr, ctx)?;
    let Expr::Dot(boxed, ..) = expr else {
        return None;
    };
    (dot_property_name(&boxed.rhs)? == "rfc3339").then_some(binding)
}

fn dir_entry_metadata_len<'a>(expr: &'a Expr, ctx: &EmitCtx) -> Option<&'a str> {
    let Expr::Dot(outer, ..) = expr else {
        return None;
    };
    let Expr::Dot(inner, ..) = &outer.lhs else {
        return None;
    };
    let binding = dir_entry_variable(&inner.lhs, ctx)?;
    if dot_property_name(&inner.rhs)? != "metadata" {
        return None;
    }
    (dot_property_name(&outer.rhs)? == "len").then_some(binding)
}

fn dir_entry_metadata_modified_unix_millis<'a>(expr: &'a Expr, ctx: &EmitCtx) -> Option<&'a str> {
    let Expr::Dot(outer, ..) = expr else {
        return None;
    };
    let Expr::Dot(inner, ..) = &outer.lhs else {
        return None;
    };
    let Expr::Dot(meta, ..) = &inner.lhs else {
        return None;
    };
    let binding = dir_entry_variable(&meta.lhs, ctx)?;
    if dot_property_name(&meta.rhs)? != "metadata" || dot_property_name(&inner.rhs)? != "modified" {
        return None;
    }
    (dot_property_name(&outer.rhs)? == "unix_millis").then_some(binding)
}

fn dir_entry_metadata_modified_rfc3339<'a>(expr: &'a Expr, ctx: &EmitCtx) -> Option<&'a str> {
    let Expr::Dot(outer, ..) = expr else {
        return None;
    };
    let Expr::Dot(inner, ..) = &outer.lhs else {
        return None;
    };
    let Expr::Dot(meta, ..) = &inner.lhs else {
        return None;
    };
    let binding = dir_entry_variable(&meta.lhs, ctx)?;
    if dot_property_name(&meta.rhs)? != "metadata" || dot_property_name(&inner.rhs)? != "modified" {
        return None;
    }
    (dot_property_name(&outer.rhs)? == "rfc3339").then_some(binding)
}

fn fs_metadata_modified_arg(expr: &Expr) -> Option<(&Expr, &str)> {
    let Expr::Dot(boxed, ..) = expr else {
        return None;
    };
    if dot_property_name(&boxed.rhs)? != "modified" {
        return None;
    }
    if let Some(path) = std_fs_metadata_arg(&boxed.lhs) {
        return Some((path, "metadata"));
    }
    if let Some(path) = std_fs_symlink_metadata_arg(&boxed.lhs) {
        return Some((path, "symlink_metadata"));
    }
    None
}

fn fs_metadata_len_arg(expr: &Expr) -> Option<(&Expr, &str)> {
    let Expr::Dot(boxed, ..) = expr else {
        return None;
    };
    if dot_property_name(&boxed.rhs)? != "len" {
        return None;
    }
    if let Some(path) = std_fs_metadata_arg(&boxed.lhs) {
        return Some((path, "metadata"));
    }
    if let Some(path) = std_fs_symlink_metadata_arg(&boxed.lhs) {
        return Some((path, "symlink_metadata"));
    }
    None
}

fn fs_metadata_modified_unix_millis(expr: &Expr) -> Option<(&Expr, &str)> {
    let Expr::Dot(outer, ..) = expr else {
        return None;
    };
    let Expr::Dot(inner, ..) = &outer.lhs else {
        return None;
    };
    let (path, call_name) = if let Some(path) = std_fs_metadata_arg(&inner.lhs) {
        (path, "metadata")
    } else {
        let path = std_fs_symlink_metadata_arg(&inner.lhs)?;
        (path, "symlink_metadata")
    };
    if dot_property_name(&inner.rhs)? != "modified"
        || dot_property_name(&outer.rhs)? != "unix_millis"
    {
        return None;
    }
    Some((path, call_name))
}

fn fs_metadata_modified_rfc3339(expr: &Expr) -> Option<(&Expr, &str)> {
    let Expr::Dot(outer, ..) = expr else {
        return None;
    };
    let Expr::Dot(inner, ..) = &outer.lhs else {
        return None;
    };
    let (path, call_name) = if let Some(path) = std_fs_metadata_arg(&inner.lhs) {
        (path, "metadata")
    } else {
        let path = std_fs_symlink_metadata_arg(&inner.lhs)?;
        (path, "symlink_metadata")
    };
    if dot_property_name(&inner.rhs)? != "modified" || dot_property_name(&outer.rhs)? != "rfc3339" {
        return None;
    }
    Some((path, call_name))
}

fn metadata_modified_property_binding<'a>(
    expr: &'a Expr,
    ctx: &EmitCtx,
) -> Option<(&'a str, &'a str)> {
    let Expr::Dot(outer, ..) = expr else {
        return None;
    };
    let Expr::Dot(inner, ..) = &outer.lhs else {
        return None;
    };
    let Expr::Variable(ident, ..) = &inner.lhs else {
        return None;
    };
    if ctx.scope.get(ident.1.as_str()).copied() != Some(ValueKind::Metadata) {
        return None;
    }
    if dot_property_name(&inner.rhs)? != "modified" {
        return None;
    }
    let property = dot_property_name(&outer.rhs)?;
    matches!(property, "unix_millis" | "rfc3339").then_some((ident.1.as_str(), property))
}

enum ProcessArguments<'a> {
    Literal(&'a [Expr]),
    StringList(&'a str),
    JsonArray(&'a str),
}

fn process_command_argv_arg_index(call: &rhai::FnCallExpr) -> Option<usize> {
    if call.namespace.to_string() != "std::process" {
        return None;
    }
    match call.name.as_str() {
        "command_status" if (3..=4).contains(&call.args.len()) => Some(1),
        "command_stdout_file" if (4..=5).contains(&call.args.len()) => Some(1),
        _ => None,
    }
}

fn process_command_argv_param_upgrade(
    call: &rhai::FnCallExpr,
    def: &ScriptFuncDef,
) -> Option<(usize, ValueKind)> {
    let argv_index = process_command_argv_arg_index(call)?;
    let Expr::Variable(ident, ..) = &call.args[argv_index] else {
        return None;
    };
    let param_index = def
        .params
        .iter()
        .position(|param| param.as_str() == ident.1.as_str())?;
    Some((param_index, ValueKind::StringList))
}

fn process_arguments_arg<'a>(expr: &'a Expr, ctx: &EmitCtx) -> Option<ProcessArguments<'a>> {
    match expr {
        Expr::Array(items, ..) => Some(ProcessArguments::Literal(items)),
        Expr::Variable(ident, ..) => match ctx.scope.get(ident.1.as_str()).copied()? {
            ValueKind::StringList => Some(ProcessArguments::StringList(ident.1.as_str())),
            ValueKind::Json => Some(ProcessArguments::JsonArray(ident.1.as_str())),
            _ => None,
        },
        _ => None,
    }
}

fn process_status_args<'a>(
    expr: &'a Expr,
    ctx: &EmitCtx,
) -> Option<(&'a Expr, ProcessArguments<'a>, &'a Expr, Option<&'a Expr>)> {
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
    let arguments = process_arguments_arg(&call.args[1], ctx)?;
    Some((&call.args[0], arguments, &call.args[2], options))
}

fn process_stdout_file_args<'a>(
    expr: &'a Expr,
    ctx: &EmitCtx,
) -> Option<(
    &'a Expr,
    ProcessArguments<'a>,
    &'a Expr,
    &'a Expr,
    Option<&'a Expr>,
)> {
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
    let arguments = process_arguments_arg(&call.args[1], ctx)?;
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
    if !is_host_api_call(call, "json", "parse", 1) {
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

fn child_platform_facts_path<'a>(expr: &'a Expr, ctx: &EmitCtx) -> Option<(&'a str, Vec<&'a str>)> {
    let Expr::Dot(boxed, ..) = expr else {
        return None;
    };
    let Expr::Variable(ident, ..) = &boxed.lhs else {
        return None;
    };
    if ctx.scope.get(ident.1.as_str()).copied() != Some(ValueKind::Child) {
        return None;
    }
    let mut path = Vec::new();
    if !append_json_properties(&boxed.rhs, &mut path)
        || path.first().copied() != Some("platform_facts")
    {
        return None;
    }
    path.remove(0);
    (!path.is_empty()).then_some((ident.1.as_str(), path))
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

/// `doc.path.contains(needle)` for JSON string substring or array membership.
///
/// Rhai may attach `.contains` either as the outermost method:
/// `Dot(json_path, MethodCall(contains))`, or nest properties under the rhs:
/// `Dot(root, Dot(Property…, MethodCall(contains)))`.
fn json_contains_path<'a>(
    expr: &'a Expr,
    ctx: &EmitCtx,
) -> Option<(&'a str, Vec<&'a str>, &'a Expr)> {
    let Expr::Dot(boxed, ..) = expr else {
        return None;
    };
    let (binding, mut path) = json_value_path(&boxed.lhs, ctx)?;
    let needle = append_json_contains(&boxed.rhs, &mut path)?;
    Some((binding, path, needle))
}

fn append_json_contains<'a>(expr: &'a Expr, path: &mut Vec<&'a str>) -> Option<&'a Expr> {
    match expr {
        Expr::MethodCall(call, ..) if call.name == "contains" && call.args.len() == 1 => {
            Some(&call.args[0])
        }
        Expr::Dot(boxed, ..) => {
            if !append_json_properties(&boxed.lhs, path) {
                return None;
            }
            append_json_contains(&boxed.rhs, path)
        }
        _ => None,
    }
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

fn is_keys_method_rhs(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::MethodCall(call, ..) if call.name == "keys" && call.args.is_empty()
    )
}

fn is_len_method_or_property(expr: &Expr) -> bool {
    matches!(expr, Expr::Property(prop, ..) if prop.2.as_str() == "len")
        || matches!(
            expr,
            Expr::MethodCall(call, ..) if call.name == "len" && call.args.is_empty()
        )
}

fn json_object_keys_path<'a>(expr: &'a Expr, ctx: &EmitCtx) -> Option<(&'a str, Vec<&'a str>)> {
    let Expr::Dot(boxed, ..) = expr else {
        return None;
    };
    if !is_keys_method_rhs(&boxed.rhs) {
        return None;
    }
    json_value_path(&boxed.lhs, ctx)
}

fn json_object_keys_len_path<'a>(expr: &'a Expr, ctx: &EmitCtx) -> Option<(&'a str, Vec<&'a str>)> {
    let Expr::Dot(outer, ..) = expr else {
        return None;
    };
    let (binding, path) = json_value_path(&outer.lhs, ctx)?;
    let Expr::Dot(inner, ..) = &outer.rhs else {
        return None;
    };
    if !is_keys_method_rhs(&inner.lhs) || !is_len_method_or_property(&inner.rhs) {
        return None;
    }
    Some((binding, path))
}

fn set_keys_for_path<'a>(expr: &'a Expr, ctx: &EmitCtx) -> Option<&'a str> {
    let Expr::Dot(boxed, ..) = expr else {
        return None;
    };
    if !is_keys_method_rhs(&boxed.rhs) {
        return None;
    }
    let Expr::Variable(ident, ..) = &boxed.lhs else {
        return None;
    };
    (ctx.scope.get(ident.1.as_str()) == Some(&ValueKind::Set)).then_some(ident.1.as_str())
}

fn set_keys_len_path<'a>(expr: &'a Expr, ctx: &EmitCtx) -> Option<&'a str> {
    let Expr::Dot(outer, ..) = expr else {
        return None;
    };
    let Expr::Variable(ident, ..) = &outer.lhs else {
        return None;
    };
    if ctx.scope.get(ident.1.as_str()) != Some(&ValueKind::Set) {
        return None;
    }
    let Expr::Dot(inner, ..) = &outer.rhs else {
        return None;
    };
    if !is_keys_method_rhs(&inner.lhs) || !is_len_method_or_property(&inner.rhs) {
        return None;
    }
    Some(ident.1.as_str())
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
        || path_parent_display_arg(expr).is_some()
        || path_absolute_display_arg(expr).is_some()
        || path_buf_from_display_arg(expr).is_some()
        || path_buf_from_file_name_arg(expr).is_some()
        || path_binding_display(expr, ctx).is_some()
        || path_binding_file_name(expr, ctx).is_some()
        || env_current_dir_display(expr)
        || std_fs_read_to_string_arg(expr).is_some()
        || crypto_sha256_file_arg(expr).is_some()
        || hash_fnv1a64_arg(expr).is_some()
        || json_stringify_pretty_arg(expr).is_some()
        || json_stringify_arg(expr).is_some()
        || string_sub_string_arg(expr, ctx).is_some()
        || string_list_index(expr, ctx).is_some()
        || json_path_array_index(expr, ctx).is_some()
        || dir_entry_string_field(expr, ctx).is_some()
        || std_time_system_time_now_rfc3339(expr)
        || system_time_rfc3339_binding(expr, ctx).is_some()
        || fs_metadata_modified_rfc3339(expr).is_some()
        || dir_entry_metadata_modified_rfc3339(expr, ctx).is_some()
        || child_state_binding(expr, ctx).is_some()
        || output_stdout_text_call(expr, ctx).is_some()
        || output_stderr_text_call(expr, ctx).is_some()
        || parse_string_method_call(expr, ctx).is_some_and(|(_, call)| {
            call.args.is_empty() && matches!(call.name.as_str(), "to_lower" | "trim" | "to_string")
        })
        || string_method_on_path_display(expr, ctx).is_some()
        || (json_value_path(expr, ctx).is_some_and(|(_, path)| !path.is_empty())
            && json_array_len_path(expr, ctx).is_none()
            && !is_var_len_expr(expr))
        || matches!(
            expr,
            Expr::Variable(ident, ..)
                if ctx.scope.get(ident.1.as_str()).copied() == Some(ValueKind::Path)
        )
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
    // JSON/array `.len` is an INT surface; never force string compare for it.
    // Note: `is_pure_int_expr(Variable)` is always true, so do not use it here
    // as a "both sides int" short-circuit — that regresses String variable ops.
    if json_array_len_path(lhs, ctx).is_some()
        || json_array_len_path(rhs, ctx).is_some()
        || json_object_keys_len_path(lhs, ctx).is_some()
        || json_object_keys_len_path(rhs, ctx).is_some()
        || is_var_len_expr(lhs)
        || is_var_len_expr(rhs)
    {
        return false;
    }
    // String literals always select the string lane (`"" + doc.field`, `x == "measured"`).
    if matches!(lhs, Expr::StringConstant(..)) || matches!(rhs, Expr::StringConstant(..)) {
        return true;
    }
    // JSON field + INT-typed operand must stay arithmetic. `is_explicit_string_expr`
    // treats every non-empty JSON path as stringish, which otherwise turns
    // `bootstrap.setup_ms + task_ms` into format! and poisons return kinds.
    if (json_field_path(lhs, ctx) && is_int_typed_operand(rhs, ctx))
        || (json_field_path(rhs, ctx) && is_int_typed_operand(lhs, ctx))
    {
        return false;
    }
    is_explicit_string_expr(lhs, ctx) || is_explicit_string_expr(rhs, ctx)
}

fn json_field_path(expr: &Expr, ctx: &EmitCtx) -> bool {
    json_value_path(expr, ctx).is_some_and(|(_, path)| !path.is_empty())
        && json_array_len_path(expr, ctx).is_none()
        && !is_var_len_expr(expr)
}

/// Operand that should keep `+` with a JSON field in the INT lane.
fn is_int_typed_operand(expr: &Expr, ctx: &EmitCtx) -> bool {
    matches!(expr, Expr::IntegerConstant(..))
        || matches!(
            expr,
            Expr::Variable(ident, ..)
                if matches!(
                    ctx.scope.get(ident.1.as_str()).copied(),
                    Some(ValueKind::Int | ValueKind::Bool)
                )
        )
        || json_field_path(expr, ctx)
}

fn is_native_json_int_expr(expr: &Expr, ctx: &EmitCtx) -> bool {
    if string_split_len_parts(expr, ctx).is_some()
        || json_array_len_path(expr, ctx).is_some()
        || json_object_keys_len_path(expr, ctx).is_some()
        || set_keys_len_path(expr, ctx).is_some()
        || json_contains_path(expr, ctx).is_some()
        || std_time_system_time_now_unix_millis(expr)
        || std_process_id(expr)
        || output_property_binding(expr, ctx).is_some()
        || child_property_binding(expr, ctx).is_some_and(|(_, property)| property == "id")
        || system_time_unix_millis_binding(expr, ctx).is_some()
        || fs_metadata_modified_unix_millis(expr).is_some()
        || dir_entry_metadata_modified_unix_millis(expr, ctx).is_some()
        || dir_entry_metadata_len(expr, ctx).is_some()
        || fs_metadata_len_arg(expr).is_some()
        || metadata_property_binding(expr, ctx).is_some_and(|(_, name)| name == "len")
        || json_path_key_field_get(expr, ctx).is_some()
        || json_value_path(expr, ctx).is_some_and(|(_, path)| !path.is_empty())
        || string_list_index_misparse_len(expr, ctx).is_some()
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
    if let Some(path) = path_buf_from_arg(expr) {
        emit_stringish(out, path, ctx)?;
        // `PathBuf::from(variable)` passes the string through — but a bare
        // variable here MOVES it into the new binding, and rh scripts have
        // value semantics (the source keeps using the original afterwards;
        // prune-target-incremental's `target_argument` was the live E0382).
        if matches!(path, Expr::Variable(..)) {
            out.push_str(".clone()");
        }
        return Ok(());
    }
    if emit_bytes_method(out, expr, ctx)? {
        return Ok(());
    }
    if let Some((binding, path)) = child_platform_facts_path(expr, ctx) {
        out.push_str("rh_json_string_path(&rh_child_platform_facts(&mut ");
        out.push_str(binding);
        out.push_str("), ");
        emit_json_path(out, &path);
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
    if let Some((binding, path, index)) = json_path_array_index(expr, ctx) {
        out.push_str("rh_json_string_path_index(&");
        out.push_str(binding);
        out.push_str(", ");
        emit_json_path(out, &path);
        out.push_str(", ");
        emit_intish(out, index, ctx)?;
        out.push(')');
        return Ok(());
    }
    if let Some((binding, path, key)) = json_path_key_get(expr, ctx) {
        out.push_str("rh_json_as_str(&");
        emit_json_path_key_get(out, binding, &path, key, ctx)?;
        out.push(')');
        return Ok(());
    }
    if let Some((binding, path, index, field)) = json_rhai_array_index_property(expr, ctx) {
        emit_json_rhai_array_index_property_string(out, binding, &path, index, field, ctx)?;
        return Ok(());
    }
    if let Some((binding, field)) = dir_entry_string_field(expr, ctx) {
        out.push_str(binding);
        out.push('.');
        out.push_str(field);
        out.push_str(".clone()");
        return Ok(());
    }
    if let Some(index) = args_index_expr(expr) {
        out.push_str("rh_arg(");
        emit_expr(out, index, ctx)?;
        out.push(')');
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
    if let Some(path) = path_parent_display_arg(expr)
        && emit_path_parent(out, path, ctx)?
    {
        return Ok(());
    }
    if let Some(path) = path_buf_from_display_arg(expr)
        && emit_path_buf_from(out, path, ctx)?
    {
        return Ok(());
    }
    if env_current_dir_display(expr) {
        out.push_str("rh_env_current_dir()");
        return Ok(());
    }
    if let Some(binding) = char_to_string_binding(expr, ctx) {
        out.push_str(binding);
        out.push_str(".to_string()");
        return Ok(());
    }
    if let Some((binding, path)) = json_value_path(expr, ctx)
        && !path.is_empty()
        && let Expr::Dot(boxed, ..) = expr
        && matches!(
            &boxed.rhs,
            Expr::MethodCall(call, ..) if call.name == "to_lower" && call.args.is_empty()
        )
    {
        out.push_str("rh_json_string_path(&");
        out.push_str(binding);
        out.push_str(", ");
        out.push_str("&[");
        for (index, segment) in path.iter().enumerate() {
            if index > 0 {
                out.push_str(", ");
            }
            out.push_str(&format!("{segment:?}"));
        }
        out.push_str("]).to_ascii_lowercase()");
        return Ok(());
    }
    if emit_string_method_expr(out, expr, ctx)? {
        return Ok(());
    }
    match expr {
        Expr::StringConstant(value, ..) => {
            out.push_str("String::from(");
            out.push_str(&format!("{value:?}"));
            out.push(')');
        }
        Expr::Variable(ident, ..)
            if matches!(
                ctx.scope.get(ident.1.as_str()).copied(),
                Some(ValueKind::String | ValueKind::Path)
            ) =>
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
        Expr::FloatConstant(value, ..) => {
            out.push_str(&format!("{value:?}"));
            out.push_str(".to_string()");
        }
        _ if let Some((binding, index)) = json_array_index(expr, ctx) => {
            out.push_str("rh_json_as_str(&rh_json_array_get(&");
            out.push_str(binding);
            out.push_str(", ");
            emit_expr(out, index, ctx)?;
            out.push_str("))");
        }
        _ if let Some((binding, index)) = string_list_index_misparse_len(expr, ctx) => {
            emit_string_list_index_misparse_len(out, binding, index)?;
        }
        _ if let Some((binding, index)) = string_list_index(expr, ctx) => {
            emit_string_list_index(out, binding, index, ctx)?;
        }
        _ if let Some(value) = json_stringify_pretty_arg(expr) => {
            if !emit_json_stringify_pretty(out, value, ctx)? {
                return Err(RhError::Transpile(
                    "stringify_pretty argument must be a JSON value".into(),
                ));
            }
        }
        _ if let Some(value) = json_stringify_arg(expr) => {
            if !emit_json_stringify(out, value, ctx)? {
                return Err(RhError::Transpile(
                    "stringify argument must be a JSON value".into(),
                ));
            }
        }
        _ if string_sub_string_arg(expr, ctx).is_some() => {
            if !emit_string_sub_string(out, expr, ctx)? {
                return Err(RhError::Transpile(
                    "sub_string receiver must be a string binding".into(),
                ));
            }
        }
        _ if let Some((binding, field)) = dir_entry_string_field(expr, ctx) => {
            out.push_str(binding);
            out.push('.');
            out.push_str(field);
            out.push_str(".clone()");
        }
        _ if output_property_binding(expr, ctx)
            .is_some_and(|(_, property)| property == "stdout" || property == "stderr") =>
        {
            emit_output_property(out, expr, ctx)?;
        }
        _ if let Some(binding) = child_state_binding(expr, ctx) => {
            out.push_str("rh_child_state(&mut ");
            out.push_str(binding);
            out.push(')');
        }
        _ if let Some(binding) = output_stdout_text_call(expr, ctx) => {
            out.push_str("rh_output_stdout_text(&");
            out.push_str(binding);
            out.push(')');
        }
        _ if let Some(binding) = output_stderr_text_call(expr, ctx) => {
            out.push_str("rh_output_stderr_text(&");
            out.push_str(binding);
            out.push(')');
        }
        _ if let Some(path) = std_fs_read_to_string_arg(expr) => {
            if !emit_std_fs_read_to_string(out, path, ctx)? {
                return Err(RhError::Transpile(
                    "read_to_string argument must be a string path".into(),
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
        _ if clipboard_get_text_arg(expr) => {
            out.push_str("rh_clipboard_get_text()");
        }
        _ if hash_fnv1a64_arg(expr).is_some() => {
            if !emit_hash_fnv1a64(out, expr, ctx)? {
                return Err(RhError::Transpile(
                    "fnv1a64 argument must be bytes::from_text of a stringish value".into(),
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
        _ if env_current_dir_display(expr) => {
            out.push_str("rh_env_current_dir()");
        }
        _ if let Some(path) = path_buf_from_display_arg(expr) => {
            if !emit_path_buf_from(out, path, ctx)? {
                return Err(RhError::Transpile(
                    "PathBuf::from.display argument must be a string path".into(),
                ));
            }
        }
        _ if let Some(path) = path_buf_from_file_name_arg(expr) => {
            if !emit_path_file_name(out, path, ctx)? {
                return Err(RhError::Transpile(
                    "PathBuf::from.file_name argument must be a string path".into(),
                ));
            }
        }
        _ if let Some(binding) = path_binding_display(expr, ctx) => {
            out.push_str(binding);
            out.push_str(".clone()");
        }
        _ if let Some(binding) = path_binding_file_name(expr, ctx) => {
            out.push_str("rh_path_file_name(&");
            out.push_str(binding);
            out.push(')');
        }
        _ if std_time_system_time_now_rfc3339(expr) => {
            out.push_str("rh_system_time_now_rfc3339()");
        }
        _ if let Some(binding) = system_time_rfc3339_binding(expr, ctx) => {
            out.push_str("rh_system_time_rfc3339(&");
            out.push_str(binding);
            out.push(')');
        }
        _ if let Some(binding) = dir_entry_metadata_modified_rfc3339(expr, ctx) => {
            out.push_str("rh_system_time_rfc3339(&rh_metadata(&");
            out.push_str(binding);
            out.push_str(".path).modified)");
        }
        _ if let Some((path, _)) = fs_metadata_modified_rfc3339(expr) => {
            out.push_str("rh_system_time_rfc3339(&");
            if !emit_std_fs_metadata(out, path, ctx)? {
                return Err(RhError::Transpile(
                    "metadata.modified.rfc3339 path must be a string".into(),
                ));
            }
            out.push_str(".modified)");
        }
        _ if is_pure_int_expr(expr) || is_native_json_int_expr(expr, ctx) => {
            out.push('(');
            emit_expr(out, expr, ctx)?;
            out.push_str(").to_string()");
        }
        Expr::FnCall(call, ..)
            if call.op_token.is_none()
                && call.namespace.is_empty()
                && ctx.local_fns.contains(call.name.as_str()) =>
        {
            emit_call(out, call, ctx)?;
        }
        _ => {
            let rendered = expr_to_rhai(expr).unwrap_or_else(|_| "<unprintable>".into());
            let scope_kind = match expr {
                Expr::Dot(boxed, ..) => match &boxed.lhs {
                    Expr::Variable(ident, ..) => ctx.scope.get(ident.1.as_str()).copied(),
                    _ => None,
                },
                Expr::Variable(ident, ..) => ctx.scope.get(ident.1.as_str()).copied(),
                _ => None,
            };
            return Err(RhError::Transpile(format!(
                "unsupported string expression in native rh: {rendered} \
                 (expr={expr:?}, scope_kind={scope_kind:?})"
            )));
        }
    }
    Ok(())
}

fn emit_intish(out: &mut String, expr: &Expr, ctx: &mut EmitCtx) -> Result<(), RhError> {
    if let Some((receiver, json, separator)) = string_split_len_parts(expr, ctx) {
        out.push('(');
        emit_string_split_call(out, receiver, json, separator, ctx)?;
        out.push_str(".len() as INT)");
        return Ok(());
    }
    if let Some((binding, path)) = json_object_keys_len_path(expr, ctx) {
        out.push_str("rh_json_object_keys_len(&");
        out.push_str(binding);
        out.push_str(", ");
        emit_json_path(out, &path);
        out.push(')');
        return Ok(());
    }
    if let Some(binding) = set_keys_len_path(expr, ctx) {
        out.push('(');
        out.push_str(binding);
        out.push_str(".len() as INT)");
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
    if let Some((binding, path, key, field)) = json_path_key_field_get(expr, ctx) {
        emit_json_path_key_field_int(out, binding, &path, key, field, ctx)?;
        return Ok(());
    }
    if let Some((binding, path, index, field)) = json_rhai_array_index_property(expr, ctx) {
        emit_json_rhai_array_index_property_int(out, binding, &path, index, field, ctx)?;
        return Ok(());
    }
    if let Some(name) = std_env_get_parse_int_arg(expr)
        && emit_std_env_get_parse_int(out, name, ctx)?
    {
        return Ok(());
    }
    if emit_string_parse_int(out, expr, ctx)? {
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
        if task_sleep_arg(expr).is_some() {
            let mut stmt = String::new();
            if emit_task_sleep_stmt(&mut stmt, expr, ctx)? {
                out.push_str("{\n");
                out.push_str(&stmt);
                out.push_str("    0\n}");
                return Ok(());
            }
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
        if let Some((program, arguments, timeout, options)) = process_status_args(expr, ctx)
            && emit_process_status(out, program, arguments, timeout, options, ctx)?
        {
            return Ok(());
        }
        if let Some((program, arguments, timeout, stdout_path, options)) =
            process_stdout_file_args(expr, ctx)
            && emit_process_stdout_file(
                out,
                program,
                arguments,
                timeout,
                stdout_path,
                options,
                ctx,
            )?
        {
            return Ok(());
        }
        if let Some(source) = json_parse_arg(expr)
            && emit_json_parse(out, source, ctx)?
        {
            return Ok(());
        }
        if let Some(path) = json_parse_file_arg(expr)
            && emit_json_parse_file(out, path, ctx)?
        {
            return Ok(());
        }
        if let Some(path) = image_inspect_png_arg(expr) {
            out.push_str("rh_host_json_call(\"image.inspect_png\", &serde_json::json!({\"path\": ");
            emit_stringish(out, path, ctx)?;
            out.push_str("}))");
            return Ok(());
        }
        if clipboard_get_text_arg(expr) {
            out.push_str("rh_clipboard_get_text()");
            return Ok(());
        }
        if let Some(text) = clipboard_set_text_arg(expr) {
            out.push_str("rh_clipboard_set_text(&");
            emit_stringish(out, text, ctx)?;
            out.push(')');
            return Ok(());
        }
        if let Some(argument) = type_of_arg(expr)
            && emit_type_of(out, argument, ctx)?
        {
            return Ok(());
        }
        if let Some((binding, path)) = json_object_keys_len_path(expr, ctx) {
            out.push_str("rh_json_object_keys_len(&");
            out.push_str(binding);
            out.push_str(", ");
            emit_json_path(out, &path);
            out.push(')');
            return Ok(());
        }
        if let Some(binding) = set_keys_len_path(expr, ctx) {
            out.push('(');
            out.push_str(binding);
            out.push_str(".len() as INT)");
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
        if let Some((path, contents)) = std_fs_write_args(expr)
            && emit_std_fs_write(out, path, contents, ctx)?
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
        if let Some(path) = std_fs_remove_dir_all_arg(expr)
            && emit_std_fs_remove_dir_all(out, path, ctx)?
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
        if emit_json_path_contains(out, expr, ctx)? {
            return Ok(());
        }
        if emit_string_predicate(out, expr, ctx)? {
            return Ok(());
        }
        if emit_string_sub_string(out, expr, ctx)? {
            return Ok(());
        }
        if emit_string_method_expr(out, expr, ctx)? {
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
        if let Some(path) = path_parent_display_arg(expr)
            && emit_path_parent(out, path, ctx)?
        {
            return Ok(());
        }
        if let Some(path) = path_buf_from_display_arg(expr)
            && emit_path_buf_from(out, path, ctx)?
        {
            return Ok(());
        }
        if let Some(binding) = path_binding_display(expr, ctx) {
            out.push_str(binding);
            out.push_str(".clone()");
            return Ok(());
        }
        if env_current_dir_display(expr) {
            out.push_str("rh_env_current_dir()");
            return Ok(());
        }
        if let Some(path) = path_buf_from_arg(expr)
            && emit_path_buf_from(out, path, ctx)?
        {
            return Ok(());
        }
        if let Some(path) = path_buf_from_is_absolute_arg(expr)
            && emit_path_is_absolute(out, path, ctx)?
        {
            return Ok(());
        }
        if let Some(binding) = path_binding_is_absolute(expr, ctx) {
            out.push_str("rh_path_is_absolute(&");
            out.push_str(binding);
            out.push(')');
            return Ok(());
        }
        if let Some(path) = std_fs_symlink_metadata_arg(expr)
            && emit_std_fs_symlink_metadata(out, path, ctx)?
        {
            return Ok(());
        }
        if let Some(path) = std_fs_metadata_arg(expr)
            && emit_std_fs_metadata(out, path, ctx)?
        {
            return Ok(());
        }
        if let Some(binding) = dir_entry_metadata_binding(expr, ctx) {
            out.push_str("rh_metadata(&");
            out.push_str(binding);
            out.push_str(".path)");
            return Ok(());
        }
        if let Some(binding) = metadata_modified_binding(expr, ctx) {
            out.push_str(binding);
            out.push_str(".modified");
            return Ok(());
        }
        if let Some((binding, property)) = metadata_modified_property_binding(expr, ctx) {
            if property == "unix_millis" {
                out.push_str(binding);
                out.push_str(".modified.unix_millis");
            } else {
                out.push_str("rh_system_time_rfc3339(&");
                out.push_str(binding);
                out.push_str(".modified)");
            }
            return Ok(());
        }
        if let Some((binding, property)) = system_time_property_binding(expr, ctx) {
            if property == "unix_millis" {
                out.push_str(binding);
                out.push_str(".unix_millis");
            } else {
                out.push_str("rh_system_time_rfc3339(&");
                out.push_str(binding);
                out.push(')');
            }
            return Ok(());
        }
        if let Some((path, call_name)) = fs_metadata_modified_arg(expr) {
            emit_fs_metadata_call(out, path, call_name, ctx)?;
            out.push_str(".modified");
            return Ok(());
        }
        if let Some((path, call_name)) = fs_metadata_len_arg(expr) {
            emit_fs_metadata_call(out, path, call_name, ctx)?;
            out.push_str(".len");
            return Ok(());
        }
        if let Some((path, call_name)) = fs_metadata_modified_unix_millis(expr) {
            emit_fs_metadata_call(out, path, call_name, ctx)?;
            out.push_str(".modified.unix_millis");
            return Ok(());
        }
        if let Some((path, call_name)) = fs_metadata_modified_rfc3339(expr) {
            out.push_str("rh_system_time_rfc3339(&");
            emit_fs_metadata_call(out, path, call_name, ctx)?;
            out.push_str(".modified)");
            return Ok(());
        }
        if let Some(binding) = dir_entry_metadata_modified_unix_millis(expr, ctx) {
            out.push_str("rh_metadata(&");
            out.push_str(binding);
            out.push_str(".path).modified.unix_millis");
            return Ok(());
        }
        if let Some(binding) = dir_entry_metadata_modified_rfc3339(expr, ctx) {
            out.push_str("rh_system_time_rfc3339(&rh_metadata(&");
            out.push_str(binding);
            out.push_str(".path).modified)");
            return Ok(());
        }
        if emit_metadata_property(out, expr, ctx)? {
            return Ok(());
        }
        if std_time_system_time_now_unix_millis(expr) {
            out.push_str("rh_system_time_now_unix_millis()");
            return Ok(());
        }
        if std_process_id(expr) {
            out.push_str("rh_process_id()");
            return Ok(());
        }
        if let Some(pid) = std_process_kill_arg(expr) {
            out.push_str("{\n        rh_process_kill(");
            emit_intish(out, pid, ctx)?;
            out.push_str(");\n        0\n    }");
            return Ok(());
        }
        if std_process_list(expr) {
            out.push_str("rh_host_json_call(\"process.list\", &serde_json::json!({}))");
            return Ok(());
        }
        if let Some(program) = std_process_command_arg(expr)
            && emit_std_process_command(out, program, ctx)?
        {
            return Ok(());
        }
        if emit_local_command_receiver_method(out, expr, ctx)? {
            return Ok(());
        }
        if emit_command_method(out, expr, ctx)? {
            return Ok(());
        }
        if emit_output_method(out, expr, ctx)? {
            return Ok(());
        }
        if emit_child_method(out, expr, ctx)? {
            return Ok(());
        }
        if emit_window_control_method(out, expr, ctx)? {
            return Ok(());
        }
        if emit_stream_method(out, expr, ctx)? {
            return Ok(());
        }
        if emit_bytes_method(out, expr, ctx)? {
            return Ok(());
        }
        if emit_output_property(out, expr, ctx)? {
            return Ok(());
        }
        if emit_child_property(out, expr, ctx)? {
            return Ok(());
        }
        if emit_window_control_property(out, expr, ctx)? {
            return Ok(());
        }
        if emit_window_rect_property(out, expr, ctx)? {
            return Ok(());
        }
        if emit_bytes_property(out, expr, ctx)? {
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
        if hash_fnv1a64_arg(expr).is_some() && emit_hash_fnv1a64(out, expr, ctx)? {
            return Ok(());
        }
        if let Some((path, value)) = runtime_atomic_write_args(expr)
            && emit_runtime_atomic_write(out, path, value, ctx)?
        {
            return Ok(());
        }
        if let Some((path, text)) = runtime_append_sync_args(expr)
            && emit_runtime_append_sync(out, path, text, ctx)?
        {
            return Ok(());
        }
        if let Some(items) = bytes_from_array_items(expr) {
            emit_bytes_from_array(out, items, ctx)?;
            return Ok(());
        }
        if let Some(text) = bytes_from_text_arg(expr)
            && emit_bytes_from_text(out, text, ctx)?
        {
            return Ok(());
        }
        if let Some(value) = json_stringify_pretty_arg(expr)
            && emit_json_stringify_pretty(out, value, ctx)?
        {
            return Ok(());
        }
        if let Some(value) = json_stringify_arg(expr)
            && emit_json_stringify(out, value, ctx)?
        {
            return Ok(());
        }
        if let Some((binding, index)) = string_list_index_misparse_len(expr, ctx) {
            emit_string_list_index_misparse_len(out, binding, index)?;
            return Ok(());
        }
        if let Some((binding, index)) = string_list_index(expr, ctx) {
            emit_string_list_index(out, binding, index, ctx)?;
            return Ok(());
        }
        if let Some((binding, index)) = child_list_index(expr, ctx) {
            out.push_str("rh_child_share(&mut ");
            out.push_str(binding);
            out.push('[');
            emit_intish(out, index, ctx)?;
            out.push_str(" as usize])");
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
        if let Some((binding, path, index)) = json_path_array_index(expr, ctx) {
            out.push_str("rh_json_get_path_index(&");
            out.push_str(binding);
            out.push_str(", ");
            emit_json_path(out, &path);
            out.push_str(", ");
            emit_intish(out, index, ctx)?;
            out.push(')');
            return Ok(());
        }
        if let Some((binding, path, index, field)) = json_rhai_array_index_property(expr, ctx) {
            out.push_str("rh_json_get_path(&rh_json_get_path_index(&");
            out.push_str(binding);
            out.push_str(", ");
            emit_json_path(out, &path);
            out.push_str(", ");
            emit_intish(out, index, ctx)?;
            out.push_str("), &[");
            out.push_str(&format!("{field:?}"));
            out.push_str("])");
            return Ok(());
        }
        if let Some((binding, path, key)) = json_path_key_get(expr, ctx) {
            emit_json_path_key_get(out, binding, &path, key, ctx)?;
            return Ok(());
        }
        if let Some((receiver, json, separator)) = string_split_len_parts(expr, ctx) {
            out.push('(');
            emit_string_split_call(out, receiver, json, separator, ctx)?;
            out.push_str(".len() as INT)");
            return Ok(());
        }
        if let Some((receiver, json, separator)) = string_split_parts(expr, ctx) {
            emit_string_split_call(out, receiver, json, separator, ctx)?;
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
            && is_local_fn_call(call.name.as_str(), ctx)
        {
            return emit_call(out, call, ctx);
        }
        if matches!(expr, Expr::Map(..)) {
            emit_json_map_literal(out, expr, ctx)?;
            return Ok(());
        }
        if matches!(expr, Expr::Array(items, ..) if items.is_empty()) {
            out.push_str("serde_json::Value::Array(Vec::new())");
            return Ok(());
        }
        if let Expr::Array(items, ..) = expr
            && !items.is_empty()
            && items
                .iter()
                .all(|item| is_native_json_value_item(item, ctx))
        {
            emit_json_array_value_literal(out, items, ctx)?;
            return Ok(());
        }
        if let Expr::And(args, ..) = expr {
            return logical_nary(out, "&&", args, ctx);
        }
        if let Expr::Or(args, ..) = expr {
            return logical_nary(out, "||", args, ctx);
        }
        // String-producing path/display forms must win over uses_host_surface
        // (PathBuf::from / std::path::* are host-looking but natively emitted).
        // Probe emit_stringish first; unsupported stringish edges are transpile errors.
        if is_explicit_string_expr(expr, ctx) {
            let mut stringish = String::new();
            if emit_stringish(&mut stringish, expr, ctx).is_ok() {
                out.push_str(&stringish);
                return Ok(());
            }
        }
        if let Some(name) = std_env_get_parse_int_arg(expr)
            && emit_std_env_get_parse_int(out, name, ctx)?
        {
            return Ok(());
        }
        if emit_string_parse_int(out, expr, ctx)? {
            return Ok(());
        }
        if !is_pure_int_expr(expr)
            && !is_native_json_int_expr(expr, ctx)
            && (uses_host_surface(expr)
                || !matches!(
                    expr,
                    Expr::IntegerConstant(..) | Expr::BoolConstant(..) | Expr::Variable(..)
                ))
        {
            return Err(RhError::Transpile(format!(
                "unsupported expression in native pack: {expr:?}"
            )));
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
        Expr::Variable(ident, ..) => out.push_str(ctx.resolve_binding(ident.1.as_str())),
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
                .is_some_and(|name| ctx.scope.get(name).copied() == Some(ValueKind::ChildList)) =>
        {
            out.push('(');
            out.push_str(var_len_name(expr).expect("checked child list binding"));
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
        Expr::Dot(..) if is_var_len_expr(expr) => {
            return Err(RhError::Transpile(format!(
                "unsupported var-len expression in native pack: {expr:?}"
            )));
        }
        Expr::FnCall(call, ..) => emit_call(out, call, ctx)?,
        Expr::Stmt(block) => {
            out.push_str("{ ");
            emit_block(out, block, ctx, true)?;
            out.push_str(" }");
        }
        other if ctx.cdylib && uses_host_surface(other) => {
            return Err(RhError::Transpile(format!(
                "unsupported host-surface expression in native pack: {other:?}"
            )));
        }
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
    if let Some((binding, path)) = json_value_path(path, ctx)
        && !path.is_empty()
    {
        out.push_str("rh_std_fs_read_to_string(&rh_json_string_path(&");
        out.push_str(binding);
        out.push_str(", ");
        emit_json_path(out, &path);
        out.push_str("))");
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
        Expr::Array(items, ..)
            if !items.is_empty()
                && items
                    .iter()
                    .all(|item| is_native_json_value_item(item, ctx)) =>
        {
            out.push_str("rh_json_stringify_pretty(&");
            emit_json_array_value_literal(out, items, ctx)?;
            out.push(')');
            Ok(true)
        }
        _ => Ok(false),
    }
}

fn emit_json_stringify(out: &mut String, value: &Expr, ctx: &mut EmitCtx) -> Result<bool, RhError> {
    match value {
        Expr::Variable(ident, ..)
            if ctx.scope.get(ident.1.as_str()).copied() == Some(ValueKind::Json) =>
        {
            out.push_str("rh_json_stringify(&");
            out.push_str(ident.1.as_str());
            out.push(')');
            Ok(true)
        }
        Expr::Map(..) => {
            out.push_str("rh_json_stringify(&");
            emit_json_map_literal(out, value, ctx)?;
            out.push(')');
            Ok(true)
        }
        Expr::Array(items, ..)
            if !items.is_empty()
                && items
                    .iter()
                    .all(|item| is_native_json_value_item(item, ctx)) =>
        {
            out.push_str("rh_json_stringify(&");
            emit_json_array_value_literal(out, items, ctx)?;
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
    let Some((binding, path, item)) = json_path_array_push_call(expr, ctx) else {
        return Ok(false);
    };
    let mut item_rust = String::new();
    if emit_json_value_expr(&mut item_rust, item, ctx).is_err() {
        return Ok(false);
    }
    out.push_str("    let _ = ");
    if path.is_empty() {
        out.push_str("rh_json_array_push(&mut ");
        out.push_str(binding);
        out.push_str(", ");
        out.push_str(&item_rust);
        out.push_str(");\n");
    } else {
        out.push_str("rh_json_array_push_path(&mut ");
        out.push_str(binding);
        out.push_str(", ");
        emit_json_path(out, &path);
        out.push_str(", ");
        out.push_str(&item_rust);
        out.push_str(");\n");
    }
    Ok(true)
}

fn emit_json_root_mutation_stmt(
    out: &mut String,
    expr: &Expr,
    ctx: &mut EmitCtx,
) -> Result<bool, RhError> {
    let Expr::Dot(boxed, ..) = expr else {
        return Ok(false);
    };
    let Expr::Variable(ident, ..) = &boxed.lhs else {
        return Ok(false);
    };
    if ctx.scope.get(ident.1.as_str()).copied() != Some(ValueKind::Json) {
        return Ok(false);
    }
    let Expr::MethodCall(call, ..) = &boxed.rhs else {
        return Ok(false);
    };
    match (call.name.as_str(), call.args.as_slice()) {
        ("remove", [key]) => {
            let mut key_rust = String::new();
            if emit_json_map_key(&mut key_rust, key, ctx).is_err() {
                return Ok(false);
            }
            out.push_str("    let _ = rh_json_remove(&mut ");
            out.push_str(ident.1.as_str());
            out.push_str(", &");
            out.push_str(&key_rust);
            out.push_str(");\n");
            Ok(true)
        }
        ("insert", [index, item]) if is_pure_int_expr(index) => {
            let mut item_rust = String::new();
            if emit_json_value_expr(&mut item_rust, item, ctx).is_err() {
                return Ok(false);
            }
            out.push_str("    let _ = rh_json_array_insert(&mut ");
            out.push_str(ident.1.as_str());
            out.push_str(", ");
            emit_expr(out, index, ctx)?;
            out.push_str(", ");
            out.push_str(&item_rust);
            out.push_str(");\n");
            Ok(true)
        }
        _ => Ok(false),
    }
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
        Expr::Unit(..) => {
            out.push_str("serde_json::Value::Null");
        }
        Expr::Map(..) => emit_json_map_literal(out, expr, ctx)?,
        Expr::Array(items, ..) if items.is_empty() => {
            out.push_str("serde_json::Value::Array(Vec::new())");
        }
        Expr::Array(items, ..)
            if !items.is_empty()
                && items
                    .iter()
                    .all(|item| is_native_json_value_item(item, ctx)) =>
        {
            emit_json_array_value_literal(out, items, ctx)?;
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
            if ctx.scope.get(ident.1.as_str()).copied() == Some(ValueKind::Path) =>
        {
            out.push_str("serde_json::Value::String(");
            out.push_str(ident.1.as_str());
            out.push_str(".clone())");
        }
        Expr::Variable(ident, ..)
            if ctx.scope.get(ident.1.as_str()).copied() == Some(ValueKind::StringList) =>
        {
            // Keep StringList params typed at JSON map literals (e.g. check.rh
            // `spec(..., arguments, ...)`), matching local-fn JSON arg coerce.
            out.push_str("serde_json::Value::Array(");
            out.push_str(ident.1.as_str());
            out.push_str(".iter().cloned().map(serde_json::Value::String).collect())");
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
        _ if output_property_binding(expr, ctx).is_some() => {
            let (binding, property) = output_property_binding(expr, ctx).expect("checked output");
            match property {
                "stdout" | "stderr" => {
                    out.push_str("serde_json::Value::Number(serde_json::Number::from(");
                    out.push_str(binding);
                    out.push('.');
                    out.push_str(property);
                    out.push_str(".len() as i64))");
                }
                "success" => {
                    out.push_str("serde_json::Value::Bool(");
                    out.push_str(binding);
                    out.push_str(".success != 0)");
                }
                _ => {
                    out.push_str("serde_json::json!(");
                    out.push_str(binding);
                    out.push('.');
                    out.push_str(property);
                    out.push(')');
                }
            }
        }
        _ if output_stdout_text_call(expr, ctx).is_some() => {
            let binding = output_stdout_text_call(expr, ctx).expect("checked stdout_text");
            out.push_str("serde_json::Value::String(rh_output_stdout_text(&");
            out.push_str(binding);
            out.push_str("))");
        }
        _ if output_stderr_text_call(expr, ctx).is_some() => {
            let binding = output_stderr_text_call(expr, ctx).expect("checked stderr_text");
            out.push_str("serde_json::Value::String(rh_output_stderr_text(&");
            out.push_str(binding);
            out.push_str("))");
        }
        _ if is_json_bool_comparison(expr, ctx) => emit_json_bool_comparison(out, expr, ctx)?,
        _ if string_concat_args(expr, ctx).is_some()
            || args_index_expr(expr).is_some()
            || std_env_get_arg(expr).is_some()
            || crypto_sha256_file_arg(expr).is_some()
            || hash_fnv1a64_arg(expr).is_some()
            || json_stringify_pretty_arg(expr).is_some()
            || json_stringify_arg(expr).is_some()
            || string_list_index(expr, ctx).is_some()
            || std_time_system_time_now_rfc3339(expr)
            || path_buf_from_file_name_arg(expr).is_some()
            || path_binding_file_name(expr, ctx).is_some()
            || path_binding_display(expr, ctx).is_some()
            || path_join_display_args(expr).is_some()
            || path_absolute_display_arg(expr).is_some()
            || path_parent_display_arg(expr).is_some() =>
        {
            out.push_str("serde_json::Value::String(");
            emit_stringish(out, expr, ctx)?;
            out.push(')');
        }
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
        _ if let Some((binding, path, index)) = json_path_array_index(expr, ctx) => {
            out.push_str("rh_json_get_path_index(&");
            out.push_str(binding);
            out.push_str(", ");
            emit_json_path(out, &path);
            out.push_str(", ");
            emit_intish(out, index, ctx)?;
            out.push(')');
        }
        _ if let Some((binding, path, index, field)) =
            json_rhai_array_index_property(expr, ctx) =>
        {
            out.push_str("rh_json_get_path(&rh_json_get_path_index(&");
            out.push_str(binding);
            out.push_str(", ");
            emit_json_path(out, &path);
            out.push_str(", ");
            emit_intish(out, index, ctx)?;
            out.push_str("), &[");
            out.push_str(&format!("{field:?}"));
            out.push_str("])");
        }
        _ if let Some((binding, path, key)) = json_path_key_get(expr, ctx) => {
            emit_json_path_key_get(out, binding, &path, key, ctx)?;
        }
        _ if var_len_name(expr)
            .is_some_and(|name| ctx.scope.get(name).copied() == Some(ValueKind::ChildList)) =>
        {
            out.push_str("serde_json::json!(");
            out.push('(');
            out.push_str(var_len_name(expr).expect("checked child list binding"));
            out.push_str(".len() as INT)");
            out.push(')');
        }
        _ if metadata_property_binding(expr, ctx).is_some_and(|(_, name)| name == "len")
            || is_pure_int_expr(expr)
            || is_native_json_int_expr(expr, ctx) =>
        {
            out.push_str("serde_json::json!(");
            emit_expr(out, expr, ctx)?;
            out.push(')');
        }
        Expr::FnCall(call, ..) if is_host_api_call(call, "json", "parse", 1) => {
            out.push_str("rh_json_parse(&");
            emit_stringish(out, &call.args[0], ctx)?;
            out.push(')');
        }
        Expr::FnCall(call, ..) if json_parse_file_arg(expr).is_some() => {
            emit_json_parse_file(out, &call.args[0], ctx)?;
        }
        Expr::FnCall(call, ..)
            if call.op_token.is_none()
                && call.namespace.is_empty()
                && is_local_fn_call(call.name.as_str(), ctx) =>
        {
            let resolved =
                resolve_local_fn_name(call.name.as_str(), ctx).expect("checked local fn call");
            let return_kind = ctx
                .local_fn_return_kinds
                .get(resolved.as_str())
                .copied()
                .unwrap_or(ValueKind::Int);
            let sig = ctx
                .local_fn_sigs
                .get(resolved.as_str())
                .cloned()
                .unwrap_or_default();
            let mut emit_typed_args = |out: &mut String| -> Result<(), RhError> {
                for (index, arg) in call.args.iter().enumerate() {
                    if index > 0 {
                        out.push_str(", ");
                    }
                    match sig.get(index).copied() {
                        Some(ValueKind::String) => emit_string_arg(out, arg, ctx)?,
                        Some(ValueKind::Json) => emit_json_arg(out, arg, ctx)?,
                        Some(ValueKind::ChildList) => emit_child_list_arg(out, arg, ctx)?,
                        Some(ValueKind::StringList) => emit_string_list_arg(out, arg, ctx)?,
                        Some(ValueKind::Output) => {
                            if let Expr::Variable(ident, ..) = arg
                                && ctx.scope.get(ident.1.as_str()).copied()
                                    == Some(ValueKind::Output)
                            {
                                out.push_str(ident.1.as_str());
                                out.push_str(".clone()");
                            } else {
                                emit_expr(out, arg, ctx)?;
                            }
                        }
                        Some(ValueKind::Child) => {
                            if let Expr::Variable(ident, ..) = arg
                                && ctx.scope.get(ident.1.as_str()).copied()
                                    == Some(ValueKind::Child)
                            {
                                out.push_str(ident.1.as_str());
                                out.push_str(".clone()");
                            } else {
                                emit_expr(out, arg, ctx)?;
                            }
                        }
                        _ => emit_expr(out, arg, ctx)?,
                    }
                }
                Ok(())
            };
            match return_kind {
                ValueKind::String | ValueKind::Path => {
                    out.push_str("serde_json::Value::String(");
                    out.push_str(resolved.as_str());
                    out.push('(');
                    emit_typed_args(out)?;
                    out.push_str("))");
                }
                ValueKind::Json => {
                    out.push_str(resolved.as_str());
                    out.push('(');
                    emit_typed_args(out)?;
                    out.push(')');
                }
                ValueKind::Bool => {
                    out.push_str("serde_json::Value::Bool(");
                    out.push_str(resolved.as_str());
                    out.push('(');
                    emit_typed_args(out)?;
                    out.push_str(") != 0)");
                }
                _ => {
                    out.push_str("serde_json::json!(");
                    out.push_str(resolved.as_str());
                    out.push('(');
                    emit_typed_args(out)?;
                    out.push_str("))");
                }
            }
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

fn emit_std_env_get_parse_int(
    out: &mut String,
    name: &Expr,
    ctx: &mut EmitCtx,
) -> Result<bool, RhError> {
    let mut name_expr = String::new();
    if !emit_native_string(&mut name_expr, name, ctx)? {
        return Ok(false);
    }
    out.push_str("rh_env_parse_int(");
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

fn emit_hash_fnv1a64_bytes_input(
    out: &mut String,
    expr: &Expr,
    ctx: &mut EmitCtx,
) -> Result<bool, RhError> {
    let text_arg = bytes_from_text_arg(expr).unwrap_or(expr);
    if let Some(value) = json_stringify_arg(text_arg) {
        out.push('&');
        if !emit_json_stringify(out, value, ctx)? {
            return Ok(false);
        }
        return Ok(true);
    }
    if let Some(value) = json_stringify_pretty_arg(text_arg) {
        out.push('&');
        if !emit_json_stringify_pretty(out, value, ctx)? {
            return Ok(false);
        }
        return Ok(true);
    }
    if emit_native_string(out, text_arg, ctx)? {
        return Ok(true);
    }
    out.push('&');
    emit_stringish(out, text_arg, ctx)?;
    Ok(true)
}

fn emit_hash_fnv1a64(out: &mut String, expr: &Expr, ctx: &mut EmitCtx) -> Result<bool, RhError> {
    let Some(bytes_arg) = hash_fnv1a64_arg(expr) else {
        return Ok(false);
    };
    let mut input = String::new();
    if !emit_hash_fnv1a64_bytes_input(&mut input, bytes_arg, ctx)? {
        return Ok(false);
    }
    out.push_str("rh_hash_fnv1a64(");
    out.push_str(&input);
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
    if !emit_native_path_ref(&mut path_expr, path, ctx)? {
        return Ok(false);
    }
    out.push_str("rh_atomic_write(");
    out.push_str(&path_expr);
    out.push_str(", &");
    emit_stringish(out, value, ctx)?;
    out.push(')');
    Ok(true)
}

fn emit_runtime_append_sync(
    out: &mut String,
    path: &Expr,
    text: &Expr,
    ctx: &mut EmitCtx,
) -> Result<bool, RhError> {
    let mut path_expr = String::new();
    if !emit_native_string(&mut path_expr, path, ctx)? {
        return Ok(false);
    }
    out.push_str("rh_append_sync(");
    out.push_str(&path_expr);
    out.push_str(", &");
    emit_stringish(out, text, ctx)?;
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
    arguments: ProcessArguments<'_>,
    timeout: &Expr,
    options: Option<&Expr>,
    ctx: &mut EmitCtx,
) -> Result<bool, RhError> {
    let mut program_expr = String::new();
    if !emit_native_string(&mut program_expr, program, ctx)? || !is_pure_int_expr(timeout) {
        return Ok(false);
    }
    let mut options_expr = String::new();
    if let Some(options) = options
        && !emit_process_options(&mut options_expr, options, ctx)?
    {
        return Ok(false);
    }
    out.push_str("rh_process_status(");
    out.push_str(&program_expr);
    out.push_str(", ");
    if !emit_process_arguments(out, arguments, ctx)? {
        return Ok(false);
    }
    out.push_str(", ");
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
    arguments: ProcessArguments<'_>,
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
    let mut options_expr = String::new();
    if let Some(options) = options
        && !emit_process_options(&mut options_expr, options, ctx)?
    {
        return Ok(false);
    }
    out.push_str("rh_process_stdout_file(");
    out.push_str(&program_expr);
    out.push_str(", ");
    if !emit_process_arguments(out, arguments, ctx)? {
        return Ok(false);
    }
    out.push_str(", ");
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

fn emit_owned_string_element(
    out: &mut String,
    expr: &Expr,
    ctx: &mut EmitCtx,
) -> Result<bool, RhError> {
    match expr {
        Expr::Variable(ident, ..)
            if matches!(
                ctx.scope.get(ident.1.as_str()).copied(),
                Some(ValueKind::String | ValueKind::Path)
            ) =>
        {
            // Clone so later uses of the binding remain valid (argv + options).
            out.push_str(ident.1.as_str());
            out.push_str(".clone()");
            Ok(true)
        }
        _ => {
            // format!/String::from/path joins and `"" + json.field` already produce owned String.
            match emit_stringish(out, expr, ctx) {
                Ok(()) => Ok(true),
                Err(_) => {
                    let mut borrowed = String::new();
                    if !emit_native_string(&mut borrowed, expr, ctx)? {
                        return Ok(false);
                    }
                    out.push_str("String::from(");
                    out.push_str(&borrowed);
                    out.push(')');
                    Ok(true)
                }
            }
        }
    }
}

fn emit_process_arguments(
    out: &mut String,
    arguments: ProcessArguments<'_>,
    ctx: &mut EmitCtx,
) -> Result<bool, RhError> {
    match arguments {
        ProcessArguments::Literal(items) => {
            out.push_str("&vec![");
            for (index, argument) in items.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                if !emit_owned_string_element(out, argument, ctx)? {
                    return Ok(false);
                }
            }
            out.push(']');
            Ok(true)
        }
        ProcessArguments::StringList(binding) => {
            out.push('&');
            out.push_str(binding);
            Ok(true)
        }
        ProcessArguments::JsonArray(binding) => {
            out.push_str("&rh_json_string_argv(&");
            out.push_str(binding);
            out.push(')');
            Ok(true)
        }
    }
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

fn emit_json_parse_file(out: &mut String, path: &Expr, ctx: &mut EmitCtx) -> Result<bool, RhError> {
    if let Some((base, child)) = path_join_display_args(path) {
        let mut join_expr = String::new();
        if !emit_path_join(&mut join_expr, base, child, ctx)? {
            return Ok(false);
        }
        out.push_str("rh_json_parse(&rh_std_fs_read_to_string(&");
        out.push_str(&join_expr);
        out.push_str("))");
        return Ok(true);
    }
    if let Some(source) = path_absolute_display_arg(path) {
        let mut source_expr = String::new();
        if !emit_path_absolute(&mut source_expr, source, ctx)? {
            return Ok(false);
        }
        out.push_str("rh_json_parse(&rh_std_fs_read_to_string(&");
        out.push_str(&source_expr);
        out.push_str("))");
        return Ok(true);
    }
    if let Some(binding) = path_binding_display(path, ctx) {
        out.push_str("rh_json_parse(&rh_std_fs_read_to_string(&");
        out.push_str(binding);
        out.push_str("))");
        return Ok(true);
    }
    let mut path_expr = String::new();
    if !emit_native_string(&mut path_expr, path, ctx)? {
        return Ok(false);
    }
    out.push_str("rh_json_parse(&rh_std_fs_read_to_string(");
    out.push_str(&path_expr);
    out.push_str("))");
    Ok(true)
}

fn emit_native_string(out: &mut String, expr: &Expr, ctx: &mut EmitCtx) -> Result<bool, RhError> {
    match expr {
        Expr::StringConstant(value, ..) => out.push_str(&format!("{value:?}")),
        Expr::Variable(ident, ..)
            if matches!(
                ctx.scope.get(ident.1.as_str()).copied(),
                Some(ValueKind::String | ValueKind::Path)
            ) =>
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
        _ if let Some(binding) = path_binding_display(expr, ctx) => {
            out.push('&');
            out.push_str(binding);
        }
        _ if let Some(binding) = path_binding_file_name(expr, ctx) => {
            out.push_str("&rh_path_file_name(&");
            out.push_str(binding);
            out.push(')');
        }
        _ if env_current_dir_display(expr) => {
            out.push_str("&rh_env_current_dir()");
        }
        _ if let Some(binding) = child_state_binding(expr, ctx) => {
            out.push_str("&rh_child_state(&mut ");
            out.push_str(binding);
            out.push(')');
        }
        _ if let Some(binding) = output_stdout_text_call(expr, ctx) => {
            out.push_str("&rh_output_stdout_text(&");
            out.push_str(binding);
            out.push(')');
        }
        _ if let Some(binding) = output_stderr_text_call(expr, ctx) => {
            out.push_str("&rh_output_stderr_text(&");
            out.push_str(binding);
            out.push(')');
        }
        _ if bytes_to_text_call(expr, ctx).is_some() => {
            out.push('&');
            emit_bytes_method(out, expr, ctx)?;
        }
        _ => return Ok(false),
    }
    Ok(true)
}

/// Path refs for FS/runtime calls: string bindings plus `parent/join/absolute/.display`
/// and JSON-scalar path locals (e.g. `timing.output_path`).
fn emit_native_path_ref(out: &mut String, expr: &Expr, ctx: &mut EmitCtx) -> Result<bool, RhError> {
    if emit_native_string(out, expr, ctx)? {
        return Ok(true);
    }
    if let Expr::Variable(ident, ..) = expr
        && ctx.scope.get(ident.1.as_str()).copied() == Some(ValueKind::Json)
    {
        out.push_str("&rh_json_as_str(&");
        out.push_str(ident.1.as_str());
        out.push(')');
        return Ok(true);
    }
    if let Some(path) = path_parent_display_arg(expr) {
        out.push_str("&rh_path_parent(");
        if !emit_native_path_ref(out, path, ctx)? {
            return Ok(false);
        }
        out.push(')');
        return Ok(true);
    }
    if let Some((base, child)) = path_join_display_args(expr) {
        out.push_str("&rh_path_join(");
        if !emit_native_path_ref(out, base, ctx)? {
            return Ok(false);
        }
        out.push_str(", ");
        if !emit_native_path_ref(out, child, ctx)? {
            return Ok(false);
        }
        out.push(')');
        return Ok(true);
    }
    if let Some(path) = path_absolute_display_arg(expr) {
        out.push_str("&rh_path_absolute(");
        if !emit_native_path_ref(out, path, ctx)? {
            return Ok(false);
        }
        out.push(')');
        return Ok(true);
    }
    if let Some(path) = path_buf_from_display_arg(expr) {
        // PathBuf::from is a UTF-8 path string in native packs.
        emit_native_path_ref(out, path, ctx)
    } else {
        Ok(false)
    }
}

fn string_for_binding<'a>(expr: &'a Expr, ctx: &'a EmitCtx) -> Option<&'a str> {
    let Expr::Variable(ident, ..) = expr else {
        return None;
    };
    (ctx.scope.get(ident.1.as_str()).copied() == Some(ValueKind::String))
        .then_some(ident.1.as_str())
}

#[derive(Clone)]
enum StringReceiver<'a> {
    Binding(&'a str),
    JsonBinding(&'a str),
    JsonPath {
        binding: &'a str,
        path: Vec<&'a str>,
    },
    Literal(&'a str),
    DirEntryField {
        binding: &'a str,
        field: &'a str,
    },
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
            if let Expr::Dot(inner, ..) = &boxed.rhs {
                let mut path = Vec::new();
                if append_json_properties(&inner.lhs, &mut path) {
                    StringReceiver::JsonPath {
                        binding: ident.1.as_str(),
                        path,
                    }
                } else {
                    StringReceiver::JsonBinding(ident.1.as_str())
                }
            } else {
                StringReceiver::JsonBinding(ident.1.as_str())
            }
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
        StringReceiver::JsonPath { binding, path } => {
            out.push_str("rh_json_string_path(&");
            out.push_str(binding);
            out.push_str(", ");
            emit_json_path(out, &path);
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

fn emit_string_needle(out: &mut String, expr: &Expr, ctx: &mut EmitCtx) -> Result<bool, RhError> {
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
        _ if string_concat_args(expr, ctx).is_some()
            || json_value_path(expr, ctx).is_some_and(|(_, path)| !path.is_empty())
            || json_path_array_index(expr, ctx).is_some()
            || string_list_index(expr, ctx).is_some()
            || args_index_expr(expr).is_some() =>
        {
            out.push('&');
            emit_stringish(out, expr, ctx)?;
            Ok(true)
        }
        _ => Ok(false),
    }
}

fn string_plus_int_assign<'a>(
    assign: &'a (OpAssignment, BinaryExpr),
    ctx: &EmitCtx,
) -> Option<(&'a str, &'a Expr)> {
    let (op, bin) = assign;
    let (_, _, _, syntax, _, _) = op.get_op_assignment_info()?;
    if syntax != "+=" {
        return None;
    }
    let Expr::Variable(ident, ..) = &bin.lhs else {
        return None;
    };
    if !matches!(
        ctx.scope.get(ident.1.as_str()),
        Some(ValueKind::String | ValueKind::Path)
    ) {
        return None;
    }
    if is_explicit_string_expr(&bin.rhs, ctx) {
        return None;
    }
    (is_pure_int_expr(&bin.rhs) || is_native_json_int_expr(&bin.rhs, ctx))
        .then_some((ident.1.as_str(), &bin.rhs))
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

fn emit_set_key(out: &mut String, key: &Expr, ctx: &mut EmitCtx) -> Result<(), RhError> {
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
        Expr::Variable(ident, ..)
            if ctx.scope.get(ident.1.as_str()).copied() == Some(ValueKind::Path) =>
        {
            out.push_str(ident.1.as_str());
            out.push_str(".clone()");
            Ok(())
        }
        _ if string_concat_args(key, ctx).is_some()
            || json_value_path(key, ctx).is_some_and(|(_, path)| !path.is_empty())
            || json_path_array_index(key, ctx).is_some()
            || json_path_key_get(key, ctx).is_some()
            || string_list_index(key, ctx).is_some()
            || args_index_expr(key).is_some() =>
        {
            emit_stringish(out, key, ctx)
        }
        _ => {
            let rendered = crate::expr_print::expr_to_rhai(key).unwrap_or_else(|_| "<key>".into());
            Err(RhError::Transpile(format!(
                "set index key must be a string binding, literal, or stringish path (key={rendered}, expr={key:?})"
            )))
        }
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

fn emit_json_path_contains(
    out: &mut String,
    expr: &Expr,
    ctx: &mut EmitCtx,
) -> Result<bool, RhError> {
    let Some((binding, path, needle)) = json_contains_path(expr, ctx) else {
        return Ok(false);
    };
    // Unsupported needle shapes fail native emit instead of host-eval fallback.
    let mut needle_rust = String::new();
    if emit_json_value_expr(&mut needle_rust, needle, ctx).is_err() {
        return Ok(false);
    }
    out.push_str("rh_json_contains_path(&");
    out.push_str(binding);
    out.push_str(", ");
    emit_json_path(out, &path);
    out.push_str(", &");
    out.push_str(&needle_rust);
    out.push(')');
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
    if !matches!(
        call.name.as_str(),
        "contains" | "starts_with" | "ends_with" | "index_of"
    ) || call.args.len() != 1
    {
        return Ok(false);
    }
    let mut needle = String::new();
    if !emit_string_needle(&mut needle, &call.args[0], ctx)? {
        return Ok(false);
    }
    if call.name == "index_of" {
        out.push_str("rh_string_index_of(&");
        emit_string_receiver(out, receiver);
        out.push_str(", &");
        out.push_str(&needle);
        out.push(')');
        return Ok(true);
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

fn emit_string_parse_int(
    out: &mut String,
    expr: &Expr,
    ctx: &mut EmitCtx,
) -> Result<bool, RhError> {
    let Some((receiver, call)) = parse_string_method_call(expr, ctx) else {
        return Ok(false);
    };
    if call.name != "parse_int" || !call.args.is_empty() {
        return Ok(false);
    }
    out.push_str("rh_string_parse_int(&");
    emit_string_receiver(out, receiver);
    out.push(')');
    Ok(true)
}

fn emit_string_sub_string(
    out: &mut String,
    expr: &Expr,
    ctx: &mut EmitCtx,
) -> Result<bool, RhError> {
    let Some((receiver, call)) = parse_string_method_call(expr, ctx) else {
        return Ok(false);
    };
    if call.name != "sub_string" || call.args.is_empty() || call.args.len() > 2 {
        return Ok(false);
    }
    if !is_pure_int_expr(&call.args[0])
        || call
            .args
            .get(1)
            .is_some_and(|argument| !is_pure_int_expr(argument))
    {
        return Ok(false);
    }
    out.push_str("rh_string_sub_string(&");
    emit_string_receiver(out, receiver);
    out.push_str(", ");
    emit_expr(out, &call.args[0], ctx)?;
    out.push_str(", ");
    if call.args.len() == 2 {
        out.push_str("Some(");
        emit_expr(out, &call.args[1], ctx)?;
        out.push(')');
    } else {
        out.push_str("None");
    }
    out.push(')');
    Ok(true)
}

fn emit_string_method_expr(
    out: &mut String,
    expr: &Expr,
    ctx: &mut EmitCtx,
) -> Result<bool, RhError> {
    if emit_string_method_on_path_display(out, expr, ctx)? {
        return Ok(true);
    }
    let Some((receiver, call)) = parse_string_method_call(expr, ctx) else {
        return Ok(false);
    };
    match (receiver, call.name.as_str()) {
        (StringReceiver::Binding(binding), "to_lower") if call.args.is_empty() => {
            out.push_str(binding);
            out.push_str(".to_ascii_lowercase()");
            Ok(true)
        }
        (StringReceiver::JsonBinding(binding), "to_lower") if call.args.is_empty() => {
            out.push_str("rh_json_as_str(&");
            out.push_str(binding);
            out.push_str(").to_ascii_lowercase()");
            Ok(true)
        }
        (StringReceiver::JsonPath { binding, path }, "to_lower") if call.args.is_empty() => {
            out.push_str("rh_json_string_path(&");
            out.push_str(binding);
            out.push_str(", ");
            emit_json_path(out, &path);
            out.push_str(").to_ascii_lowercase()");
            Ok(true)
        }
        (StringReceiver::Binding(binding), "trim") if call.args.is_empty() => {
            out.push_str(binding);
            out.push_str(".trim().to_string()");
            Ok(true)
        }
        (StringReceiver::JsonBinding(binding), "trim") if call.args.is_empty() => {
            out.push_str("rh_json_as_str(&");
            out.push_str(binding);
            out.push_str(").trim().to_string()");
            Ok(true)
        }
        (StringReceiver::JsonPath { binding, path }, "trim") if call.args.is_empty() => {
            out.push_str("rh_json_string_path(&");
            out.push_str(binding);
            out.push_str(", ");
            emit_json_path(out, &path);
            out.push_str(").trim().to_string()");
            Ok(true)
        }
        _ => Ok(false),
    }
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
    if emit_native_string(&mut base_expr, base, ctx)?
        && emit_native_string(&mut child_expr, child, ctx)?
    {
        out.push_str("rh_path_join(");
        out.push_str(&base_expr);
        out.push_str(", ");
        out.push_str(&child_expr);
        out.push(')');
        return Ok(true);
    }
    // Borrow both sides so String bindings remain usable after join
    // (rh_path_join2 used to move owned temps/bindings).
    out.push_str("rh_path_join(&");
    emit_stringish(out, base, ctx)?;
    out.push_str(", &");
    emit_stringish(out, child, ctx)?;
    out.push(')');
    Ok(true)
}

fn emit_path_absolute(out: &mut String, path: &Expr, ctx: &mut EmitCtx) -> Result<bool, RhError> {
    let mut path_expr = String::new();
    if emit_native_string(&mut path_expr, path, ctx)? {
        out.push_str("rh_path_absolute(");
        out.push_str(&path_expr);
        out.push(')');
        return Ok(true);
    }
    // Nested path.display / join / absolute args need stringish emit.
    out.push_str("rh_path_absolute(&");
    emit_stringish(out, path, ctx)?;
    out.push(')');
    Ok(true)
}

fn emit_path_parent(out: &mut String, path: &Expr, ctx: &mut EmitCtx) -> Result<bool, RhError> {
    let mut path_expr = String::new();
    if emit_native_string(&mut path_expr, path, ctx)? {
        out.push_str("rh_path_parent(");
        out.push_str(&path_expr);
        out.push(')');
        return Ok(true);
    }
    // Nested `parent(absolute(p).display)` and other stringish paths.
    out.push_str("rh_path_parent(&");
    emit_stringish(out, path, ctx)?;
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

fn emit_std_fs_metadata(out: &mut String, path: &Expr, ctx: &mut EmitCtx) -> Result<bool, RhError> {
    let mut path_expr = String::new();
    if !emit_native_string(&mut path_expr, path, ctx)? {
        return Ok(false);
    }
    out.push_str("rh_metadata(");
    out.push_str(&path_expr);
    out.push(')');
    Ok(true)
}

fn emit_path_buf_from(out: &mut String, path: &Expr, ctx: &mut EmitCtx) -> Result<bool, RhError> {
    // PathBuf is represented as a UTF-8 path string in native packs.
    // Clone existing String/Path bindings so later uses of the same name remain valid.
    if let Expr::Variable(ident, ..) = path
        && matches!(
            ctx.scope.get(ident.1.as_str()).copied(),
            Some(ValueKind::String | ValueKind::Path)
        )
    {
        out.push_str(ident.1.as_str());
        out.push_str(".clone()");
        return Ok(true);
    }
    emit_stringish(out, path, ctx)?;
    Ok(true)
}

fn emit_path_is_absolute(
    out: &mut String,
    path: &Expr,
    ctx: &mut EmitCtx,
) -> Result<bool, RhError> {
    let mut path_expr = String::new();
    if !emit_native_string(&mut path_expr, path, ctx)? {
        return Ok(false);
    }
    out.push_str("rh_path_is_absolute(");
    out.push_str(&path_expr);
    out.push(')');
    Ok(true)
}

fn emit_path_file_name(out: &mut String, path: &Expr, ctx: &mut EmitCtx) -> Result<bool, RhError> {
    let mut path_expr = String::new();
    if emit_native_string(&mut path_expr, path, ctx)? {
        out.push_str("rh_path_file_name(");
        out.push_str(&path_expr);
        out.push(')');
        return Ok(true);
    }
    out.push_str("rh_path_file_name(&");
    emit_stringish(out, path, ctx)?;
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

fn emit_std_fs_write(
    out: &mut String,
    path: &Expr,
    contents: &Expr,
    ctx: &mut EmitCtx,
) -> Result<bool, RhError> {
    let mut path_expr = String::new();
    if !emit_native_string(&mut path_expr, path, ctx)? {
        return Ok(false);
    }
    out.push_str("rh_std_fs_write(");
    out.push_str(&path_expr);
    out.push_str(", &");
    emit_stringish(out, contents, ctx)?;
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
    if !emit_native_path_ref(&mut path_expr, path, ctx)? {
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

fn emit_std_fs_remove_dir_all(
    out: &mut String,
    path: &Expr,
    ctx: &mut EmitCtx,
) -> Result<bool, RhError> {
    let mut path_expr = String::new();
    if !emit_native_string(&mut path_expr, path, ctx)? {
        return Ok(false);
    }
    out.push_str("rh_remove_dir_all(");
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

fn emit_fs_metadata_call(
    out: &mut String,
    path: &Expr,
    call_name: &str,
    ctx: &mut EmitCtx,
) -> Result<bool, RhError> {
    let mut path_expr = String::new();
    if !emit_native_string(&mut path_expr, path, ctx)? {
        return Ok(false);
    }
    let helper = if call_name == "symlink_metadata" {
        "rh_symlink_metadata"
    } else {
        "rh_metadata"
    };
    out.push_str(helper);
    out.push('(');
    out.push_str(&path_expr);
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
    let (helper, path, property) = if let Some((path, property)) = symlink_metadata_property(expr) {
        ("rh_symlink_metadata", path, property)
    } else if let Some((path, property)) = metadata_call_property(expr) {
        ("rh_metadata", path, property)
    } else {
        return Ok(false);
    };
    let mut path_expr = String::new();
    if !emit_native_string(&mut path_expr, path, ctx)? {
        return Ok(false);
    }
    out.push_str(helper);
    out.push('(');
    out.push_str(&path_expr);
    out.push(')');
    out.push('.');
    out.push_str(property);
    Ok(true)
}

fn emit_dir_entry_property(out: &mut String, expr: &Expr, ctx: &EmitCtx) -> Result<bool, RhError> {
    if let Some(binding) = dir_entry_metadata_len(expr, ctx) {
        out.push_str("rh_metadata(&");
        out.push_str(binding);
        out.push_str(".path).len");
        return Ok(true);
    }
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
        return Err(RhError::Transpile(
            "throw outside try requires native cdylib lowering".into(),
        ));
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
    if call.namespace.is_empty() && is_local_fn_call(call.name.as_str(), ctx) && ctx.cdylib {
        let resolved = resolve_local_fn_name(call.name.as_str(), ctx).expect("checked local fn");
        let sig = ctx
            .local_fn_sigs
            .get(resolved.as_str())
            .cloned()
            .unwrap_or_default();
        out.push_str(resolved.as_str());
        out.push('(');
        for (index, arg) in call.args.iter().enumerate() {
            if index > 0 {
                out.push_str(", ");
            }
            match sig.get(index).copied() {
                Some(ValueKind::String) => emit_string_arg(out, arg, ctx)?,
                Some(ValueKind::Json) => emit_json_arg(out, arg, ctx)?,
                Some(ValueKind::ChildList) => emit_child_list_arg(out, arg, ctx)?,
                Some(ValueKind::StringList) => emit_string_list_arg(out, arg, ctx)?,
                Some(ValueKind::Output) => {
                    if let Expr::Variable(ident, ..) = arg
                        && ctx.scope.get(ident.1.as_str()).copied() == Some(ValueKind::Output)
                    {
                        // Clone so callers can keep reading fields after the call.
                        out.push_str(ident.1.as_str());
                        out.push_str(".clone()");
                    } else {
                        emit_expr(out, arg, ctx)?;
                    }
                }
                Some(ValueKind::Child) => {
                    if let Expr::Variable(ident, ..) = arg
                        && ctx.scope.get(ident.1.as_str()).copied() == Some(ValueKind::Child)
                    {
                        out.push_str(ident.1.as_str());
                        out.push_str(".clone()");
                    } else {
                        emit_expr(out, arg, ctx)?;
                    }
                }
                Some(ValueKind::Command) => {
                    if let Expr::Variable(ident, ..) = arg
                        && ctx.scope.get(ident.1.as_str()).copied() == Some(ValueKind::Command)
                    {
                        // By-value `RhCommand` params; clone so callers keep the binding.
                        out.push_str(ident.1.as_str());
                        out.push_str(".clone()");
                    } else {
                        emit_expr(out, arg, ctx)?;
                    }
                }
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
        Expr::Variable(ident, ..)
            if ctx.scope.get(ident.1.as_str()).copied() == Some(ValueKind::Set) =>
        {
            // Empty `#{}` binds as Set; at JSON local-fn boundaries it is `{}`.
            let _ = ident;
            out.push_str("serde_json::json!({})");
            Ok(())
        }
        Expr::Variable(ident, ..)
            if ctx.scope.get(ident.1.as_str()).copied() == Some(ValueKind::StringList) =>
        {
            // StringList arrays (e.g. path lists) coerce to JSON string arrays at
            // typed local-fn boundaries.
            out.push_str("serde_json::Value::Array(");
            out.push_str(ident.1.as_str());
            out.push_str(".iter().cloned().map(serde_json::Value::String).collect())");
            Ok(())
        }
        Expr::FnCall(call, ..)
            if call.op_token.is_none()
                && call.namespace.is_empty()
                && is_local_fn_call(call.name.as_str(), ctx)
                && matches!(
                    ctx.local_fn_return_kinds
                        .get(
                            resolve_local_fn_name(call.name.as_str(), ctx)
                                .expect("checked local fn")
                                .as_str(),
                        )
                        .copied(),
                    Some(ValueKind::Json)
                ) =>
        {
            // `add_result(..., evidence_lines(gate.evidence))` — Json-returning
            // local calls are valid JSON arguments.
            emit_call(out, call, ctx)
        }
        Expr::FnCall(call, ..)
            if call.op_token.is_none()
                && call.namespace.is_empty()
                && is_local_fn_call(call.name.as_str(), ctx)
                && matches!(
                    ctx.local_fn_return_kinds
                        .get(
                            resolve_local_fn_name(call.name.as_str(), ctx)
                                .expect("checked local fn")
                                .as_str(),
                        )
                        .copied(),
                    Some(ValueKind::StringList)
                ) =>
        {
            out.push_str("serde_json::Value::Array((");
            emit_call(out, call, ctx)?;
            out.push_str(").into_iter().map(serde_json::Value::String).collect())");
            Ok(())
        }
        Expr::Map(..) | Expr::Array(..) => emit_json_value_expr(out, expr, ctx),
        _ if json_value_path(expr, ctx).is_some()
            || json_array_index(expr, ctx).is_some()
            || json_path_array_index(expr, ctx).is_some()
            || json_path_key_get(expr, ctx).is_some()
            || json_rhai_array_index_property(expr, ctx).is_some() =>
        {
            emit_json_value_expr(out, expr, ctx)
        }
        _ => Err(RhError::Transpile(format!(
            "local fn JSON argument must be a JSON value (expr={expr:?})"
        ))),
    }
}

fn emit_child_list_arg(out: &mut String, expr: &Expr, ctx: &mut EmitCtx) -> Result<(), RhError> {
    match expr {
        Expr::Variable(ident, ..)
            if ctx.scope.get(ident.1.as_str()).copied() == Some(ValueKind::ChildList) =>
        {
            out.push_str(ident.1.as_str());
            out.push_str(".clone()");
            Ok(())
        }
        Expr::Array(items, ..) if !items.is_empty() => {
            out.push_str("vec![");
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                match item {
                    Expr::Variable(ident, ..)
                        if ctx.scope.get(ident.1.as_str()).copied() == Some(ValueKind::Child) =>
                    {
                        out.push_str("rh_child_share(&mut ");
                        out.push_str(ident.1.as_str());
                        out.push(')');
                    }
                    _ => {
                        return Err(RhError::Transpile(format!(
                            "child list literal items must be Child bindings (expr={item:?})"
                        )));
                    }
                }
            }
            out.push(']');
            Ok(())
        }
        Expr::Array(..) => {
            out.push_str("Vec::new()");
            Ok(())
        }
        _ => Err(RhError::Transpile(format!(
            "local fn child-list argument must be a Child array (expr={expr:?})"
        ))),
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

fn local_fn_returns_kind(expr: &Expr, ctx: &EmitCtx, kind: ValueKind) -> bool {
    let Expr::FnCall(call, ..) = expr else {
        return false;
    };
    if call.op_token.is_some()
        || !call.namespace.is_empty()
        || !is_local_fn_call(call.name.as_str(), ctx)
    {
        return false;
    }
    let Some(resolved) = resolve_local_fn_name(call.name.as_str(), ctx) else {
        return false;
    };
    ctx.local_fn_return_kinds.get(resolved.as_str()).copied() == Some(kind)
}

fn emit_string_list_producing_call(
    out: &mut String,
    expr: &Expr,
    ctx: &mut EmitCtx,
) -> Result<bool, RhError> {
    if !local_fn_returns_kind(expr, ctx, ValueKind::StringList) {
        return Ok(false);
    }
    let Expr::FnCall(call, ..) = expr else {
        return Ok(false);
    };
    emit_call(out, call, ctx)?;
    Ok(true)
}

fn emit_string_list_arg(out: &mut String, expr: &Expr, ctx: &mut EmitCtx) -> Result<(), RhError> {
    match expr {
        Expr::Variable(ident, ..)
            if ctx.scope.get(ident.1.as_str()).copied() == Some(ValueKind::StringList) =>
        {
            out.push_str(ident.1.as_str());
            out.push_str(".clone()");
            Ok(())
        }
        Expr::Variable(ident, ..)
            if ctx.scope.get(ident.1.as_str()).copied() == Some(ValueKind::Json) =>
        {
            // JSON string arrays (common harness argv vectors) coerce at the
            // typed StringList boundary.
            out.push_str("rh_json_array_items(&");
            out.push_str(ident.1.as_str());
            out.push_str(", &[]).into_iter().map(|value| rh_json_as_str(&value)).collect()");
            Ok(())
        }
        _ if let Some((binding, path)) = json_value_path(expr, ctx)
            && !path.is_empty() =>
        {
            out.push_str("rh_json_array_items(&");
            out.push_str(binding);
            out.push_str(", ");
            emit_json_path(out, &path);
            out.push_str(").into_iter().map(|value| rh_json_as_str(&value)).collect()");
            Ok(())
        }
        Expr::Array(items, ..) => {
            out.push_str("vec![");
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                if let Expr::Variable(ident, ..) = item
                    && matches!(
                        ctx.scope.get(ident.1.as_str()).copied(),
                        Some(ValueKind::String | ValueKind::Path)
                    )
                {
                    out.push_str(ident.1.as_str());
                    out.push_str(".clone()");
                } else {
                    emit_stringish(out, item, ctx)?;
                }
            }
            out.push(']');
            Ok(())
        }
        _ if emit_string_list_producing_call(out, expr, ctx)? => Ok(()),
        _ => Err(RhError::Transpile(format!(
            "local fn string-list argument must be a string array (expr={expr:?})"
        ))),
    }
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

fn is_unit_expr(expr: &Expr) -> bool {
    matches!(expr, Expr::Unit(..))
}

fn is_json_null_compare_value(expr: &Expr, ctx: &EmitCtx) -> bool {
    if let Expr::Variable(ident, ..) = expr {
        return matches!(
            ctx.scope.get(ident.1.as_str()).copied(),
            Some(ValueKind::Json)
        );
    }
    json_path_key_get(expr, ctx).is_some()
        || json_path_array_index(expr, ctx).is_some()
        || json_array_index(expr, ctx).is_some()
        || json_rhai_array_index_property(expr, ctx).is_some()
        || json_value_path(expr, ctx).is_some()
}

/// Operand of a json-to-json equality: any json value expression
/// `is_json_null_compare_value` accepts, plus a zero-argument local fn call
/// whose inferred return kind is Json (the bundled `alias__null_json()`
/// helper shape the qualification gate compares against).
fn is_json_equality_operand(expr: &Expr, ctx: &EmitCtx) -> bool {
    // `doc.items.len` parses as a json path ending in `len`, but it is an
    // INT surface, not a json value — routing `a.len == b.len` through
    // serde_json equality emitted `rh_json_get_path(.., ["len"])`, which
    // fails closed at runtime (prd-alignment's module-count compare).
    if json_array_len_path(expr, ctx).is_some()
        || json_object_keys_len_path(expr, ctx).is_some()
        || set_keys_len_path(expr, ctx).is_some()
    {
        return false;
    }
    if let Expr::FnCall(call, ..) = expr
        && call.namespace.is_empty()
        && call.args.is_empty()
        && ctx.local_fn_return_kinds.get(call.name.as_str()).copied() == Some(ValueKind::Json)
    {
        return true;
    }
    is_json_null_compare_value(expr, ctx)
}

fn json_unit_compare_pair<'a>(
    lhs: &'a Expr,
    rhs: &'a Expr,
    ctx: &EmitCtx,
) -> Option<(&'a Expr, bool)> {
    if is_unit_expr(rhs) && is_json_null_compare_value(lhs, ctx) {
        Some((lhs, true))
    } else if is_unit_expr(lhs) && is_json_null_compare_value(rhs, ctx) {
        Some((rhs, true))
    } else {
        None
    }
}

fn emit_json_value_operand(
    out: &mut String,
    expr: &Expr,
    ctx: &mut EmitCtx,
) -> Result<(), RhError> {
    if let Expr::Variable(ident, ..) = expr
        && ctx.scope.get(ident.1.as_str()).copied() == Some(ValueKind::Json)
    {
        out.push_str(ident.1.as_str());
        return Ok(());
    }
    if let Expr::FnCall(call, ..) = expr
        && call.namespace.is_empty()
        && call.args.is_empty()
        && ctx.local_fn_return_kinds.get(call.name.as_str()).copied() == Some(ValueKind::Json)
    {
        out.push_str(call.name.as_str());
        out.push_str("()");
        return Ok(());
    }
    if let Some((binding, path, key)) = json_path_key_get(expr, ctx) {
        out.push_str("rh_json_get_path_key(&");
        out.push_str(binding);
        out.push_str(", ");
        emit_json_path(out, &path);
        out.push_str(", &");
        emit_json_map_key(out, key, ctx)?;
        out.push(')');
        return Ok(());
    }
    if let Some((binding, path, index)) = json_path_array_index(expr, ctx) {
        out.push_str("rh_json_get_path_index(&");
        out.push_str(binding);
        out.push_str(", ");
        emit_json_path(out, &path);
        out.push_str(", ");
        emit_intish(out, index, ctx)?;
        out.push(')');
        return Ok(());
    }
    if let Some((binding, index)) = json_array_index(expr, ctx) {
        out.push_str("rh_json_array_get(&");
        out.push_str(binding);
        out.push_str(", ");
        emit_intish(out, index, ctx)?;
        out.push(')');
        return Ok(());
    }
    if let Some((binding, path, index, field)) = json_rhai_array_index_property(expr, ctx) {
        out.push_str("rh_json_get_path(&rh_json_get_path_index(&");
        out.push_str(binding);
        out.push_str(", ");
        emit_json_path(out, &path);
        out.push_str(", ");
        emit_intish(out, index, ctx)?;
        out.push_str("), &[");
        out.push_str(&format!("{field:?}"));
        out.push_str("])");
        return Ok(());
    }
    if let Some((binding, path)) = json_value_path(expr, ctx) {
        if path.is_empty() {
            out.push_str(binding);
        } else {
            out.push_str("rh_json_get_path(&");
            out.push_str(binding);
            out.push_str(", ");
            emit_json_path(out, &path);
            out.push(')');
        }
        return Ok(());
    }
    Err(RhError::Transpile(format!(
        "json null compare operand must be a JSON value (expr={expr:?})"
    )))
}

fn comparison_binary(
    out: &mut String,
    op: &str,
    lhs: &Expr,
    rhs: &Expr,
    ctx: &mut EmitCtx,
) -> Result<(), RhError> {
    if ctx.cdylib {
        if let Some((list, rhs)) = string_list_compare_pair(lhs, rhs, ctx) {
            out.push('(');
            if op == "==" {
                out.push_str(list);
                out.push_str(" == vec![");
                emit_stringish(out, rhs, ctx)?;
                out.push(']');
            } else if op == "!=" {
                out.push('!');
                out.push('(');
                out.push_str(list);
                out.push_str(" == vec![");
                emit_stringish(out, rhs, ctx)?;
                out.push_str("])");
            } else {
                return Err(RhError::Transpile(format!(
                    "unsupported string-list comparison `{op}`"
                )));
            }
            out.push_str(") as INT");
            return Ok(());
        }
        if let Some((json_expr, _)) = json_unit_compare_pair(lhs, rhs, ctx) {
            // `((value.is_null())) as INT` / `(!(value.is_null())) as INT`
            // so `!` applies to the bool before the INT cast.
            out.push('(');
            if op == "!=" {
                out.push('!');
            }
            out.push('(');
            emit_json_value_operand(out, json_expr, ctx)?;
            out.push_str(".is_null())) as INT");
            return Ok(());
        }
        if (op == "==" || op == "!=")
            && is_json_equality_operand(lhs, ctx)
            && is_json_equality_operand(rhs, ctx)
        {
            // json == json compares as real serde_json::Value equality.
            // Routing both sides through string coercion was lossy and
            // fail-closed on null — the qualification gate's
            // `timing.first_failure == null_json()` died exactly there
            // whenever there was no failure to report.
            out.push('(');
            if op == "!=" {
                out.push('!');
            }
            out.push('(');
            emit_json_value_operand(out, lhs, ctx)?;
            out.push_str(" == ");
            emit_json_value_operand(out, rhs, ctx)?;
            out.push_str(")) as INT");
            return Ok(());
        }
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
        CdylibExecutionMode, CdylibTranspileOutput, RhError, transpile, transpile_cdylib,
        transpile_cdylib_with_mode, transpile_cdylib_with_project,
    };

    const NATIVE_ASSIGN_BLOCKERS: &[&str] = &[
        "assignment lhs must be a variable",
        "set index key must be a string binding",
    ];

    fn assert_transpile_past_assign_lhs_blockers(
        root: &std::path::Path,
        rel: &str,
    ) -> Result<CdylibTranspileOutput, RhError> {
        let source = std::fs::read_to_string(root.join(rel)).expect("read script");
        match transpile_cdylib_with_project(root, &source) {
            Ok(output) => Ok(output),
            Err(RhError::Transpile(msg)) => {
                for needle in NATIVE_ASSIGN_BLOCKERS {
                    assert!(
                        !msg.contains(needle),
                        "{rel} still blocked by assign/set-index emit: {msg}"
                    );
                }
                Err(RhError::Transpile(msg))
            }
            Err(other) => Err(other),
        }
    }

    #[test]
    fn transpiles_add_fn() {
        let rust = transpile("fn add(a, b) { a + b }").expect("transpile");
        assert!(rust.contains("pub fn add"));
        assert!(rust.contains("a + b"));
    }

    /// `PathBuf::from(variable)` passes the string through, but a bare
    /// variable there MOVES it into the new binding while the script (value
    /// semantics) keeps using the original — prune-target-incremental's
    /// `target_argument` produced a live E0382 in the generated pack. The
    /// passthrough must clone variables.
    #[test]
    fn path_buf_from_variable_clones_instead_of_moving() {
        let rust = transpile_cdylib_with_mode(
            "fn entry() {\n\
                 let target_argument = \"target\";\n\
                 let candidate = std::path::PathBuf::from(target_argument);\n\
                 let joined = std::path::join(\"root\", target_argument).display;\n\
                 if candidate.is_absolute != 0 {\n\
                     print(joined);\n\
                 }\n\
                 0\n\
             }",
        )
        .expect("transpile")
        .rust;
        assert!(
            rust.contains("target_argument.clone()"),
            "PathBuf::from(variable) must clone the passthrough: {rust}"
        );
    }

    /// `a.len == b.len` on json arrays is an INT compare of lengths; the
    /// json-to-json equality lane must not capture it and turn `len` into
    /// a path key (prd-alignment died at runtime on `json_path: len`).
    #[test]
    fn json_len_compare_emits_array_len_not_path_lookup() {
        let rust = transpile_cdylib(
            "fn entry() {\n\
                 let actual = [];\n\
                 let linked = [];\n\
                 if actual.len == linked.len { return 1; }\n\
                 0\n\
             }",
        )
        .expect("transpile");
        assert!(
            rust.contains("rh_json_array_len"),
            "len compare should use rh_json_array_len: {rust}"
        );
        assert!(
            !rust.contains("&[\"len\"]"),
            "len must not become a json path key: {rust}"
        );
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
    fn compat_delegating_pack_rejects_unsupported_construct() {
        let err = transpile_cdylib("fn entry() { switch 1 { 1 => 42, _ => 0 } }")
            .expect_err("switch must not compat-delegate");
        assert!(
            matches!(err, RhError::Subset { .. } | RhError::Transpile(_)),
            "expected subset/transpile error, got {err:?}"
        );
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
    fn char_to_string_assignment_stays_native() {
        let source = r#"
fn entry() {
    let text = "abc";
    let pieces = [];
    for character in text {
        let piece = character.to_string();
        pieces.push(piece);
    }
    require(pieces.len == 3, "pieces");
    0
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
                .contains("let mut piece = character.to_string();"),
            "{}",
            output.rust
        );
        assert_eq!(output.rust.matches("rh_host_eval_int(\"").count(), 0);
    }

    #[test]
    fn cdylib_transpile_emits_json_path_array_string_index() {
        let source = include_str!("../../../fixtures/rh/json-path-index-probe.rh");
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
                .contains("rh_json_string_path_index(&doc, &[\"items\"], 0)"),
            "{}",
            output.rust
        );
        assert!(
            output
                .rust
                .contains("rh_json_string_path_index(&doc, &[\"nested\", \"probe\"], 0)"),
            "{}",
            output.rust
        );
        assert!(!output.rust.contains("rh_host_run_script(RH_SCRIPT_SOURCE)"));
        assert_eq!(output.rust.matches("rh_host_eval_int(\"").count(), 0);
    }

    #[test]
    fn cdylib_transpile_emits_json_object_keys_iteration() {
        let source = include_str!("../../../fixtures/rh/json-keys-probe.rh");
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
                .contains("for key in rh_json_object_keys(&obj, &[])"),
            "{}",
            output.rust
        );
        assert!(
            output.rust.contains("rh_json_object_keys_len(&obj, &[])"),
            "{}",
            output.rust
        );
        assert!(!output.rust.contains("rh_host_eval_int(\"for"));
        assert!(!output.rust.contains("rh_host_run_script(RH_SCRIPT_SOURCE)"));
        assert_eq!(output.rust.matches("rh_host_eval_int(\"").count(), 0);
    }

    #[test]
    fn cdylib_transpile_emits_int_string_concat() {
        let source = include_str!("../../../fixtures/rh/int-string-concat-probe.rh");
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
                .contains("s = format!(\"{}{}\", s, String::from(\"123\"))"),
            "{}",
            output.rust
        );
        assert!(
            output
                .rust
                .contains("s = format!(\"{}{}\", s, (n).to_string())"),
            "{}",
            output.rust
        );
        assert!(
            output
                .rust
                .contains("format!(\"{}{}\", String::from(\"prefix-\"), (n).to_string())"),
            "{}",
            output.rust
        );
        assert!(
            output.rust.contains("String::from(\".locked-\")")
                && output.rust.contains("(suffix).to_string()"),
            "{}",
            output.rust
        );
        assert!(!output.rust.contains("rh_host_eval_int(\"+"));
        assert_eq!(output.rust.matches("rh_host_eval_int(\"").count(), 0);
    }

    #[test]
    fn cdylib_transpile_emits_map_set_keys_iteration() {
        let output = transpile_cdylib_with_mode(
            "fn entry() { let names = #{}; names[\"a\"] = true; let count = 0; for key in names.keys() { count += 1; } if names.keys().len != 1 { return rh::fail(\"len\"); } count }",
        )
        .expect("transpile");
        assert_eq!(
            output.execution_mode,
            CdylibExecutionMode::Native,
            "{}",
            output.rust
        );
        assert!(
            output.rust.contains("for key in names.iter().cloned()"),
            "{}",
            output.rust
        );
        assert!(
            output.rust.contains("(names.len() as INT)"),
            "{}",
            output.rust
        );
        assert!(!output.rust.contains("rh_host_eval_int(\"for"));
        assert_eq!(output.rust.matches("rh_host_eval_int(\"").count(), 0);
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
        assert_eq!(output.rust.matches("rh_host_eval_int(\"").count(), 0);
    }

    #[test]
    fn cdylib_transpile_emits_json_array_index_in_map_return() {
        let source = include_str!("../../../fixtures/rh/json-array-index-map-return-probe.rh");
        let output = transpile_cdylib_with_mode(source).expect("transpile");
        assert_eq!(
            output.execution_mode,
            CdylibExecutionMode::Native,
            "{}",
            output.rust
        );
        assert!(
            output.rust.contains("rh_json_array_get(&matches, "),
            "{}",
            output.rust
        );
        assert!(
            !output.rust.contains("rh_json_get_path_key(&matches"),
            "{}",
            output.rust
        );
        assert!(!output.rust.contains("rh_host_run_script(RH_SCRIPT_SOURCE)"));
    }

    #[test]
    fn cdylib_transpile_emits_rhai_array_index_property_misparse() {
        let output = transpile_cdylib_with_mode(
            r#"fn entry() {
    let ordered = [#{ id: 1 }, #{ id: 3 }];
    let order_index = 0;
    let process = #{ id: 2 };
    if process.id < ordered[order_index].id {
        return 1;
    }
    0
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
            output.rust.contains("rh_json_get_path_index(&ordered"),
            "{}",
            output.rust
        );
        assert!(
            !output.rust.contains("rh_json_get_path_key(&ordered"),
            "{}",
            output.rust
        );
    }

    #[test]
    fn cdylib_transpile_emits_json_null_unit_compare() {
        let source = include_str!("../tests/fixtures/rh_null_unit_compare.rh");
        let output = transpile_cdylib_with_mode(source).expect("transpile");
        assert_eq!(
            output.execution_mode,
            CdylibExecutionMode::Native,
            "{}",
            output.rust
        );
        assert!(
            output.rust.contains("serde_json::Value::Null"),
            "{}",
            output.rust
        );
        assert!(
            output.rust.contains(".is_null())) as INT"),
            "null/unit compare must close bool group before INT cast: {}",
            output.rust
        );
        assert_eq!(output.rust.matches("rh_host_eval_int(\"").count(), 0);
    }

    #[test]
    fn json_path_array_index_binding_stays_json_not_string() {
        let output = transpile_cdylib_with_mode(
            r#"
fn active_tab_from_snapshot(snapshot) {
    for index in 0..snapshot.tabs.len {
        let tab = snapshot.tabs[index];
        if tab.active != 0 {
            return tab;
        }
    }
    rh::json::parse("null")
}

fn entry() {
    let snapshot = rh::json::parse("{\"tabs\":[{\"active\":1}]}");
    let tab = active_tab_from_snapshot(snapshot);
    if tab.active != 1 {
        return rh::fail("tab");
    }
    0
}
"#,
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
                .contains("rh_json_get_path_index(&snapshot, &[\"tabs\"], index)"),
            "{}",
            output.rust
        );
        assert!(
            !output
                .rust
                .contains("rh_json_string_path_index(&snapshot, &[\"tabs\"]"),
            "{}",
            output.rust
        );
        assert!(
            output
                .rust
                .contains("pub fn active_tab_from_snapshot(mut snapshot: serde_json::Value)"),
            "{}",
            output.rust
        );
    }

    #[test]
    fn cdylib_transpile_emits_json_map_key_string_read() {
        let output = transpile_cdylib_with_mode(
            r#"fn entry() {
    let identities = #{};
    let id = "proc-1";
    identities[id] = "identity-a";
    let name = "" + identities[id];
    if name != "identity-a" { return rh::fail("bad"); }
    0
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
            output
                .rust
                .contains("rh_json_as_str(&rh_json_get_path_key(&identities"),
            "{}",
            output.rust
        );
    }

    #[test]
    fn cdylib_transpile_emits_set_map_value_assignments() {
        let source = include_str!("../../../fixtures/rh/set-map-value-assign-probe.rh");
        let output = transpile_cdylib_with_mode(source).expect("transpile");
        assert_eq!(
            output.execution_mode,
            CdylibExecutionMode::Native,
            "{}",
            output.rust
        );
        assert!(
            output.rust.contains("rh_json_set_path_key(&mut identities"),
            "{}",
            output.rust
        );
        assert!(
            output.rust.contains("rh_json_set_path_key(&mut unique"),
            "{}",
            output.rust
        );
        assert!(
            !output.rust.contains("HashSet::<String>::new()"),
            "{}",
            output.rust
        );
    }

    #[test]
    fn cdylib_transpile_emits_json_array_index_assignment() {
        let source = include_str!("../../../fixtures/rh/json-array-index-assign-probe.rh");
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
                .contains("rh_json_set_path_index(&mut safe, &[], "),
            "{}",
            output.rust
        );
    }

    #[test]
    fn cdylib_transpile_emits_json_param_index_assignment() {
        let source = include_str!("../../../fixtures/rh/json-param-index-assign-probe.rh");
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
                .contains("rh_json_set_path_key(&mut states, &[], "),
            "{}",
            output.rust
        );
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
        assert_eq!(output.rust.matches("rh_host_eval_int(\"").count(), 0);
    }

    #[test]
    fn cdylib_transpile_emits_path_file_name_native() {
        let source = include_str!("../../../fixtures/rh/path-file-name-probe.rh");
        let output = transpile_cdylib_with_mode(source).expect("transpile");
        assert_eq!(
            output.execution_mode,
            CdylibExecutionMode::Native,
            "{}",
            output.rust
        );
        assert!(
            output.rust.contains("rh_path_file_name("),
            "{}",
            output.rust
        );
        assert!(!output.rust.contains("rh_host_run_script(RH_SCRIPT_SOURCE)"));
        assert_eq!(output.rust.matches("rh_host_eval_int(\"").count(), 0);
    }

    #[test]
    fn cdylib_transpile_emits_metadata_current_dir_is_absolute_parse_file() {
        let source = include_str!("../../../fixtures/rh/path-metadata-sugar.rh");
        let output = transpile_cdylib_with_mode(source).expect("transpile");
        assert_eq!(
            output.execution_mode,
            CdylibExecutionMode::Native,
            "{}",
            output.rust
        );
        assert!(
            output.rust.contains("rh_env_current_dir()"),
            "{}",
            output.rust
        );
        assert!(
            output.rust.contains("rh_path_is_absolute("),
            "{}",
            output.rust
        );
        assert!(output.rust.contains("rh_metadata("), "{}", output.rust);
        assert!(
            output
                .rust
                .contains("rh_json_parse(&rh_std_fs_read_to_string("),
            "{}",
            output.rust
        );
        assert!(output.rust.contains("meta.is_file"), "{}", output.rust);
        assert!(output.rust.contains("meta.len"), "{}", output.rust);
        assert!(!output.rust.contains("rh_host_run_script(RH_SCRIPT_SOURCE)"));
        assert_eq!(output.rust.matches("rh_host_eval_int(\"").count(), 0);
        assert!(
            !output.rust.contains("rh_host_eval_int(\"std::fs::metadata"),
            "{}",
            output.rust
        );
        assert!(
            !output
                .rust
                .contains("rh_host_eval_int(\"rh::json::parse_file"),
            "{}",
            output.rust
        );
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
        assert!(
            output.rust.contains("pub fn helper__add("),
            "{}",
            output.rust
        );
        assert!(
            output.rust.contains("helper__add(40, 2)"),
            "{}",
            output.rust
        );
        assert!(!output.rust.contains("rh_host_run_script(RH_SCRIPT_SOURCE)"));
        assert_eq!(output.rust.matches("rh_host_eval_int(\"").count(), 0);
    }

    #[test]
    fn cdylib_transpile_emits_nested_json_parse_read_to_string() {
        let output = transpile_cdylib_with_mode(
            "fn entry() { let path = args[0]; let doc = rh::json::parse(std::fs::read_to_string(path)); doc.schema_version }",
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
        assert!(output.rust.contains("\"current_dir\""), "{}", output.rust);
        assert!(output.rust.contains("\"env\""), "{}", output.rust);
        assert!(output.rust.contains("\"env_remove\""), "{}", output.rust);
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
        assert!(
            output.rust.contains("rh_process_status("),
            "{}",
            output.rust
        );
        assert!(output.rust.contains("\"current_dir\""), "{}", output.rust);
    }

    #[test]
    fn cdylib_transpile_emits_command_stdout_file_with_argv_variable() {
        let output = transpile_cdylib_with_mode(
            r#"fn entry() {
    let repo = args[0];
    let cargo_args = [];
    cargo_args.push("xwin");
    cargo_args.push("build");
    let stdout_path = repo + ".stdout.tmp";
    std::process::command_stdout_file(
        "cargo",
        cargo_args,
        3600000,
        stdout_path,
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
        assert!(
            output.rust.contains("rh_process_stdout_file("),
            "{}",
            output.rust
        );
        assert!(
            output.rust.contains("rh_json_string_argv(&cargo_args)"),
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
    fn cdylib_transpile_emits_command_status_with_argv_variable() {
        let output = transpile_cdylib_with_mode(
            r#"fn entry() {
    let worker = args[0];
    let prune_arguments = [
        "task", "run", "prune-target-incremental",
        "--manifest", "agenterm.tasks.json",
        "--", "."
    ];
    prune_arguments.push("--invocation");
    prune_arguments.push("abc123");
    std::process::command_status(worker, prune_arguments, 300000)
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
            output.rust.contains("rh_process_status("),
            "{}",
            output.rust
        );
        assert!(
            output
                .rust
                .contains("rh_process_status(&worker, &prune_arguments,")
                || output
                    .rust
                    .contains("rh_process_status(&worker, &prune_arguments, "),
            "{}",
            output.rust
        );
        assert!(
            !output
                .rust
                .contains("rh_host_eval_int(\"std::process::command_status")
        );
    }

    #[test]
    fn cdylib_transpile_emits_command_status_with_argv_param() {
        let output = transpile_cdylib_with_mode(
            r#"fn run_status(arguments, timeout_ms) {
    std::process::command_status("worker", arguments, timeout_ms)
}
fn entry() {
    let argv = ["task", "run", "stage-build"];
    run_status(argv, 30000)
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
            output.rust.contains("rh_process_status("),
            "{}",
            output.rust
        );
        assert!(
            output
                .rust
                .contains("rh_process_status(\"worker\", &arguments,")
                || output
                    .rust
                    .contains("rh_process_status(\"worker\", &arguments, "),
            "{}",
            output.rust
        );
        assert!(
            !output
                .rust
                .contains("rh_host_eval_int(\"std::process::command_status")
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
    let pretty = rh::json::stringify_pretty(manifest);
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
        let digest = rh::crypto::sha256_file(path);
        digest.to_lower();
        let _stamp = std::time::SystemTime::now().rfc3339;
        rh::runtime::atomic_write(path, digest);
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
    fn cdylib_transpile_emits_hash_fnv1a64_json_map_value_native() {
        let probe = r#"fn entry() {
    let workload_identity = #{
        lane: "quick",
        profile: "dev",
        gate_manifest_sha256: "abc"
    };
    let timing = #{
        workload: #{
            identity: workload_identity,
            fingerprint: rh::hash::fnv1a64(
                rh::bytes::from_text(
                    rh::json::stringify(workload_identity)
                )
            )
        }
    };
    timing.workload.fingerprint
}"#;
        let probe_path = format!("/tmp/rh_probe_hash_{}.rh", std::process::id());
        let _ = std::fs::write(&probe_path, probe);
        let output = transpile_cdylib_with_mode(probe).expect("transpile");
        assert_eq!(
            output.execution_mode,
            CdylibExecutionMode::Native,
            "{}",
            output.rust
        );
        assert!(
            output
                .rust
                .contains("rh_hash_fnv1a64(&rh_json_stringify(&workload_identity))"),
            "{}",
            output.rust
        );
        assert!(
            output.rust.contains("serde_json::Value::String("),
            "{}",
            output.rust
        );
        assert_eq!(output.rust.matches("rh_host_eval_int(\"").count(), 0);
    }

    #[test]
    fn cdylib_transpile_emits_env_get_param_binding() {
        let output = transpile_cdylib_with_mode(
            r#"fn env_value(name) {
    if !std::env::has(name) {
        return "";
    }
    std::env::get(name)
}
fn entry() { env_value("AGENTERM_BOOTSTRAP_SETUP_MS") }"#,
        )
        .expect("transpile");
        assert_eq!(
            output.execution_mode,
            CdylibExecutionMode::Native,
            "{}",
            output.rust
        );
        assert!(output.rust.contains("rh_env_has(&name)"), "{}", output.rust);
        assert!(output.rust.contains("rh_env_get(&name)"), "{}", output.rust);
        assert!(!output.rust.contains("rh_host_run_script(RH_SCRIPT_SOURCE)"));
    }

    #[test]
    fn cdylib_transpile_emits_env_get_parse_int_native() {
        let output = transpile_cdylib_with_mode(
            r#"fn entry() { std::env::get("AGENTERM_BOOTSTRAP_SETUP_MS").parse_int() }"#,
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
                .contains("rh_env_parse_int(\"AGENTERM_BOOTSTRAP_SETUP_MS\")"),
            "{}",
            output.rust
        );
        assert!(
            !output.rust.contains(
                "rh_host_eval_int(\"std::env::get(`AGENTERM_BOOTSTRAP_SETUP_MS`).parse_int()"
            ),
            "{}",
            output.rust
        );
        assert!(!output.rust.contains("rh_host_run_script(RH_SCRIPT_SOURCE)"));
    }

    #[test]
    fn cdylib_transpile_emits_string_binding_parse_int_native() {
        let output = transpile_cdylib_with_mode(
            r#"fn entry() {
    let budget_text = args[0];
    let budget = budget_text.parse_int();
    require(budget > 0, "budget");
    budget
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
            output
                .rust
                .contains("let mut budget = rh_string_parse_int(&budget_text);"),
            "{}",
            output.rust
        );
        assert_eq!(output.rust.matches("rh_host_eval_int(\"").count(), 0);
    }

    #[test]
    fn cdylib_transpile_emits_env_get_parse_int_param_binding() {
        let output = transpile_cdylib_with_mode(
            r#"fn read_ms(name) {
    if !std::env::has(name) {
        return -1;
    }
    std::env::get(name).parse_int()
}
fn entry() { read_ms("AGENTERM_BOOTSTRAP_SETUP_MS") }"#,
        )
        .expect("transpile");
        assert_eq!(
            output.execution_mode,
            CdylibExecutionMode::Native,
            "{}",
            output.rust
        );
        assert!(
            output.rust.contains("rh_env_parse_int(&name)"),
            "{}",
            output.rust
        );
        assert!(
            !output
                .rust
                .contains("rh_host_eval_int(\"std::env::get(name).parse_int()"),
            "{}",
            output.rust
        );
    }

    #[test]
    fn cdylib_transpile_json_state_field_plus_int_stays_native() {
        // `.state` is also an RhChild member name; JSON evidence (`.setup_ms`) must win,
        // string coerce must use rh_json_string_path, and `field + int` must stay INT.
        let output = transpile_cdylib_with_mode(
            r#"fn wall_time(bootstrap, task_ms) {
    let timing_state = "" + bootstrap.state;
    if timing_state == "measured" {
        bootstrap.setup_ms + task_ms
    } else {
        task_ms
    }
}
fn entry() {
    let bootstrap = #{ state: "measured", setup_ms: 3 };
    wall_time(bootstrap, 1)
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
            output.rust.contains(
                "pub fn wall_time(mut bootstrap: serde_json::Value, task_ms: INT) -> INT",
            ),
            "{}",
            output.rust
        );
        assert!(
            output
                .rust
                .contains("rh_json_string_path(&bootstrap, &[\"state\"])"),
            "{}",
            output.rust
        );
        assert!(
            output
                .rust
                .contains("rh_json_int_path(&bootstrap, &[\"setup_ms\"])"),
            "{}",
            output.rust
        );
        assert!(
            !output
                .rust
                .contains("rh_host_eval_int(\"bootstrap.setup_ms"),
            "{}",
            output.rust
        );
    }

    #[test]
    fn build_rh_project_transpiles_native() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("repo root");
        let source = std::fs::read_to_string(root.join("scripts/rh/build.rh")).expect("entry");
        let output = transpile_cdylib_with_project(&root, &source).expect("transpile");
        assert_eq!(
            output.execution_mode,
            CdylibExecutionMode::Native,
            "{}",
            output.rust
        );
        assert_eq!(output.rust.matches("rh_host_eval_int(\"").count(), 0);
        assert!(
            !output
                .rust
                .contains("rh_host_eval_int(\"std::process::command_"),
            "{}",
            output.rust
        );
        let bundled = crate::bundle_project_source(&root, &source).expect("bundle");
        let dir = tempfile::tempdir().expect("tempdir");
        let pack = crate::build_pack_dir(&bundled, dir.path()).expect("pack");
        assert!(pack.native_path.is_file(), "{:?}", pack.native_path);
    }

    #[test]
    fn cdylib_transpile_bootstrap_timing_env_get_parse_int_native() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("repo root");
        let mut source =
            std::fs::read_to_string(root.join("scripts/rh/lib/bootstrap_timing.rh")).expect("read");
        source.push_str("\nfn entry() { read_setup_ms() }\n");
        let output = transpile_cdylib_with_project(&root, &source).expect("transpile");
        for name in [
            "AGENTERM_BOOTSTRAP_SETUP_MS",
            "AGENTERM_BOOTSTRAP_CARGO_BUILD_MS",
            "AGENTERM_BOOTSTRAP_WORKER_COPY_MS",
            "AGENTERM_BOOTSTRAP_OTHER_SETUP_MS",
            "AGENTERM_BOOTSTRAP_CLOCK_RESOLUTION_MS",
        ] {
            assert!(
                output.rust.contains(&format!("rh_env_parse_int({name:?})")),
                "{}",
                output.rust
            );
        }
        assert!(!output.rust.contains(".parse_int()"), "{}", output.rust);
        assert!(
            output
                .rust
                .lines()
                .all(|line| !(line.contains("rh_host_eval_int") && line.contains("parse_int"))),
            "{}",
            output.rust
        );
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
        assert_eq!(output.rust.matches("rh_host_eval_int(\"").count(), 0);
    }

    #[test]
    fn cdylib_transpile_emits_direntry_metadata_len_and_modified_native() {
        let source = include_str!("../../../fixtures/rh/direntry-metadata-probe.rh");
        let output = transpile_cdylib_with_mode(source).expect("transpile");
        assert_eq!(
            output.execution_mode,
            CdylibExecutionMode::Native,
            "{}",
            output.rust
        );
        assert!(
            output.rust.contains("rh_metadata(&entry.path)"),
            "{}",
            output.rust
        );
        assert!(output.rust.contains("metadata.len"), "{}", output.rust);
        assert!(
            output.rust.contains("modified.unix_millis"),
            "{}",
            output.rust
        );
        assert!(
            output.rust.contains("rh_system_time_rfc3339(&modified)"),
            "{}",
            output.rust
        );
        assert!(
            !output.rust.contains("rh_host_eval_int(\"entry.metadata"),
            "{}",
            output.rust
        );
        assert!(!output.rust.contains("rh_host_run_script(RH_SCRIPT_SOURCE)"));
        assert_eq!(output.rust.matches("rh_host_eval_int(\"").count(), 0);
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
    fn create_dir_all_parent_display_native() {
        let source = r#"
fn entry() {
    let output_path = "target/tmp/out.json";
    std::fs::create_dir_all(std::path::parent(output_path).display);
    0
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
            output.rust.contains(
                "rh_create_dir_all(&rh_path_parent(&String::from(\"target/tmp/out.json\")))"
            ) || output.rust.contains("rh_create_dir_all(&rh_path_parent("),
            "{}",
            output.rust
        );
        assert_eq!(output.rust.matches("rh_host_eval_int(\"").count(), 0);
        assert!(
            !output
                .rust
                .contains("rh_host_eval_int(\"std::fs::create_dir_all"),
            "{}",
            output.rust
        );
    }

    #[test]
    fn timing_finish_parent_atomic_native() {
        let source = r#"
fn finish(timing) {
    let output_path = timing.output_path;
    std::fs::create_dir_all(std::path::parent(output_path).display);
    rh::runtime::atomic_write(output_path, "{}\n");
    0
}
fn entry() {
    let timing = #{ output_path: "target/tmp/timing.json" };
    finish(timing)
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
            output.rust.contains("rh_create_dir_all(&rh_path_parent(")
                && output.rust.contains("rh_atomic_write("),
            "{}",
            output.rust
        );
        assert_eq!(output.rust.matches("rh_host_eval_int(\"").count(), 0);
    }

    #[test]
    fn write_receipt_parent_atomic_native() {
        let source = r#"
fn write_receipt(report, output_path) {
    std::fs::create_dir_all(std::path::parent(output_path).display);
    rh::runtime::atomic_write(
        output_path,
        rh::json::stringify_pretty(report) + "\n"
    );
    0
}
fn entry() {
    write_receipt(#{ ok: 1 }, "target/tmp/receipt.json")
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
            output.rust.contains("rh_create_dir_all(&rh_path_parent(")
                && output.rust.contains("rh_atomic_write("),
            "{}",
            output.rust
        );
        assert_eq!(output.rust.matches("rh_host_eval_int(\"").count(), 0);
    }

    #[test]
    fn json_remove_and_array_insert_statements_stay_native() {
        let source = r#"
fn entry() {
    let report = #{ output_path: "out.json", started_at_unix_ms: 1 };
    report.remove("output_path");
    report.remove("started_at_unix_ms");
    let result = [#{ id: "b" }];
    let value = #{ id: "a" };
    result.insert(0, value);
    require(result.len == 2, "insert");
    0
}
"#;
        let output = transpile_cdylib_with_mode(source).expect("transpile");
        assert_eq!(
            output.execution_mode,
            CdylibExecutionMode::Native,
            "{}",
            output.rust
        );
        assert_eq!(output.rust.matches("rh_json_remove(&mut report").count(), 2);
        assert!(
            output
                .rust
                .contains("rh_json_array_insert(&mut result, 0, value.clone())"),
            "{}",
            output.rust
        );
        assert_eq!(output.rust.matches("rh_host_eval_int(\"").count(), 0);
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
        assert_eq!(output.rust.matches("rh_host_eval_int(\"").count(), 0);
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
        assert_eq!(output.rust.matches("rh_host_eval_int(\"").count(), 0);
    }

    #[test]
    fn cdylib_transpile_emits_json_array_push_in_loop_native() {
        let output = transpile_cdylib_with_mode(
            r#"fn entry() {
    let doc = rh::json::parse(args[0]);
    let names = [];
    for item in doc.items {
        names.push(item.name);
    }
    let assets = [];
    assets.push(#{ name: "tail", size: names.len });
    names.len + assets.len
}"#,
        )
        .expect("transpile");
        assert_eq!(
            output.execution_mode,
            CdylibExecutionMode::Native,
            "{}",
            output.rust
        );
        assert_eq!(output.rust.matches("rh_json_array_push(&mut").count(), 2);
        assert_eq!(output.rust.matches("rh_host_eval_int(\"").count(), 0);
    }

    #[test]
    fn cdylib_transpile_emits_json_property_assign_native() {
        let output = transpile_cdylib_with_mode(
            r#"fn entry() {
    let metadata = rh::json::parse("{}");
    metadata.git_commit = "abc";
    metadata.git_dirty = true;
    metadata.executables = [];
    0
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
            output
                .rust
                .contains("rh_json_set_path(&mut metadata, &[\"git_commit\"]"),
            "{}",
            output.rust
        );
        assert!(
            output
                .rust
                .contains("rh_json_set_path(&mut metadata, &[\"git_dirty\"]"),
            "{}",
            output.rust
        );
        assert!(
            output
                .rust
                .contains("rh_json_set_path(&mut metadata, &[\"executables\"]"),
            "{}",
            output.rust
        );
        assert_eq!(output.rust.matches("rh_host_eval_int(\"").count(), 0);
    }

    #[test]
    fn cdylib_transpile_emits_json_path_key_and_index_field_assign_native() {
        let output = transpile_cdylib_with_mode(
            r#"fn entry() {
    let context = rh::json::parse("{\"results\":{}}");
    let timing = rh::json::parse("{\"gates\":[{\"id\":\"a\",\"status\":\"not_run\",\"duration_ms\":0}]}");
    let gate_key = "a";
    let index = 0;
    context.results[gate_key] = #{ id: gate_key, status: "passed" };
    timing.gates[index].status = "passed";
    timing.gates[index].duration_ms = 0;
    0
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
            output
                .rust
                .contains("rh_json_set_path_key(&mut context, &[\"results\"]"),
            "{}",
            output.rust
        );
        assert!(
            output.rust.contains(
                "rh_json_set_path_index_field(&mut timing, &[\"gates\"], index, \"status\""
            ),
            "{}",
            output.rust
        );
        assert!(
            output.rust.contains(
                "rh_json_set_path_index_field(&mut timing, &[\"gates\"], index, \"duration_ms\""
            ),
            "{}",
            output.rust
        );
        assert_eq!(output.rust.matches("rh_host_eval_int(\"").count(), 0);
    }

    #[test]
    fn qualification_selftest_project_transpiles_native() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("repo root");
        let source = std::fs::read_to_string(root.join("scripts/rh/qualification-selftest.rh"))
            .expect("entry");
        let bundled = crate::bundle_project_source(&root, &source).expect("bundle");
        let output = transpile_cdylib_with_project(&root, &source).expect("transpile");
        assert_eq!(
            output.execution_mode,
            CdylibExecutionMode::Native,
            "{}",
            output.rust
        );
        assert!(
            output.rust.contains("rh_json_set_path(&mut metadata"),
            "{}",
            output.rust
        );
        let dir = tempfile::tempdir().expect("tempdir");
        let pack = crate::build_pack_dir(&bundled, dir.path()).expect("pack");
        assert!(pack.native_path.is_file(), "{:?}", pack.native_path);
        assert_eq!(output.rust.matches("rh_host_eval_int(\"").count(), 0);
    }

    #[test]
    fn cdylib_transpile_emits_json_field_and_index_assign_native() {
        let output = transpile_cdylib_with_mode(
            r#"fn entry() {
    let metadata = rh::json::parse("{\"git_commit\":\"\",\"git_dirty\":false}");
    metadata.git_commit = args[0];
    metadata.git_dirty = true;
    let timing = rh::json::parse("{\"gates\":[{\"status\":\"not_run\",\"duration_ms\":0}]}");
    let index = 0;
    timing.gates[index].status = "passed";
    timing.gates[index].duration_ms = 12;
    0
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
            output.rust.contains("rh_json_set_path(&mut metadata"),
            "{}",
            output.rust
        );
        assert!(
            output
                .rust
                .contains("rh_json_set_path_index_field(&mut timing"),
            "{}",
            output.rust
        );
        assert_eq!(output.rust.matches("rh_host_eval_int(\"").count(), 0);
    }

    #[test]
    fn cdylib_transpile_emits_hash_fnv1a64_native() {
        let output = transpile_cdylib_with_mode(
            r#"fn entry() {
    let doc = rh::json::parse("{\"a\":1}");
    let fingerprint = rh::hash::fnv1a64(rh::bytes::from_text(rh::json::stringify(doc)));
    fingerprint.len
}"#,
        )
        .expect("transpile");
        assert_eq!(
            output.execution_mode,
            CdylibExecutionMode::Native,
            "{}",
            output.rust
        );
        assert!(output.rust.contains("rh_hash_fnv1a64("), "{}", output.rust);
        assert!(!output.rust.contains("rh_host_eval_int(\"rh::hash::fnv1a64"));
        assert_eq!(output.rust.matches("rh_host_eval_int(\"").count(), 0);
    }

    #[test]
    fn cdylib_transpile_emits_json_path_plus_assign_native() {
        let output = transpile_cdylib_with_mode(
            r#"fn entry() {
    let context = rh::json::parse("{\"process_observation\":{\"owned_commands\":0,\"process_samples\":0}}");
    context.process_observation.owned_commands += 1;
    context.process_observation.process_samples += 2;
    context.process_observation.owned_commands
}"#,
        )
        .expect("transpile");
        assert_eq!(
            output.execution_mode,
            CdylibExecutionMode::Native,
            "{}",
            output.rust
        );
        assert_eq!(
            output.rust.matches("rh_json_set_path(&mut context").count(),
            2
        );
        assert!(
            output.rust.contains(
                "rh_json_int_path(&context, &[\"process_observation\", \"owned_commands\"]) + "
            ),
            "{}",
            output.rust
        );
        assert_eq!(output.rust.matches("rh_host_eval_int(\"").count(), 0);
    }

    #[test]
    fn cdylib_transpile_local_fn_json_return_as_json_arg_native() {
        let output = transpile_cdylib_with_mode(
            r#"fn evidence_lines(evidence) {
    let output = [];
    for id in evidence {
        output.push("EVIDENCE " + "" + id);
    }
    output
}
fn add_result(context, lines) {
    context.lines = lines;
    context
}
fn entry() {
    let context = rh::json::parse("{\"lines\":[]}");
    let gate = rh::json::parse("{\"evidence\":[\"a\"]}");
    context = add_result(context, evidence_lines(gate.evidence));
    context.lines.len
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
            output.rust.contains("evidence_lines(") || output.rust.contains("fn evidence_lines"),
            "{}",
            output.rust
        );
        assert_eq!(output.rust.matches("rh_host_eval_int(\"").count(), 0);
    }

    #[test]
    fn cdylib_transpile_json_path_array_push_contains_native() {
        let output = transpile_cdylib_with_mode(
            r#"fn entry() {
    let context = rh::json::parse("{\"process_observation\":{\"automation_processes\":[]}}");
    let process_text = "powershell";
    if context.process_observation.automation_processes.contains(process_text) == 0 {
        context.process_observation.automation_processes.push(process_text);
    }
    context.process_observation.automation_processes.len
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
            output.rust.contains("rh_json_array_push_path(&mut context"),
            "{}",
            output.rust
        );
        assert!(
            output.rust.contains("rh_json_contains_path(&context"),
            "{}",
            output.rust
        );
        assert_eq!(output.rust.matches("rh_host_eval_int(\"").count(), 0);
    }

    #[test]
    fn cdylib_transpile_for_loop_json_rebind_string_field_native() {
        // Outer JSON null + assign from for-item must keep Json typing for
        // post-loop `"" + metadata_entry.sha256` (qualification artifact hash).
        let output = transpile_cdylib_with_mode(
            r#"fn entry() {
    let metadata = rh::json::parse("{\"executables\":[{\"name\":\"a\",\"sha256\":\"deadbeef\"}]}");
    let metadata_entry = rh::json::parse("null");
    for entry in metadata.executables {
        metadata_entry = entry;
    }
    "" + metadata_entry.sha256
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
            output
                .rust
                .contains("rh_json_string_path(&metadata_entry, &[\"sha256\"])"),
            "{}",
            output.rust
        );
        assert_eq!(output.rust.matches("rh_host_eval_int(\"").count(), 0);
    }

    #[test]
    fn cdylib_transpile_emits_json_array_literal_from_json_locals_native() {
        // Mixed JSON path + int keeps Json array emit. An all-string-path
        // literal may infer StringList (harness pending-directory lists).
        let output = transpile_cdylib_with_mode(
            r#"fn entry() {
    let doc = rh::json::parse(args[0]);
    let pair = [doc.a, 42];
    pair.len
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
            output.rust.contains("serde_json::Value::Array(vec!["),
            "{}",
            output.rust
        );
        assert!(
            output.rust.contains("rh_json_get_path(&doc, &[\"a\"])"),
            "{}",
            output.rust
        );
        assert_eq!(output.rust.matches("rh_host_eval_int(\"").count(), 0);
    }

    #[test]
    fn cdylib_transpile_emits_string_list_index_misparse_len_native() {
        let output = transpile_cdylib_with_mode(
            r#"fn entry() {
    let parts = "a.b".split(".");
    parts[0].len
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
            output
                .rust
                .contains("rh_string_list_get(&parts, 0).chars().count() as INT)"),
            "{}",
            output.rust
        );
        assert_eq!(output.rust.matches("rh_host_eval_int(\"").count(), 0);
    }

    #[test]
    fn cdylib_transpile_emits_set_key_from_json_path_native() {
        let output = transpile_cdylib_with_mode(
            r#"fn entry() {
    let doc = rh::json::parse(args[0]);
    let seen = #{};
    seen[doc.id] = true;
    seen.contains(doc.id)
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
            output.rust.contains("rh_json_string_path(&doc, &[\"id\"])"),
            "{}",
            output.rust
        );
        assert_eq!(output.rust.matches("rh_host_eval_int(\"").count(), 0);
    }

    #[test]
    fn cdylib_transpile_emits_process_id_native() {
        let output = transpile_cdylib_with_mode(
            r#"fn entry() { let n = 0 + std::process::id(); print("" + n); n }"#,
        )
        .expect("transpile");
        assert_eq!(
            output.execution_mode,
            CdylibExecutionMode::Native,
            "{}",
            output.rust
        );
        assert!(output.rust.contains("rh_process_id()"), "{}", output.rust);
        assert!(
            !output.rust.contains("rh_host_eval_int(\"std::process::id"),
            "{}",
            output.rust
        );
        assert_eq!(output.rust.matches("rh_host_eval_int(\"").count(), 0);
    }

    #[test]
    fn cdylib_transpile_emits_path_parent_display_native() {
        let source = include_str!("../../../fixtures/rh/path-parent-probe.rh");
        let output = transpile_cdylib_with_mode(source).expect("transpile");
        assert_eq!(
            output.execution_mode,
            CdylibExecutionMode::Native,
            "{}",
            output.rust
        );
        assert!(output.rust.contains("rh_path_parent("), "{}", output.rust);
        assert!(
            !output.rust.contains("rh_host_eval_int(\"std::path::parent"),
            "{}",
            output.rust
        );
        assert_eq!(output.rust.matches("rh_host_eval_int(\"").count(), 0);
    }

    #[test]
    fn cdylib_transpile_emits_path_parent_display_to_lower_native() {
        let output = transpile_cdylib_with_mode(
            r#"fn entry() {
    let path = args[0];
    let root = args[1];
    if std::path::parent(path).display.to_lower() == root.to_lower() {
        1
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
        assert!(output.rust.contains("rh_path_parent("), "{}", output.rust);
        assert!(
            output.rust.contains(".to_ascii_lowercase()"),
            "{}",
            output.rust
        );
        assert!(
            !output.rust.contains("rh_host_eval_int(\"std::path::parent"),
            "{}",
            output.rust
        );
        assert_eq!(output.rust.matches("rh_host_eval_int(\"").count(), 0);
    }

    #[test]
    fn cdylib_transpile_emits_nested_parent_absolute_to_lower_native() {
        let output = transpile_cdylib_with_mode(
            r#"fn entry() {
    let diagnostic = args[0];
    let failure_directory = args[1];
    if std::path::parent(
        std::path::absolute(diagnostic).display
    ).display.to_lower() !=
        std::path::absolute(
            failure_directory
        ).display.to_lower() {
        1
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
        assert!(output.rust.contains("rh_path_parent("), "{}", output.rust);
        assert!(output.rust.contains("rh_path_absolute("), "{}", output.rust);
        assert!(
            output.rust.contains(".to_ascii_lowercase()"),
            "{}",
            output.rust
        );
        assert_eq!(output.rust.matches("rh_host_eval_int(\"").count(), 0);
    }

    #[test]
    fn cdylib_transpile_emits_json_split_len_and_for_local_json_array() {
        let output = transpile_cdylib_with_mode(
            r#"
fn retained() {
    let paths = [];
    paths.push("/a/b-c");
    paths
}
fn entry() {
    let manifest = #{ failure: "aaXbbXcc" };
    let marker = "X";
    let count = manifest.failure.split(marker).len;
    let n = 0;
    for path in retained() {
        let name = std::path::PathBuf::from(path).file_name;
        if name.starts_with("b-") {
            n += 1;
        }
    }
    count + n
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
            output.rust.contains("rh_json_array_items("),
            "{}",
            output.rust
        );
        assert!(
            output.rust.contains("rh_path_file_name("),
            "{}",
            output.rust
        );
        assert_eq!(output.rust.matches("rh_host_eval_int(\"").count(), 0);
    }

    #[test]
    fn cdylib_transpile_emits_json_stringify_native() {
        let output = transpile_cdylib_with_mode(
            r#"fn entry() { let doc = #{ answer: 42 }; let text = rh::json::stringify(doc); text.len }"#,
        )
        .expect("transpile");
        assert_eq!(
            output.execution_mode,
            CdylibExecutionMode::Native,
            "{}",
            output.rust
        );
        assert!(
            output.rust.contains("rh_json_stringify(&doc)"),
            "{}",
            output.rust
        );
        assert!(
            !output
                .rust
                .contains("rh_host_eval_int(\"rh::json::stringify"),
            "{}",
            output.rust
        );
        assert_eq!(output.rust.matches("rh_host_eval_int(\"").count(), 0);
    }

    #[test]
    fn cdylib_transpile_emits_append_sync_native() {
        let output = transpile_cdylib_with_mode(
            r#"fn entry() { let path = args[0]; rh::runtime::append_sync(path, "tail") }"#,
        )
        .expect("transpile");
        assert_eq!(
            output.execution_mode,
            CdylibExecutionMode::Native,
            "{}",
            output.rust
        );
        assert!(
            output.rust.contains("rh_append_sync(&path, &"),
            "{}",
            output.rust
        );
        assert!(
            !output
                .rust
                .contains("rh_host_eval_int(\"rh::runtime::append_sync"),
            "{}",
            output.rust
        );
        assert_eq!(output.rust.matches("rh_host_eval_int(\"").count(), 0);
    }

    #[test]
    fn cdylib_transpile_emits_sub_string_native() {
        let output = transpile_cdylib_with_mode(
            r#"fn entry() { let text = "abcdef"; let tail = text.sub_string(1, 3); tail.len }"#,
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
                .contains("rh_string_sub_string(&text, 1, Some(3))"),
            "{}",
            output.rust
        );
        assert_eq!(output.rust.matches("rh_host_eval_int(\"").count(), 0);
    }

    #[test]
    fn cdylib_transpile_emits_stringify_append_sync_sub_string_bundle() {
        let source = include_str!("../../../fixtures/rh/append-sync-probe.rh");
        let output = transpile_cdylib_with_mode(source).expect("transpile");
        assert_eq!(
            output.execution_mode,
            CdylibExecutionMode::Native,
            "{}",
            output.rust
        );
        assert!(
            output.rust.contains("rh_json_stringify("),
            "{}",
            output.rust
        );
        assert!(output.rust.contains("rh_append_sync("), "{}", output.rust);
        assert!(
            output.rust.contains("rh_string_sub_string("),
            "{}",
            output.rust
        );
        assert_eq!(output.rust.matches("rh_host_eval_int(\"").count(), 0);
    }

    #[test]
    fn cdylib_transpile_emits_std_fs_remove_dir_all_native() {
        let source = include_str!("../../../fixtures/rh/remove-dir-all-probe.rh");
        let output = transpile_cdylib_with_mode(source).expect("transpile");
        assert_eq!(
            output.execution_mode,
            CdylibExecutionMode::Native,
            "{}",
            output.rust
        );
        assert!(
            output.rust.contains("rh_remove_dir_all(&path)"),
            "{}",
            output.rust
        );
        assert!(
            !output
                .rust
                .contains("rh_host_eval_int(\"std::fs::remove_dir_all"),
            "{}",
            output.rust
        );
        assert_eq!(output.rust.matches("rh_host_eval_int(\"").count(), 0);
    }

    #[test]
    fn new_context_with_args_index_stays_json_binding() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let source = r#"
import "scripts/rh/lib/test_harness" as test_harness;
fn entry() {
    let context = test_harness::new_context(args[0], "args-index-json");
    let command = std::process::command("/bin/echo");
    command.args(["x"]);
    let output = command.output();
    test_harness::append_command_record(context, ["x"], output, 0, []);
    0
}
"#;
        let output = transpile_cdylib_with_project(&root, source).expect("transpile");
        assert_eq!(
            output.execution_mode,
            CdylibExecutionMode::Native,
            "{}",
            output.rust
        );
        assert!(
            output
                .rust
                .contains("let mut context = test_harness__new_context(rh_arg(0)"),
            "{}",
            output.rust
        );
        assert_eq!(output.rust.matches("rh_host_eval_int(\"").count(), 0);
    }

    #[test]
    fn require_in_json_returning_fn_returns_null_after_fail() {
        let source = r#"
fn facts(profile) {
    require(profile == "dev", "bad_profile");
    #{ ok: 1 }
}
fn entry() {
    let doc = facts("nope");
    0
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
            output.rust.contains("let _ = rh_fail(")
                && output.rust.contains("return serde_json::Value::Null;"),
            "{}",
            output.rust
        );
    }

    #[test]
    fn throw_string_binding_emits_rh_fail_return() {
        let source = r#"
fn entry() {
    let failure = "boom";
    if 1 == 0 {
        throw failure;
    }
    0
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
            output.rust.contains("return rh_fail(&failure);"),
            "{}",
            output.rust
        );
        assert!(!output.rust.contains("return failure;"), "{}", output.rust);
    }

    #[test]
    fn try_catch_binds_error_and_returns_int_arm() {
        let source = r#"
fn entry() {
    let failure = "";
    try {
        throw "boom";
    } catch (error) {
        failure = "" + error;
    }
    if failure == "" {
        return 0;
    }
    0
}
"#;
        let output = transpile_cdylib_with_mode(source).expect("transpile");
        assert_eq!(
            output.execution_mode,
            CdylibExecutionMode::Native,
            "{}",
            output.rust
        );
        let entry = output
            .rust
            .split("pub fn entry()")
            .nth(1)
            .expect("entry fn");
        assert!(
            entry.contains("Err(error) =>") && entry.contains("failure ="),
            "{entry}"
        );
        assert!(
            !entry.contains("Err(_) =>"),
            "catch arm must bind the error name: {entry}"
        );
    }

    #[test]
    fn throw_from_output_helper_keeps_output_return_kind() {
        let source = r#"
fn wait_or_fail(child, code) {
    if child.state == "exited" {
        return child.wait_with_output(std::time::Duration::from_secs(1));
    }
    throw code;
    child.wait_with_output(std::time::Duration::from_secs(0))
}
fn entry() {
    0
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
            output.rust.contains("let _ = rh_fail(&code);")
                && output.rust.contains("return RhOutput {"),
            "{}",
            output.rust
        );
        assert!(
            !output.rust.contains("return rh_fail(&code);"),
            "{}",
            output.rust
        );
    }

    #[test]
    fn json_path_contains_array_and_string_stay_native() {
        let source = r#"
fn entry() {
    let proof = #{
        cleanup: #{ forced_pids: [1, 2] },
        manifest: #{ failure: "original-marker-text" }
    };
    let owned_pid = 1;
    let marker = "marker";
    require(proof.cleanup.forced_pids.contains(owned_pid), "array");
    require(proof.manifest.failure.contains(marker), "string");
    0
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
                .contains("rh_json_contains_path(&proof, &[\"cleanup\", \"forced_pids\"]"),
            "{}",
            output.rust
        );
        assert!(
            output
                .rust
                .contains("rh_json_contains_path(&proof, &[\"manifest\", \"failure\"]"),
            "{}",
            output.rust
        );
        assert!(!output.rust.contains("rh_host_eval_int(\""));
        assert_eq!(output.rust.matches("rh_host_eval_int(\"").count(), 0);
    }

    #[test]
    fn output_fn_arg_accepts_rh_output_in_local_call() {
        let source = include_str!("../../../fixtures/rh/output-fn-arg-probe.rh");
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let output = transpile_cdylib_with_project(&root, source).expect("transpile");
        assert!(
            output
                .rust
                .contains("pub fn test_harness__append_command_record("),
            "{}",
            output.rust
        );
        assert!(output.rust.contains("output: RhOutput"), "{}", output.rust);
        assert!(
            output.rust.contains("rh_command_output(&mut command)"),
            "{}",
            output.rust
        );
        assert!(
            output.rust.contains("test_harness__append_command_record(")
                && output.rust.contains("String::from(\"probe\")")
                && (output.rust.contains(", output, 0")
                    || output.rust.contains(", output.clone(), 0")),
            "{}",
            output.rust
        );
        assert!(
            output.rust.contains("rh_output_stdout_text(&output)"),
            "{}",
            output.rust
        );
        assert!(
            matches!(output.execution_mode, CdylibExecutionMode::Native),
            "{:?}\n{}",
            output.execution_mode,
            output.rust
        );
        assert_eq!(output.rust.matches("rh_host_eval_int(\"").count(), 0);
    }

    #[test]
    fn child_platform_facts_use_structured_native_host_call() {
        let source = r#"
fn entry() {
    let command = std::process::command("agenterm");
    let child = command.start();
    let facts = child.platform_facts;
    require(facts.top_level_window_supported >= 0, "facts");
    child.kill();
    0
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
                .contains("let mut facts = rh_child_platform_facts(&mut child);"),
            "{}",
            output.rust
        );
        assert_eq!(output.rust.matches("rh_host_eval_int(\"").count(), 0);
    }

    #[test]
    fn gui_window_control_visible_click_stays_native() {
        let source = include_str!("../tests/fixtures/rh_gui_window_control_visible_click.rh");
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
                .contains("rh_window_control_visible(&mut tabs_button)"),
            "{}",
            output.rust
        );
        assert!(
            output
                .rust
                .contains("rh_window_control_click(&mut tabs_button)"),
            "{}",
            output.rust
        );
        assert!(
            output.rust.contains("rh_child_window_message(&mut gui, "),
            "{}",
            output.rust
        );
        assert_eq!(output.rust.matches("rh_host_eval_int(\"").count(), 0);
    }

    #[test]
    fn child_window_key_uses_structured_native_host_call() {
        let source = r#"
fn entry() {
    let command = std::process::command("agenterm");
    let child = command.start();
    child.window_key("Escape");
    child.kill();
    0
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
                .contains("rh_child_window_key(&mut child, &String::from(\"Escape\"))"),
            "{}",
            output.rust
        );
        assert_eq!(output.rust.matches("rh_host_eval_int(\"").count(), 0);
    }

    #[test]
    fn child_stream_read_and_bytes_text_stay_native() {
        let source = r#"
fn read_once(stream) {
    let chunk = stream.read(65536, std::time::Duration::from_millis(100));
    if chunk.len > 0 {
        return chunk.to_text();
    }
    ""
}

fn entry() {
    let command = std::process::command("agenterm");
    let child = command.start();
    let text = read_once(child.stderr);
    child.kill();
    text.len
}
"#;
        let output = transpile_cdylib_with_mode(source).expect("transpile");
        assert_eq!(
            output.execution_mode,
            CdylibExecutionMode::Native,
            "{}",
            output.rust
        );
        assert!(output.rust.contains("stream: RhStream"), "{}", output.rust);
        assert!(
            output
                .rust
                .contains("rh_stream_read(&mut stream, 65536, 100)"),
            "{}",
            output.rust
        );
        assert!(
            output.rust.contains("rh_bytes_to_text(&chunk)"),
            "{}",
            output.rust
        );
        assert_eq!(output.rust.matches("rh_host_eval_int(\"").count(), 0);
    }

    #[test]
    fn local_child_argument_with_nested_platform_facts_stays_native() {
        let source = r#"
fn wait_for_window(child) {
    let facts = child.platform_facts;
    if facts.top_level_window_present != 0 {
        return facts;
    }
    child.platform_facts
}

fn entry() {
    let command = std::process::command("agenterm");
    let child = command.start();
    let facts = wait_for_window(child);
    child.kill();
    facts.top_level_window_present
}
"#;
        let output = transpile_cdylib_with_mode(source).expect("transpile");
        assert_eq!(
            output.execution_mode,
            CdylibExecutionMode::Native,
            "{}",
            output.rust
        );
        assert!(output.rust.contains("child: RhChild"), "{}", output.rust);
        assert!(
            output.rust.contains("wait_for_window(child.clone())"),
            "{}",
            output.rust
        );
        assert_eq!(output.rust.matches("rh_host_eval_int(\"").count(), 0);
    }

    #[test]
    fn child_return_before_terminal_throw_keeps_child_kind() {
        let source = r#"
fn start_ready() {
    let command = std::process::command("agenterm");
    let child = command.start();
    for attempt in 0..2 {
        if child.state == "running" {
            return child;
        }
    }
    throw "not_ready";
}

fn entry() {
    let children = [];
    let child = start_ready();
    children.push(child);
    child.kill();
    0
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
            output.rust.contains("pub fn start_ready() -> RhChild"),
            "{}",
            output.rust
        );
        assert_eq!(output.rust.matches("rh_host_eval_int(\"").count(), 0);
    }

    #[test]
    fn local_output_text_helper_keeps_output_kind() {
        let source = r#"
fn process_output_text(output) {
    output.stdout_text() + output.stderr_text()
}

fn entry() {
    let command = std::process::command("tool");
    let output = command.output();
    print(process_output_text(output));
    0
}
"#;
        let output = transpile_cdylib_with_mode(source).expect("transpile");
        assert_eq!(
            output.execution_mode,
            CdylibExecutionMode::Native,
            "{}",
            output.rust
        );
        assert!(output.rust.contains("output: RhOutput"), "{}", output.rust);
        assert_eq!(output.rust.matches("rh_host_eval_int(\"").count(), 0);
    }

    #[test]
    fn local_command_return_can_chain_output_native() {
        let source = r#"
fn configured() {
    std::process::command("tool")
}

fn entry() {
    let output = configured().output();
    print(output.stdout_text() + output.stderr_text());
    0
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
            output.rust.contains(
                "let mut output = { let mut command = configured(); rh_command_output(&mut command) };"
            ),
            "{}",
            output.rust
        );
        assert_eq!(output.rust.matches("rh_host_eval_int(\"").count(), 0);
    }

    #[test]
    fn command_output_file_redirection_stays_native() {
        let source = r#"
fn entry() {
    let command = std::process::command("tool");
    command.stdout_file("target/out.log");
    command.stderr_file("target/err.log");
    let output = command.output();
    output.exit_code
}
"#;
        let output = transpile_cdylib_with_mode(source).expect("transpile");
        assert_eq!(output.execution_mode, CdylibExecutionMode::Native);
        assert!(
            output.rust.contains("rh_command_stdout_file(&mut command"),
            "{}",
            output.rust
        );
        assert!(
            output.rust.contains("rh_command_stderr_file(&mut command"),
            "{}",
            output.rust
        );
        assert_eq!(output.rust.matches("rh_host_eval_int(\"").count(), 0);
        let dir = tempfile::tempdir().expect("pack dir");
        let pack = crate::build_pack_dir(source, dir.path()).expect("build pack");
        assert!(pack.native_path.exists());
    }

    #[test]
    fn chained_child_platform_facts_string_path_stays_native() {
        let source = r#"
fn title(child) {
    "" + child.platform_facts.top_level_window_title
}

fn entry() {
    let command = std::process::command("agenterm");
    let child = command.start();
    let value = title(child);
    child.kill();
    value.len
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
            output.rust.contains(
                "rh_json_string_path(&rh_child_platform_facts(&mut child), &[\"top_level_window_title\"])"
            ),
            "{}",
            output.rust
        );
        assert_eq!(output.rust.matches("rh_host_eval_int(\"").count(), 0);
    }

    #[test]
    fn process_list_uses_structured_native_host_call() {
        let source = r#"
fn entry() {
    let processes = std::process::list();
    for index in 0..processes.len {
        let process = processes[index];
        if process.id == std::process::id() {
            require(process.executable_name != "", "process_name");
        }
    }
    0
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
            output.rust.contains("rh_host_json_call(\"process.list\""),
            "{}",
            output.rust
        );
        assert_eq!(output.rust.matches("rh_host_eval_int(\"").count(), 0);
    }

    #[test]
    fn image_inspect_png_uses_structured_native_host_call() {
        let source = r#"
fn entry() {
    let path = args[0];
    let image = rh::image::inspect_png(path);
    require(image.width > 0, "width");
    require(image.luminance >= 0, "luminance");
    0
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
                .contains("rh_host_json_call(\"image.inspect_png\""),
            "{}",
            output.rust
        );
        assert_eq!(output.rust.matches("rh_host_eval_int(\"").count(), 0);
    }

    #[test]
    fn clipboard_get_set_text_uses_structured_native_host_call() {
        let source = r#"
fn entry() {
    let before = rh::clipboard::get_text();
    rh::clipboard::set_text("probe");
    let after = rh::clipboard::get_text();
    before.len() + after.len()
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
            output.rust.contains("rh_clipboard_get_text()"),
            "{}",
            output.rust
        );
        assert!(
            output
                .rust
                .contains("rh_clipboard_set_text(&String::from(\"probe\"))"),
            "{}",
            output.rust
        );
        assert!(
            !output.rust.contains("rh_host_eval_int(\"rh::clipboard::"),
            "{}",
            output.rust
        );
        assert_eq!(output.rust.matches("rh_host_eval_int(\"").count(), 0);
    }

    #[test]
    fn cdylib_transpile_emits_command_stdout_file_with_dynamic_args_and_options_var() {
        let output = transpile_cdylib_with_mode(
            r#"fn entry() {
    let repo = args[0];
    let out = args[1];
    let cargo_args = [];
    cargo_args.push("build");
    cargo_args.push("--locked");
    let command_options = #{
        current_dir: repo,
        env: #{ "AGENTERM_NO_ACTIVATE": "1" },
    };
    std::process::command_stdout_file("cargo", cargo_args, 3600000, out, command_options)
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
            output.rust.contains("rh_json_string_argv(&cargo_args)"),
            "{}",
            output.rust
        );
        assert!(
            output.rust.contains("Some(&command_options)"),
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
    fn cdylib_transpile_emits_command_status_with_string_list_args_and_options_var() {
        let output = transpile_cdylib_with_mode(
            r#"fn entry() {
    let repo = args[0];
    let worker = args[1];
    let task_manifest = args[2];
    let prune_arguments = [
        "task", "run", "prune-target-incremental",
        "--manifest", task_manifest,
        "--", repo
    ];
    let command_options = #{ current_dir: repo };
    std::process::command_status(worker, prune_arguments, 300000, command_options)
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
            output.rust.contains("rh_process_status("),
            "{}",
            output.rust
        );
        assert!(output.rust.contains("&prune_arguments"), "{}", output.rust);
        assert!(
            output.rust.contains("Some(&command_options)"),
            "{}",
            output.rust
        );
        assert!(
            !output
                .rust
                .contains("rh_host_eval_int(\"std::process::command_status")
        );
    }

    #[test]
    fn local_string_list_literals_clone_reused_bindings() {
        let output = transpile_cdylib_with_mode(
            r#"
fn consume(values) {
    let command = std::process::command("tool");
    command.args(values);
    0
}

fn entry() {
    let marker = "shared";
    consume(["--marker", marker]);
    consume(["--marker", marker]);
    0
}
"#,
        )
        .expect("transpile");
        assert_eq!(output.execution_mode, CdylibExecutionMode::Native);
        assert_eq!(
            output
                .rust
                .matches("vec![String::from(\"--marker\"), marker.clone()]")
                .count(),
            2,
            "{}",
            output.rust
        );
        assert_eq!(output.rust.matches("rh_host_eval_int(\"").count(), 0);
    }

    #[test]
    fn json_path_coerces_at_local_string_list_boundary() {
        let output = transpile_cdylib_with_mode(
            r#"
fn consume(values) {
    let command = std::process::command("tool");
    command.args(values);
    0
}

fn entry() {
    let document = rh::json::parse("{\"tabs\":[\"one\",\"two\"]}");
    consume(document.tabs);
    0
}
"#,
        )
        .expect("transpile");
        assert_eq!(output.execution_mode, CdylibExecutionMode::Native);
        assert!(
            output.rust.contains(
                "rh_json_array_items(&document, &[\"tabs\"]).into_iter().map(|value| rh_json_as_str(&value)).collect()"
            ),
            "{}",
            output.rust
        );
        assert_eq!(output.rust.matches("rh_host_eval_int(\"").count(), 0);
    }

    #[test]
    fn string_length_does_not_infer_string_list_parameter() {
        let output = transpile_cdylib_with_mode(
            r#"
fn is_commit(value) {
    let n = value.len;
    if n == 40 {
        return value.to_lower() == value;
    }
    0
}

fn entry() {
    if is_commit(args[0]) {
        return 0;
    }
    rh::fail("invalid_commit")
}
"#,
        )
        .expect("transpile");
        assert_eq!(output.execution_mode, CdylibExecutionMode::Native);
        assert!(
            output.rust.contains("pub fn is_commit(value: String)"),
            "{}",
            output.rust
        );
        assert_eq!(output.rust.matches("rh_host_eval_int(\"").count(), 0);
    }

    #[test]
    fn indexed_string_list_param_with_len_stays_string_list() {
        let output = transpile_cdylib_with_mode(
            r#"
fn new_timing(gate_ids) {
    require(gate_ids.len > 0, "empty");
    let gate_index = 0;
    while gate_index < gate_ids.len {
        let gate_id = "" + gate_ids[gate_index];
        print(gate_id);
        gate_index = gate_index + 1;
    }
    0
}

fn entry() {
    new_timing(["repo-lint", "rustfmt"]);
    0
}
"#,
        )
        .expect("transpile");
        assert_eq!(output.execution_mode, CdylibExecutionMode::Native);
        assert!(
            output
                .rust
                .contains("pub fn new_timing(mut gate_ids: Vec<String>)"),
            "{}",
            output.rust
        );
        assert_eq!(output.rust.matches("rh_host_eval_int(\"").count(), 0);
    }

    #[test]
    fn string_list_param_in_json_map_literal_stays_typed() {
        let output = transpile_cdylib_with_mode(
            r#"
fn spec(program, arguments, timeout_ms) {
    let program_text = "" + program;
    let timeout_value = 0 + timeout_ms;
    #{
        program: program_text,
        arguments: arguments,
        timeout_ms: timeout_value
    }
}

fn entry() {
    let argv = ["task", "run"];
    let command = spec("worker", argv, 10);
    if command.program == "worker" {
        return 0;
    }
    rh::fail("bad")
}
"#,
        )
        .expect("transpile");
        assert_eq!(output.execution_mode, CdylibExecutionMode::Native);
        assert!(
            output
                .rust
                .contains("pub fn spec(program: String, arguments: Vec<String>, timeout_ms: INT)")
                || output.rust.contains(
                    "pub fn spec(program: String, mut arguments: Vec<String>, timeout_ms: INT)"
                ),
            "{}",
            output.rust
        );
        assert!(
            output
                .rust
                .contains(".iter().cloned().map(serde_json::Value::String).collect()"),
            "{}",
            output.rust
        );
        assert_eq!(output.rust.matches("rh_host_eval_int(\"").count(), 0);
        let dir = tempfile::tempdir().expect("pack dir");
        let pack = crate::build_pack_dir(
            r#"
fn spec(program, arguments, timeout_ms) {
    let program_text = "" + program;
    let timeout_value = 0 + timeout_ms;
    #{
        program: program_text,
        arguments: arguments,
        timeout_ms: timeout_value
    }
}

fn entry() {
    let argv = ["task", "run"];
    let command = spec("worker", argv, 10);
    if command.program == "worker" {
        return 0;
    }
    rh::fail("bad")
}
"#,
            dir.path(),
        )
        .expect("pack");
        assert!(pack.native_path.exists());
    }

    #[test]
    fn json_param_mutation_emits_mutable_binding() {
        let output = transpile_cdylib_with_mode(
            r#"
fn finish(timing) {
    timing.status = "passed";
    timing
}

fn entry() {
    let timing = #{ status: "running" };
    let done = finish(timing);
    if done.status == "passed" {
        return 0;
    }
    rh::fail("bad")
}
"#,
        )
        .expect("transpile");
        assert_eq!(output.execution_mode, CdylibExecutionMode::Native);
        assert!(
            output
                .rust
                .contains("pub fn finish(mut timing: serde_json::Value)"),
            "{}",
            output.rust
        );
        let dir = tempfile::tempdir().expect("pack dir");
        let pack = crate::build_pack_dir(
            r#"
fn finish(timing) {
    timing.status = "passed";
    timing
}

fn entry() {
    let timing = #{ status: "running" };
    let done = finish(timing);
    if done.status == "passed" {
        return 0;
    }
    rh::fail("bad")
}
"#,
            dir.path(),
        )
        .expect("pack");
        assert!(pack.native_path.exists());
    }

    #[test]
    fn json_array_lookup_by_id_does_not_infer_child_list() {
        let output = transpile_cdylib_with_mode(
            r#"
fn find_by_id(values, target) {
    for value in values {
        if value.id == target {
            return value;
        }
    }
    rh::json::parse("null")
}

fn entry() {
    let document = rh::json::parse("{\"tabs\":[{\"id\":\"@1\"}]}");
    let found = find_by_id(document.tabs, "@1");
    if found.id == "@1" {
        return 0;
    }
    rh::fail("missing")
}
"#,
        )
        .expect("transpile");
        assert_eq!(output.execution_mode, CdylibExecutionMode::Native);
        assert!(
            output
                .rust
                .contains("pub fn find_by_id(mut values: serde_json::Value"),
            "{}",
            output.rust
        );
        assert_eq!(output.rust.matches("rh_host_eval_int(\"").count(), 0);
    }

    #[test]
    fn child_list_index_preserves_child_kind() {
        let output = transpile_cdylib_with_mode(
            r#"
fn entry() {
    let command = std::process::command("tool");
    let first = command.start();
    let children = [first];
    let index = 0;
    let child = children[index];
    if child.state == "running" {
        child.kill();
    }
    0
}
"#,
        )
        .expect("transpile");
        assert_eq!(output.execution_mode, CdylibExecutionMode::Native);
        assert!(
            output
                .rust
                .contains("let mut child = rh_child_share(&mut children[index as usize]);"),
            "{}",
            output.rust
        );
        assert_eq!(output.rust.matches("rh_host_eval_int(\"").count(), 0);
    }

    #[test]
    fn float_literal_string_coercion_stays_native() {
        let output = transpile_cdylib_with_mode(
            r#"
fn entry() {
    print("scale:" + 1.0);
    0
}
"#,
        )
        .expect("transpile");
        assert_eq!(output.execution_mode, CdylibExecutionMode::Native);
        assert!(output.rust.contains("1.0.to_string()"), "{}", output.rust);
        assert_eq!(output.rust.matches("rh_host_eval_int(\"").count(), 0);
    }

    #[test]
    fn direct_metadata_len_string_coercion_stays_native() {
        let output = transpile_cdylib_with_mode(
            r#"
fn entry() {
    let path = args[0];
    print("bytes:" + std::fs::metadata(path).len);
    0
}
"#,
        )
        .expect("transpile");
        assert_eq!(output.execution_mode, CdylibExecutionMode::Native);
        assert!(
            output.rust.contains("rh_metadata(&path).len).to_string()"),
            "{}",
            output.rust
        );
        assert_eq!(output.rust.matches("rh_host_eval_int(\"").count(), 0);
    }

    #[test]
    fn build_rh_project_transpiles_command_options_native() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("repo root");
        let source = std::fs::read_to_string(root.join("scripts/rh/build.rh")).expect("read");
        let output = transpile_cdylib_with_project(&root, &source).expect("transpile");
        let command_host_eval: Vec<_> = output
            .rust
            .lines()
            .filter(|line| {
                line.contains("rh_host_eval_int")
                    && (line.contains("command_stdout_file") || line.contains("command_status"))
            })
            .collect();
        assert!(
            command_host_eval.is_empty(),
            "command HostEval sites:\n{command_host_eval:#?}\n{}",
            output.rust
        );
        assert!(
            output.rust.contains("rh_process_stdout_file("),
            "{}",
            output.rust
        );
        assert!(
            output.rust.contains("rh_process_status("),
            "{}",
            output.rust
        );
        // build.rh passes inline `#{ current_dir, env }` option maps.
        assert!(
            output.rust.contains("Some(&command_options)")
                || (output.rust.contains("Some(&{") && output.rust.contains("\"current_dir\"")),
            "{}",
            output.rust
        );
        assert!(
            output.rust.contains("rh_json_string_argv(&cargo_args)"),
            "{}",
            output.rust
        );
        assert!(
            !output
                .rust
                .contains("rh_host_eval_int(\"std::process::command_stdout_file")
        );
        assert!(
            !output
                .rust
                .contains("rh_host_eval_int(\"std::process::command_status")
        );
    }

    #[test]
    fn process_output_command_builder_transpiles_native() {
        let source = include_str!("../../../fixtures/rh/process-output-probe.rh");
        let output = transpile_cdylib_with_mode(source).expect("transpile");
        assert_eq!(
            output.execution_mode,
            CdylibExecutionMode::Native,
            "{}",
            output.rust
        );
        assert!(output.rust.contains("rh_command_new("), "{}", output.rust);
        assert!(output.rust.contains("rh_command_args("), "{}", output.rust);
        assert!(
            output.rust.contains("rh_command_output("),
            "{}",
            output.rust
        );
        assert!(
            output.rust.contains("rh_output_stdout_text("),
            "{}",
            output.rust
        );
        assert!(
            !output
                .rust
                .contains("rh_host_eval_int(\"std::process::command"),
            "{}",
            output.rust
        );
        assert_eq!(output.rust.matches("rh_host_eval_int(\"").count(), 0);
    }

    #[test]
    fn child_lifecycle_command_start_transpiles_native() {
        let source = include_str!("../../../fixtures/rh/child-lifecycle-probe.rh");
        let output = transpile_cdylib_with_mode(source).expect("transpile");
        assert_eq!(
            output.execution_mode,
            CdylibExecutionMode::Native,
            "{}",
            output.rust
        );
        assert!(output.rust.contains("rh_command_start("), "{}", output.rust);
        assert!(output.rust.contains("rh_child_state("), "{}", output.rust);
        assert!(output.rust.contains("rh_child_kill("), "{}", output.rust);
        assert!(
            output.rust.contains("rh_child_wait_with_output("),
            "{}",
            output.rust
        );
        assert!(
            !output
                .rust
                .contains("rh_host_eval_int(\"std::process::command"),
            "{}",
            output.rust
        );
        assert_eq!(output.rust.matches("rh_host_eval_int(\"").count(), 0);
    }

    #[test]
    fn command_output_and_child_helpers_emit_in_pack_runtime() {
        let output = transpile_cdylib_with_mode(
            r#"fn entry() {
    let command = std::process::command("/bin/true");
    command.timeout(1000);
    command.capture_limit(1024);
    let output = command.output();
    output.require_success("true_failed");
    let sleeper = std::process::command("/bin/sleep");
    let child = sleeper.start();
    child.kill();
    output.exit_code
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
            output.rust.contains("rh_command_timeout_ms("),
            "{}",
            output.rust
        );
        assert!(
            output.rust.contains("rh_output_require_success("),
            "{}",
            output.rust
        );
        assert!(output.rust.contains("struct RhCommand"), "{}", output.rust);
        assert!(output.rust.contains("struct RhOutput"), "{}", output.rust);
        assert!(output.rust.contains("struct RhChild"), "{}", output.rust);
        assert_eq!(output.rust.matches("rh_host_eval_int(\"").count(), 0);
    }

    #[test]
    fn infer_context_param_as_json_from_nested_path_plus_assign() {
        let output = transpile_cdylib_with_mode(
            r#"fn f(context) {
    context.process_observation.owned_commands += 1;
    context
}
fn entry() { 0 }"#,
        )
        .expect("transpile");
        assert!(
            output
                .rust
                .contains("pub fn f(mut context: serde_json::Value) -> serde_json::Value"),
            "{}",
            output.rust
        );
    }

    #[test]
    fn cdylib_transpile_emits_json_path_contains_and_push_native() {
        let output = transpile_cdylib_with_mode(
            r#"fn add_process_observation(context, observed_powershell, terminal_compatibility) {
    context.process_observation.owned_commands += 1;
    for process in observed_powershell {
        let process_text = "" + process;
        if terminal_compatibility != 0 {
            if !context.process_observation
                    .terminal_compatibility_payloads.contains(process_text) {
                context.process_observation
                    .terminal_compatibility_payloads.push(process_text);
            }
        } else if !context.process_observation
                .automation_processes.contains(process_text) {
            context.process_observation.automation_processes.push(process_text);
        }
    }
    context
}
fn entry() { 0 }"#,
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
                .contains("rh_json_contains_path(&context, &[\"process_observation\", \"terminal_compatibility_payloads\"]"),
            "{}",
            output.rust
        );
        assert!(
            output
                .rust
                .contains("rh_json_contains_path(&context, &[\"process_observation\", \"automation_processes\"]"),
            "{}",
            output.rust
        );
        assert_eq!(
            output
                .rust
                .matches("rh_json_array_push_path(&mut context")
                .count(),
            2,
            "{}",
            output.rust
        );
        assert_eq!(output.rust.matches("rh_host_eval_int(\"").count(), 0);
    }

    #[test]
    fn cdylib_transpile_emits_json_path_push_with_path_key_arg_native() {
        let output = transpile_cdylib_with_mode(
            r#"fn entry() {
    let receipt = rh::json::parse("{\"gates\":[]}");
    let context = rh::json::parse("{\"results\":{\"a\":{\"id\":\"a\"}}}");
    let gate_id = "a";
    receipt.gates.push(context.results[gate_id]);
    receipt.gates.len
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
            output
                .rust
                .contains("rh_json_array_push_path(&mut receipt, &[\"gates\"]")
                && output
                    .rust
                    .contains("rh_json_get_path_key(&context, &[\"results\"]")
                && output.rust.contains("gate_id"),
            "{}",
            output.rust
        );
        assert_eq!(output.rust.matches("rh_host_eval_int(\"").count(), 0);
    }

    #[test]
    fn cdylib_transpile_emits_process_observation_int_reads_in_if_native() {
        let output = transpile_cdylib_with_mode(
            r#"fn would_reject(context) {
    if context.process_observation.owned_commands == 0
            || context.process_observation.process_samples == 0 {
        return 1;
    }
    if context.process_observation.automation_processes.len > 0 {
        return 1;
    }
    0
}
fn entry() { 0 }"#,
        )
        .expect("transpile");
        assert_eq!(
            output.execution_mode,
            CdylibExecutionMode::Native,
            "{}",
            output.rust
        );
        assert!(
            output.rust.contains(
                "rh_json_int_path(&context, &[\"process_observation\", \"owned_commands\"])"
            ),
            "{}",
            output.rust
        );
        assert!(
            output.rust.contains(
                "rh_json_int_path(&context, &[\"process_observation\", \"process_samples\"])"
            ),
            "{}",
            output.rust
        );
        assert!(
            output.rust.contains(
                "rh_json_array_len(&context, &[\"process_observation\", \"automation_processes\"])"
            ),
            "{}",
            output.rust
        );
        assert_eq!(output.rust.matches("rh_host_eval_int(\"").count(), 0);
    }

    #[test]
    fn cdylib_transpile_empty_child_list_push_and_arg_native() {
        let output = transpile_cdylib_with_mode(
            r#"
fn take_children(owned_children) {
    owned_children.len
}
fn entry() {
    let owned_children = [];
    let command = std::process::command("true");
    let child = command.start();
    owned_children.push(child);
    take_children(owned_children)
}
"#,
        )
        .expect("transpile");
        assert_eq!(
            output.execution_mode,
            CdylibExecutionMode::Native,
            "{}",
            output.rust
        );
        assert!(
            output.rust.contains("let mut owned_children = Vec::new();"),
            "{}",
            output.rust
        );
        assert!(
            output
                .rust
                .contains("owned_children.push(rh_child_share(&mut child));"),
            "{}",
            output.rust
        );
        assert!(
            output
                .rust
                .contains("take_children(owned_children.clone())"),
            "{}",
            output.rust
        );
        assert_eq!(output.rust.matches("rh_host_eval_int(\"").count(), 0);
    }

    #[test]
    fn cdylib_transpile_command_env_remove_and_task_sleep_native() {
        let output = transpile_cdylib_with_mode(
            r#"
fn entry() {
    let command = std::process::command("true");
    command.env_remove("AGENTERM_IPC_ADDRESS");
    rh::task::sleep(std::time::Duration::from_millis(1));
    0
}
"#,
        )
        .expect("transpile");
        assert_eq!(
            output.execution_mode,
            CdylibExecutionMode::Native,
            "{}",
            output.rust
        );
        assert!(
            output.rust.contains(
                "rh_command_env_remove(&mut command, &String::from(\"AGENTERM_IPC_ADDRESS\"));"
            ),
            "{}",
            output.rust
        );
        assert!(
            output.rust.contains(
                "std::thread::sleep(std::time::Duration::from_millis((1).max(0) as u64));"
            ),
            "{}",
            output.rust
        );
        assert_eq!(output.rust.matches("rh_host_eval_int(\"").count(), 0);
    }

    #[test]
    fn cdylib_transpile_emits_set_map_loop_value_assignments() {
        let source = include_str!("../../../fixtures/rh/set-map-loop-assign-probe.rh");
        let output = transpile_cdylib_with_mode(source).expect("transpile");
        assert_eq!(
            output.execution_mode,
            CdylibExecutionMode::Native,
            "{}",
            output.rust
        );
        assert!(
            output.rust.contains("names.insert(rh_json_as_str(&name))"),
            "{}",
            output.rust
        );
    }

    #[test]
    fn cdylib_transpile_smoke_scripts_past_assign_lhs_blockers() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("repo root");
        for rel in [
            "scripts/rh/diagnostic-bundle-selftest.rh",
            "scripts/rh/fresh-clone-rehearsal.rh",
            "scripts/rh/platform-ux-parity-smoke.rh",
            "scripts/rh/script-smoke.rh",
        ] {
            let _ = assert_transpile_past_assign_lhs_blockers(&root, rel);
        }
    }

    #[test]
    fn for_span_overflow_rejects_native_pack() {
        let source = include_str!("../../../fixtures/rh/for-span-overflow.rh");
        let err = transpile_cdylib_with_mode(source).expect_err("oversized for must fail");
        assert!(
            matches!(err, RhError::Transpile(ref msg) if msg.contains("4096")),
            "expected span limit error, got {err:?}"
        );
    }

    #[test]
    fn split_for_loop_stays_native() {
        let source = r#"
fn entry() {
    let line = "a,b,c";
    for piece in line.split(",") {
        if piece.len > 0 {
            return 1;
        }
    }
    0
}
"#;
        let output = transpile_cdylib_with_mode(source).expect("transpile");
        assert_eq!(output.execution_mode, CdylibExecutionMode::Native);
        assert!(
            output.rust.contains("rh_string_split("),
            "expected native string split for-loop:\n{}",
            output.rust
        );
        assert_eq!(output.rust.matches("rh_host_eval_int(\"").count(), 0);
    }

    #[test]
    fn rh_host_api_namespace_fixture_transpiles_native() {
        let source = include_str!("../tests/fixtures/rh_host_api_json_task.rh");
        let output = transpile_cdylib_with_mode(source).expect("transpile rh:: host api");
        assert_eq!(
            output.execution_mode,
            CdylibExecutionMode::Native,
            "{:?}\n{}",
            output.execution_mode,
            output.rust
        );
        assert!(output.rust.contains("rh_json_parse("));
        assert!(output.rust.contains("std::thread::sleep("));
        assert!(
            !output.rust.contains("rh_host_eval_int(\"rh::json::"),
            "{}",
            output.rust
        );
    }
}
