use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Component, Path, PathBuf},
    sync::Mutex,
};

use rhai::{
    AST, Dynamic, Engine, EvalAltResult, Module, ModuleResolver, Position, Shared,
    module_resolvers::FileModuleResolver,
};
use serde::{Deserialize, Serialize};

use crate::{
    script_catalog::{
        SCRIPT_CATALOG_SCHEMA_VERSION, ScriptApiStatus, entries as script_api_entries,
    },
    script_protocol::{SCRIPT_API_VERSION, ScriptBudgets},
};

pub const SCRIPT_TASK_MANIFEST: &str = "agenterm.tasks.json";
pub const SCRIPT_TASK_SCHEMA_VERSION: u32 = 3;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawManifest {
    schema_version: u32,
    project: RawProject,
    #[serde(default)]
    contracts: HashMap<String, ScriptTaskContract>,
    tasks: Vec<serde_json::Value>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawProject {
    id: String,
    version: String,
    requires: ScriptProjectRequirements,
    #[serde(default)]
    origin: Option<ScriptProjectOrigin>,
    #[serde(default)]
    provenance: Option<ScriptProjectProvenance>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScriptProjectRequirements {
    pub script_api: ScriptApiRequirement,
    pub capabilities: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScriptApiRequirement {
    pub minimum: u32,
    pub maximum: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScriptProjectOrigin {
    pub kind: ScriptProjectOriginKind,
    pub id: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScriptProjectOriginKind {
    Local,
    Repository,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScriptProjectProvenance {
    pub producer: String,
    pub revision: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTask {
    id: String,
    #[serde(default)]
    description: String,
    entry: String,
    #[serde(default = "default_profile")]
    profile: String,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    env: Vec<String>,
    #[serde(default)]
    dependencies: Vec<String>,
    #[serde(default = "default_platforms")]
    platforms: Vec<ScriptTaskPlatform>,
    #[serde(default)]
    side_effects: Vec<ScriptTaskSideEffect>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScriptTaskContract {
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
    pub budget: ScriptTaskBudget,
    pub network: Vec<ScriptTaskNetwork>,
    pub evidence: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScriptTaskBudget {
    pub timeout_ms: u64,
    pub max_operations: u64,
    pub max_output_bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_collection_items: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_string_bytes: Option<u64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ScriptTaskCatalog {
    pub schema_version: u32,
    pub runtime_version: &'static str,
    pub script_api_version: u32,
    pub script_catalog_schema_version: u32,
    pub manifest_path: String,
    pub project_root: String,
    pub project_id: String,
    pub project_version: String,
    pub requirements: ScriptProjectRequirements,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin: Option<ScriptProjectOrigin>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provenance: Option<ScriptProjectProvenance>,
    pub compatible: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compatibility_reason: Option<String>,
    pub tasks: Vec<ScriptTaskEntry>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ScriptTaskEntry {
    pub id: String,
    pub description: String,
    pub status: ScriptTaskStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub degraded_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entry: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    pub args: Vec<String>,
    pub env: Vec<String>,
    pub dependencies: Vec<String>,
    pub platforms: Vec<ScriptTaskPlatform>,
    pub side_effects: Vec<ScriptTaskSideEffect>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contract: Option<ScriptTaskContract>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScriptTaskPlatform {
    Windows,
    Linux,
    Macos,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScriptTaskSideEffect {
    RepositoryWrite,
    ArtifactWrite,
    ProcessSpawn,
    GuiSpawn,
    NetworkLoopback,
    GitMutation,
    RemotePublish,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScriptTaskNetwork {
    DependencyFetch,
    Loopback,
    RemotePublish,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScriptTaskStatus {
    Ready,
    Degraded,
}

#[derive(Clone, Debug)]
pub struct ResolvedScriptTask {
    pub id: String,
    pub entry: PathBuf,
    pub project_root: PathBuf,
    pub profile: String,
    pub cwd: PathBuf,
    pub args: Vec<String>,
    pub env: Vec<String>,
    pub budget: Option<ScriptTaskBudget>,
}

pub struct ProjectModuleResolver {
    root: PathBuf,
    files: FileModuleResolver,
    resolving: Mutex<HashSet<PathBuf>>,
}

impl ProjectModuleResolver {
    pub fn new(root: &Path) -> Result<Self, String> {
        let root = fs::canonicalize(root)
            .map_err(|error| format!("script_project_root: {}: {error}", root.display()))?;
        if !root.is_dir() {
            return Err(format!(
                "script_project_root: {} is not a directory",
                root.display()
            ));
        }
        Ok(Self {
            files: FileModuleResolver::new_with_path(&root),
            root,
            resolving: Mutex::new(HashSet::new()),
        })
    }

    fn checked_path(&self, path: &str, position: Position) -> Result<PathBuf, Box<EvalAltResult>> {
        if path.is_empty()
            || path.len() > 4096
            || Path::new(path).is_absolute()
            || Path::new(path).components().any(|component| {
                matches!(
                    component,
                    Component::ParentDir | Component::RootDir | Component::Prefix(_)
                )
            })
        {
            return Err(EvalAltResult::ErrorRuntime(
                Dynamic::from(format!(
                    "script_module_root_escape: import must be relative to the project root: {path}"
                )),
                position,
            )
            .into());
        }
        let mut candidate = self.root.join(path);
        candidate.set_extension("rhai");
        if let Ok(canonical) = fs::canonicalize(&candidate) {
            if !canonical.starts_with(&self.root) {
                return Err(EvalAltResult::ErrorRuntime(
                    Dynamic::from(format!(
                        "script_module_root_escape: import resolves outside the project root: {path}"
                    )),
                    position,
                )
                .into());
            }
            return Ok(canonical);
        }
        Ok(candidate)
    }
}

impl ModuleResolver for ProjectModuleResolver {
    fn resolve(
        &self,
        engine: &Engine,
        source: Option<&str>,
        path: &str,
        position: Position,
    ) -> Result<Shared<Module>, Box<EvalAltResult>> {
        let checked = self.checked_path(path, position)?;
        {
            let mut resolving = self.resolving.lock().map_err(|_| {
                Box::<EvalAltResult>::from("script_module_state: resolver lock poisoned")
            })?;
            if !resolving.insert(checked.clone()) {
                return Err(EvalAltResult::ErrorRuntime(
                    Dynamic::from(format!("script_module_cycle: {path}")),
                    position,
                )
                .into());
            }
        }
        let result = self.files.resolve(engine, source, path, position);
        if let Ok(mut resolving) = self.resolving.lock() {
            resolving.remove(&checked);
        }
        result
    }

    fn resolve_ast(
        &self,
        engine: &Engine,
        source: Option<&str>,
        path: &str,
        position: Position,
    ) -> Option<Result<AST, Box<EvalAltResult>>> {
        if let Err(error) = self.checked_path(path, position) {
            return Some(Err(error));
        }
        self.files.resolve_ast(engine, source, path, position)
    }
}

pub fn validate_project_imports(
    engine: &Engine,
    root: &Path,
    source: &str,
) -> Result<Vec<String>, String> {
    let root = fs::canonicalize(root)
        .map_err(|error| format!("script_project_root: {}: {error}", root.display()))?;
    let mut visiting = HashSet::new();
    let mut visited = HashSet::new();
    let mut module_sources = Vec::new();
    validate_import_tree(
        engine,
        &root,
        source,
        &mut visiting,
        &mut visited,
        &mut module_sources,
    )?;
    Ok(module_sources)
}

fn validate_import_tree(
    engine: &Engine,
    root: &Path,
    source: &str,
    visiting: &mut HashSet<PathBuf>,
    visited: &mut HashSet<PathBuf>,
    module_sources: &mut Vec<String>,
) -> Result<(), String> {
    for import in literal_imports(source)? {
        let path = checked_module_file(root, &import)?;
        if visited.contains(&path) {
            continue;
        }
        if !visiting.insert(path.clone()) {
            return Err(format!("script_module_cycle: {import}"));
        }
        let bytes =
            fs::read(&path).map_err(|error| format!("script_module_missing: {import}: {error}"))?;
        if bytes.len() > 256 * 1024 {
            return Err(format!(
                "script_module_too_large: {import} exceeds 262144 bytes"
            ));
        }
        let module_source = String::from_utf8(bytes)
            .map_err(|error| format!("script_module_encoding: {import}: {error}"))?;
        engine
            .compile(&module_source)
            .map_err(|error| format!("script_module_parse: {import}: {error}"))?;
        validate_import_tree(
            engine,
            root,
            &module_source,
            visiting,
            visited,
            module_sources,
        )?;
        module_sources.push(module_source);
        visiting.remove(&path);
        visited.insert(path);
    }
    Ok(())
}

fn checked_module_file(root: &Path, import: &str) -> Result<PathBuf, String> {
    if import.is_empty()
        || import.len() > 4096
        || Path::new(import).is_absolute()
        || Path::new(import).components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(format!("script_module_root_escape: {import}"));
    }
    let mut candidate = root.join(import);
    candidate.set_extension("rhai");
    let canonical = fs::canonicalize(&candidate)
        .map_err(|error| format!("script_module_missing: {import}: {error}"))?;
    if !canonical.starts_with(root) {
        return Err(format!("script_module_root_escape: {import}"));
    }
    if !canonical.is_file() {
        return Err(format!("script_module_missing: {import} is not a file"));
    }
    Ok(canonical)
}

fn literal_imports(source: &str) -> Result<Vec<String>, String> {
    let bytes = source.as_bytes();
    let mut imports = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'/' if bytes.get(index + 1) == Some(&b'/') => {
                index += 2;
                while index < bytes.len() && bytes[index] != b'\n' {
                    index += 1;
                }
            }
            b'/' if bytes.get(index + 1) == Some(&b'*') => {
                index += 2;
                while index + 1 < bytes.len() && !(bytes[index] == b'*' && bytes[index + 1] == b'/')
                {
                    index += 1;
                }
                index = (index + 2).min(bytes.len());
            }
            b'"' | b'`' => skip_script_string(bytes, &mut index)?,
            byte if byte.is_ascii_alphabetic() || byte == b'_' => {
                let start = index;
                index += 1;
                while index < bytes.len()
                    && (bytes[index].is_ascii_alphanumeric() || bytes[index] == b'_')
                {
                    index += 1;
                }
                if &source[start..index] != "import" {
                    continue;
                }
                skip_script_spacing(bytes, &mut index);
                let Some(delimiter @ (b'"' | b'`')) = bytes.get(index).copied() else {
                    return Err(
                        "script_module_import_literal: import path must be a string literal"
                            .to_owned(),
                    );
                };
                index += 1;
                let path_start = index;
                while index < bytes.len() && bytes[index] != delimiter {
                    if bytes[index] == b'\\' {
                        return Err(
                            "script_module_import_literal: escaped import paths are unsupported"
                                .to_owned(),
                        );
                    }
                    index += 1;
                }
                if index >= bytes.len() {
                    return Err("script_module_import_literal: unterminated import path".to_owned());
                }
                imports.push(source[path_start..index].to_owned());
                index += 1;
            }
            _ => index += 1,
        }
    }
    Ok(imports)
}

fn skip_script_spacing(bytes: &[u8], index: &mut usize) {
    while *index < bytes.len() && bytes[*index].is_ascii_whitespace() {
        *index += 1;
    }
}

fn skip_script_string(bytes: &[u8], index: &mut usize) -> Result<(), String> {
    let delimiter = bytes[*index];
    *index += 1;
    while *index < bytes.len() {
        if bytes[*index] == delimiter {
            *index += 1;
            return Ok(());
        }
        if delimiter == b'"' && bytes[*index] == b'\\' {
            *index += 1;
            if *index >= bytes.len() {
                break;
            }
        }
        *index += 1;
    }
    Err("script_parse: unterminated string while scanning imports".to_owned())
}

pub fn discover_task_manifest(start: &Path) -> Result<PathBuf, String> {
    let mut current = if start.is_dir() {
        start.to_path_buf()
    } else {
        start
            .parent()
            .ok_or_else(|| "task_manifest_not_found: start path has no parent".to_owned())?
            .to_path_buf()
    };
    loop {
        let candidate = current.join(SCRIPT_TASK_MANIFEST);
        if candidate.is_file() {
            return Ok(candidate);
        }
        if !current.pop() {
            return Err(format!(
                "task_manifest_not_found: no {SCRIPT_TASK_MANIFEST} found from {}",
                start.display()
            ));
        }
    }
}

pub fn load_task_catalog(path: &Path) -> Result<ScriptTaskCatalog, String> {
    let manifest_path = fs::canonicalize(path)
        .map_err(|error| format!("task_manifest_read: {}: {error}", path.display()))?;
    let project_root = manifest_path
        .parent()
        .ok_or_else(|| "task_manifest_root: manifest has no parent directory".to_owned())?
        .to_path_buf();
    let bytes = fs::read(&manifest_path)
        .map_err(|error| format!("task_manifest_read: {}: {error}", manifest_path.display()))?;
    if bytes.len() > 256 * 1024 {
        return Err("task_manifest_too_large: maximum is 262144 bytes".to_owned());
    }
    let raw: RawManifest =
        serde_json::from_slice(&bytes).map_err(|error| format!("task_manifest_json: {error}"))?;
    if !(2..=SCRIPT_TASK_SCHEMA_VERSION).contains(&raw.schema_version) {
        return Err(format!(
            "task_manifest_version: supported 2..={}, requested {}",
            SCRIPT_TASK_SCHEMA_VERSION, raw.schema_version
        ));
    }
    let manifest_schema_version = raw.schema_version;
    validate_identity(&raw.project.id, "task_project_id")?;
    validate_version(&raw.project.version)?;
    validate_origin(raw.project.origin.as_ref())?;
    validate_provenance(raw.project.provenance.as_ref())?;
    let (compatible, compatibility_reason) = validate_requirements(&raw.project.requires)?;
    if raw.tasks.len() > 256 {
        return Err("task_manifest_tasks: maximum is 256".to_owned());
    }
    if raw.contracts.len() > 256 {
        return Err("task_manifest_contracts: maximum is 256".to_owned());
    }
    for id in raw.contracts.keys() {
        validate_identity(id, "task_contract_id")?;
        if !raw
            .tasks
            .iter()
            .any(|task| task.get("id").and_then(serde_json::Value::as_str) == Some(id))
        {
            return Err(format!("task_contract_unknown: {id}"));
        }
    }

    let mut seen = HashSet::new();
    let mut tasks = Vec::with_capacity(raw.tasks.len());
    for (index, value) in raw.tasks.into_iter().enumerate() {
        let fallback_id = value
            .get("id")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| format!("#{index}"));
        let parsed = serde_json::from_value::<RawTask>(value);
        let entry = match parsed {
            Ok(task) => {
                let contract = raw.contracts.get(&task.id).cloned();
                catalog_task(
                    &project_root,
                    task,
                    contract,
                    manifest_schema_version >= 3,
                    &mut seen,
                )
            }
            Err(error) => degraded_task(
                fallback_id,
                format!("task_manifest_entry: index {index}: {error}"),
            ),
        };
        tasks.push(entry);
    }
    validate_task_graph(&mut tasks);

    Ok(ScriptTaskCatalog {
        schema_version: manifest_schema_version,
        runtime_version: env!("CARGO_PKG_VERSION"),
        script_api_version: SCRIPT_API_VERSION,
        script_catalog_schema_version: SCRIPT_CATALOG_SCHEMA_VERSION,
        manifest_path: manifest_path.display().to_string(),
        project_root: project_root.display().to_string(),
        project_id: raw.project.id,
        project_version: raw.project.version,
        requirements: raw.project.requires,
        origin: raw.project.origin,
        provenance: raw.project.provenance,
        compatible,
        compatibility_reason,
        tasks,
    })
}

pub fn resolve_task(catalog: &ScriptTaskCatalog, id: &str) -> Result<ResolvedScriptTask, String> {
    let matches = catalog
        .tasks
        .iter()
        .filter(|task| task.id == id)
        .collect::<Vec<_>>();
    if matches.is_empty() {
        return Err(format!("task_not_found: {id}"));
    }
    if matches.len() != 1 {
        return Err(format!("task_duplicate: {id}"));
    }
    let task = matches[0];
    if task.status == ScriptTaskStatus::Degraded {
        return Err(format!(
            "task_degraded: {id}: {}",
            task.degraded_reason.as_deref().unwrap_or("invalid task")
        ));
    }
    if !catalog.compatible {
        return Err(format!(
            "task_project_incompatible: {}",
            catalog
                .compatibility_reason
                .as_deref()
                .unwrap_or("project requirements are not satisfied")
        ));
    }
    let root = PathBuf::from(&catalog.project_root);
    let entry = task
        .entry
        .as_deref()
        .ok_or_else(|| format!("task_degraded: {id}: entry unavailable"))?;
    let cwd = task.cwd.as_deref().unwrap_or(".");
    let entry = validate_relative_path(&root, entry, true)?;
    let cwd = validate_relative_path(&root, cwd, false)?;
    Ok(ResolvedScriptTask {
        id: id.to_owned(),
        entry,
        project_root: root.clone(),
        profile: task.profile.clone().unwrap_or_else(default_profile),
        cwd,
        args: task.args.clone(),
        env: task.env.clone(),
        budget: task
            .contract
            .as_ref()
            .map(|contract| contract.budget.clone()),
    })
}

fn catalog_task(
    root: &Path,
    task: RawTask,
    contract: Option<ScriptTaskContract>,
    require_contract: bool,
    seen: &mut HashSet<String>,
) -> ScriptTaskEntry {
    let validation = validate_identity(&task.id, "task_id")
        .and_then(|_| {
            if seen.insert(task.id.clone()) {
                Ok(())
            } else {
                Err(format!("task_duplicate: {}", task.id))
            }
        })
        .and_then(|_| validate_description(&task.description))
        .and_then(|_| validate_profile(&task.profile))
        .and_then(|_| validate_relative_path(root, &task.entry, true).map(|_| ()))
        .and_then(|_| {
            validate_relative_path(root, task.cwd.as_deref().unwrap_or("."), false).map(|_| ())
        })
        .and_then(|_| validate_args(&task.args))
        .and_then(|_| validate_env(&task.env))
        .and_then(|_| validate_dependencies(&task.dependencies))
        .and_then(|_| validate_nonempty_unique(&task.platforms, "task_platforms"))
        .and_then(|_| validate_unique(&task.side_effects, "task_side_effects"))
        .and_then(|_| match contract.as_ref() {
            Some(contract) => validate_task_contract(contract),
            None if require_contract => Err(format!("task_contract_missing: {}", task.id)),
            None => Ok(()),
        });

    if let Err(reason) = validation {
        return degraded_task(task.id, reason);
    }
    ScriptTaskEntry {
        id: task.id,
        description: task.description,
        status: ScriptTaskStatus::Ready,
        degraded_reason: None,
        entry: Some(normalize_relative(&task.entry)),
        profile: Some(task.profile),
        cwd: task.cwd.map(|path| normalize_relative(&path)),
        args: task.args,
        env: task.env,
        dependencies: task.dependencies,
        platforms: task.platforms,
        side_effects: task.side_effects,
        contract,
    }
}

fn degraded_task(id: String, reason: String) -> ScriptTaskEntry {
    ScriptTaskEntry {
        id,
        description: String::new(),
        status: ScriptTaskStatus::Degraded,
        degraded_reason: Some(reason),
        entry: None,
        profile: None,
        cwd: None,
        args: Vec::new(),
        env: Vec::new(),
        dependencies: Vec::new(),
        platforms: Vec::new(),
        side_effects: Vec::new(),
        contract: None,
    }
}

fn validate_task_contract(contract: &ScriptTaskContract) -> Result<(), String> {
    validate_contract_ids(&contract.inputs, "task_contract_inputs", false)?;
    validate_contract_ids(&contract.outputs, "task_contract_outputs", false)?;
    validate_contract_ids(&contract.evidence, "task_contract_evidence", false)?;
    validate_unique(&contract.network, "task_contract_network")?;
    let hard_limits = ScriptBudgets::hard_limits();
    if contract.budget.timeout_ms == 0 || contract.budget.timeout_ms > hard_limits.wall_time_ms {
        return Err("task_contract_budget_timeout_ms: expected 1..3600000".to_owned());
    }
    if contract.budget.max_operations == 0
        || contract.budget.max_operations > hard_limits.operations
    {
        return Err("task_contract_budget_max_operations: expected 1..100000000".to_owned());
    }
    if contract.budget.max_output_bytes == 0
        || contract.budget.max_output_bytes > hard_limits.output_bytes as u64
    {
        return Err("task_contract_budget_max_output_bytes: expected 1..1048576".to_owned());
    }
    if contract
        .budget
        .max_collection_items
        .is_some_and(|value| value == 0 || value > hard_limits.collection_items as u64)
    {
        return Err("task_contract_budget_max_collection_items: expected 1..100000".to_owned());
    }
    if contract
        .budget
        .max_string_bytes
        .is_some_and(|value| value == 0 || value > hard_limits.string_bytes as u64)
    {
        return Err("task_contract_budget_max_string_bytes: expected 1..8388608".to_owned());
    }
    Ok(())
}

fn validate_contract_ids(values: &[String], field: &str, allow_empty: bool) -> Result<(), String> {
    if !allow_empty && values.is_empty() {
        return Err(format!("{field}: at least one stable ID is required"));
    }
    if values.len() > 64 {
        return Err(format!("{field}: maximum is 64"));
    }
    let mut seen = HashSet::new();
    for value in values {
        validate_identity(value, field)?;
        if !seen.insert(value.as_str()) {
            return Err(format!("{field}: duplicate {value}"));
        }
    }
    Ok(())
}

fn validate_dependencies(values: &[String]) -> Result<(), String> {
    if values.len() > 64 {
        return Err("task_dependencies: maximum is 64".to_owned());
    }
    let mut seen = HashSet::new();
    for value in values {
        validate_identity(value, "task_dependency")?;
        if !seen.insert(value.as_str()) {
            return Err(format!("task_dependency_duplicate: {value}"));
        }
    }
    Ok(())
}

fn validate_nonempty_unique<T>(values: &[T], field: &str) -> Result<(), String>
where
    T: Eq + std::hash::Hash,
{
    if values.is_empty() {
        return Err(format!("{field}: at least one value is required"));
    }
    if values.len() > 16 {
        return Err(format!("{field}: maximum is 16"));
    }
    let mut seen = HashSet::new();
    if values.iter().any(|value| !seen.insert(value)) {
        return Err(format!("{field}: duplicate value"));
    }
    Ok(())
}

fn validate_unique<T>(values: &[T], field: &str) -> Result<(), String>
where
    T: Eq + std::hash::Hash,
{
    if values.len() > 16 {
        return Err(format!("{field}: maximum is 16"));
    }
    let mut seen = HashSet::new();
    if values.iter().any(|value| !seen.insert(value)) {
        return Err(format!("{field}: duplicate value"));
    }
    Ok(())
}

fn default_platforms() -> Vec<ScriptTaskPlatform> {
    vec![
        ScriptTaskPlatform::Windows,
        ScriptTaskPlatform::Linux,
        ScriptTaskPlatform::Macos,
    ]
}

fn validate_task_graph(tasks: &mut [ScriptTaskEntry]) {
    let ready_ids = tasks
        .iter()
        .filter(|task| task.status == ScriptTaskStatus::Ready)
        .map(|task| task.id.clone())
        .collect::<HashSet<_>>();
    for task in tasks
        .iter_mut()
        .filter(|task| task.status == ScriptTaskStatus::Ready)
    {
        if task
            .dependencies
            .iter()
            .any(|dependency| dependency == &task.id)
        {
            task.status = ScriptTaskStatus::Degraded;
            task.degraded_reason = Some(format!("task_dependency_self: {}", task.id));
        } else if let Some(dependency) = task
            .dependencies
            .iter()
            .find(|dependency| !ready_ids.contains(*dependency))
        {
            task.status = ScriptTaskStatus::Degraded;
            task.degraded_reason = Some(format!("task_dependency_unknown: {dependency}"));
        }
    }

    let graph = tasks
        .iter()
        .filter(|task| task.status == ScriptTaskStatus::Ready)
        .map(|task| (task.id.clone(), task.dependencies.clone()))
        .collect::<std::collections::HashMap<_, _>>();
    let cyclic = graph
        .keys()
        .filter(|id| task_dependency_cycle(id, &graph, &mut HashSet::new(), &mut HashSet::new()))
        .cloned()
        .collect::<HashSet<_>>();
    for task in tasks.iter_mut().filter(|task| cyclic.contains(&task.id)) {
        task.status = ScriptTaskStatus::Degraded;
        task.degraded_reason = Some(format!("task_dependency_cycle: {}", task.id));
    }
}

fn task_dependency_cycle(
    id: &str,
    graph: &std::collections::HashMap<String, Vec<String>>,
    visiting: &mut HashSet<String>,
    visited: &mut HashSet<String>,
) -> bool {
    if visiting.contains(id) {
        return true;
    }
    if visited.contains(id) {
        return false;
    }
    visiting.insert(id.to_owned());
    let cyclic = graph.get(id).is_some_and(|dependencies| {
        dependencies
            .iter()
            .any(|dependency| task_dependency_cycle(dependency, graph, visiting, visited))
    });
    visiting.remove(id);
    visited.insert(id.to_owned());
    cyclic
}

fn validate_identity(value: &str, field: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 64
        || !value.bytes().enumerate().all(|(index, byte)| match byte {
            b'a'..=b'z' | b'0'..=b'9' => true,
            b'.' | b'_' | b'-' => index > 0,
            _ => false,
        })
    {
        return Err(format!(
            "{field}: expected 1..64 lowercase ASCII letters, digits, '.', '_' or '-'"
        ));
    }
    Ok(())
}

fn validate_version(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'+' | b'-'))
    {
        return Err("task_project_version: invalid version identity".to_owned());
    }
    Ok(())
}

fn validate_requirements(
    requirements: &ScriptProjectRequirements,
) -> Result<(bool, Option<String>), String> {
    let api = &requirements.script_api;
    if api.minimum == 0 || api.maximum == 0 || api.minimum > api.maximum {
        return Err(
            "task_project_script_api: expected a non-zero minimum not greater than maximum"
                .to_owned(),
        );
    }
    if requirements.capabilities.len() > 128 {
        return Err("task_project_capabilities: maximum is 128".to_owned());
    }

    let mut seen = HashSet::new();
    for capability in &requirements.capabilities {
        validate_capability_id(capability)?;
        if !seen.insert(capability.as_str()) {
            return Err(format!("task_project_capability_duplicate: {capability}"));
        }
    }

    if SCRIPT_API_VERSION < api.minimum || SCRIPT_API_VERSION > api.maximum {
        return Ok((
            false,
            Some(format!(
                "script_api_version: runtime {SCRIPT_API_VERSION}, required {}..{}",
                api.minimum, api.maximum
            )),
        ));
    }

    let entries = script_api_entries();
    for capability in &requirements.capabilities {
        match entries
            .iter()
            .find(|entry| entry.stable_id == capability.as_str())
        {
            Some(entry) if entry.status == ScriptApiStatus::Shipped => {}
            Some(_) => {
                return Ok((false, Some(format!("capability_unavailable: {capability}"))));
            }
            None => {
                return Ok((false, Some(format!("capability_unknown: {capability}"))));
            }
        }
    }
    Ok((true, None))
}

fn validate_capability_id(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 128
        || !value.bytes().enumerate().all(|(index, byte)| match byte {
            b'a'..=b'z' | b'0'..=b'9' => true,
            b'.' | b'_' | b'-' => index > 0,
            _ => false,
        })
    {
        return Err(format!(
            "task_project_capability_id: invalid stable ID {value:?}"
        ));
    }
    Ok(())
}

fn validate_origin(origin: Option<&ScriptProjectOrigin>) -> Result<(), String> {
    if let Some(origin) = origin {
        validate_identity(&origin.id, "task_project_origin_id")?;
    }
    Ok(())
}

fn validate_provenance(provenance: Option<&ScriptProjectProvenance>) -> Result<(), String> {
    if let Some(provenance) = provenance {
        validate_identity(&provenance.producer, "task_project_provenance_producer")?;
        validate_version(&provenance.revision).map_err(|_| {
            "task_project_provenance_revision: invalid revision identity".to_owned()
        })?;
    }
    Ok(())
}

fn validate_description(value: &str) -> Result<(), String> {
    if value.len() > 512 || value.chars().any(char::is_control) {
        return Err("task_description: maximum is 512 non-control UTF-8 characters".to_owned());
    }
    Ok(())
}

fn validate_profile(value: &str) -> Result<(), String> {
    if matches!(value, "local" | "pure" | "observe") {
        Ok(())
    } else {
        Err(format!("task_profile: unknown profile {value}"))
    }
}

fn validate_relative_path(root: &Path, value: &str, file: bool) -> Result<PathBuf, String> {
    if value.is_empty() || value.len() > 4096 || Path::new(value).is_absolute() {
        return Err("task_path: path must be a bounded relative path".to_owned());
    }
    if Path::new(value).components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return Err(format!("task_path_escape: {value}"));
    }
    let resolved = fs::canonicalize(root.join(value))
        .map_err(|error| format!("task_path_missing: {value}: {error}"))?;
    if !resolved.starts_with(root) {
        return Err(format!("task_path_escape: {value}"));
    }
    let matches_kind = if file {
        resolved.is_file()
    } else {
        resolved.is_dir()
    };
    if !matches_kind {
        return Err(format!(
            "task_path_type: {value} is not a {}",
            if file { "file" } else { "directory" }
        ));
    }
    Ok(resolved)
}

fn validate_args(values: &[String]) -> Result<(), String> {
    if values.len() > 128
        || values
            .iter()
            .any(|value| value.len() > 4096 || value.contains('\0'))
    {
        return Err("task_args: maximum is 128 bounded strings".to_owned());
    }
    Ok(())
}

fn validate_env(values: &[String]) -> Result<(), String> {
    if values.len() > 64 {
        return Err("task_env: maximum is 64 names".to_owned());
    }
    let mut seen = HashSet::new();
    for value in values {
        if value.is_empty()
            || value.len() > 128
            || value.contains('=')
            || value.contains('\0')
            || !seen.insert(value.to_ascii_uppercase())
        {
            return Err(format!(
                "task_env: invalid or duplicate environment name {value}"
            ));
        }
    }
    Ok(())
}

fn normalize_relative(value: &str) -> String {
    let normalized = Path::new(value)
        .components()
        .filter_map(|component| match component {
            Component::Normal(value) => Some(value.to_string_lossy().into_owned()),
            Component::CurDir => None,
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/");
    if normalized.is_empty() {
        ".".to_owned()
    } else {
        normalized
    }
}

fn default_profile() -> String {
    "local".to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (PathBuf, PathBuf) {
        let test_name = std::thread::current()
            .name()
            .unwrap_or("test")
            .chars()
            .map(|character| {
                if character.is_ascii_alphanumeric() {
                    character
                } else {
                    '-'
                }
            })
            .collect::<String>();
        let root = std::env::temp_dir().join(format!(
            "agenterm-task-project-{}-{}",
            std::process::id(),
            test_name
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("scripts")).unwrap();
        fs::write(root.join("scripts/check.rhai"), "40 + 2").unwrap();
        let manifest = root.join(SCRIPT_TASK_MANIFEST);
        fs::write(
            &manifest,
            r#"{
  "schema_version": 3,
  "project": {
    "id": "fixture",
    "version": "1.0.0",
    "requires": {
      "script_api": {"minimum": 2, "maximum": 2},
      "capabilities": ["runtime.project.named-task"]
    },
    "origin": {"kind": "repository", "id": "agenterm"},
    "provenance": {"producer": "agenterm-test", "revision": "fixture-1"}
  },
  "contracts": {
    "check": {
      "inputs": ["source-tree"],
      "outputs": ["check-result"],
      "budget": {
        "timeout_ms": 10000,
        "max_operations": 1000000,
        "max_output_bytes": 65536,
        "max_collection_items": 20000,
        "max_string_bytes": 524288
      },
      "network": [],
      "evidence": ["task.check"]
    },
    "broken": {
      "inputs": ["source-tree"],
      "outputs": ["broken-result"],
      "budget": {
        "timeout_ms": 10000,
        "max_operations": 1000000,
        "max_output_bytes": 65536
      },
      "network": [],
      "evidence": ["task.broken"]
    }
  },
  "tasks": [
    {
      "id": "check",
      "entry": "scripts/check.rhai",
      "args": ["default"],
      "dependencies": [],
      "platforms": ["windows", "linux", "macos"],
      "side_effects": ["artifact_write", "process_spawn"]
    },
    {"id": "broken", "entry": "../outside.rhai"},
    {"id": "check", "entry": "scripts/check.rhai"}
  ]
}"#,
        )
        .unwrap();
        (root, manifest)
    }

    #[test]
    fn catalog_keeps_invalid_and_duplicate_tasks_visible() {
        let (root, manifest) = fixture();
        let catalog = load_task_catalog(&manifest).unwrap();
        assert_eq!(catalog.tasks.len(), 3);
        assert_eq!(catalog.runtime_version, env!("CARGO_PKG_VERSION"));
        assert_eq!(catalog.script_api_version, SCRIPT_API_VERSION);
        assert_eq!(
            catalog.script_catalog_schema_version,
            SCRIPT_CATALOG_SCHEMA_VERSION
        );
        assert!(catalog.origin.is_some());
        assert!(catalog.provenance.is_some());
        assert_eq!(catalog.tasks[0].status, ScriptTaskStatus::Ready);
        assert_eq!(
            catalog.tasks[0].platforms,
            [
                ScriptTaskPlatform::Windows,
                ScriptTaskPlatform::Linux,
                ScriptTaskPlatform::Macos
            ]
        );
        assert_eq!(
            catalog.tasks[0].side_effects,
            [
                ScriptTaskSideEffect::ArtifactWrite,
                ScriptTaskSideEffect::ProcessSpawn
            ]
        );
        let contract = catalog.tasks[0].contract.as_ref().unwrap();
        assert_eq!(contract.inputs, ["source-tree"]);
        assert_eq!(contract.outputs, ["check-result"]);
        assert_eq!(contract.budget.timeout_ms, 10_000);
        assert_eq!(contract.budget.max_operations, 1_000_000);
        assert_eq!(contract.budget.max_output_bytes, 65_536);
        assert_eq!(contract.budget.max_collection_items, Some(20_000));
        assert_eq!(contract.budget.max_string_bytes, Some(524_288));
        assert!(contract.network.is_empty());
        assert_eq!(contract.evidence, ["task.check"]);
        assert_eq!(catalog.tasks[1].status, ScriptTaskStatus::Degraded);
        assert_eq!(catalog.tasks[2].status, ScriptTaskStatus::Degraded);
        assert!(resolve_task(&catalog, "check").is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn schema_three_requires_contracts_while_schema_two_remains_readable() {
        let (root, manifest) = fixture();
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
        value["tasks"].as_array_mut().unwrap().pop();
        value["contracts"].as_object_mut().unwrap().remove("check");
        fs::write(&manifest, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
        let catalog = load_task_catalog(&manifest).unwrap();
        let task = catalog
            .tasks
            .iter()
            .find(|task| task.id == "check")
            .unwrap();
        assert_eq!(task.status, ScriptTaskStatus::Degraded);
        assert_eq!(
            task.degraded_reason.as_deref(),
            Some("task_contract_missing: check")
        );

        value["schema_version"] = serde_json::json!(2);
        value.as_object_mut().unwrap().remove("contracts");
        fs::write(&manifest, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
        let catalog = load_task_catalog(&manifest).unwrap();
        assert_eq!(catalog.schema_version, 2);
        let task = catalog
            .tasks
            .iter()
            .find(|task| task.id == "check")
            .unwrap();
        assert_eq!(task.status, ScriptTaskStatus::Ready);
        assert!(task.contract.is_none());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn task_contract_budgets_cannot_exceed_runtime_hard_limits() {
        for (field, value, expected) in [
            (
                "timeout_ms",
                3_600_001_u64,
                "task_contract_budget_timeout_ms: expected 1..3600000",
            ),
            (
                "max_operations",
                100_000_001,
                "task_contract_budget_max_operations: expected 1..100000000",
            ),
            (
                "max_output_bytes",
                1_048_577,
                "task_contract_budget_max_output_bytes: expected 1..1048576",
            ),
        ] {
            let (root, manifest) = fixture();
            let mut manifest_value: serde_json::Value =
                serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
            manifest_value["contracts"]["check"]["budget"][field] = serde_json::json!(value);
            fs::write(
                &manifest,
                serde_json::to_vec_pretty(&manifest_value).unwrap(),
            )
            .unwrap();
            let catalog = load_task_catalog(&manifest).unwrap();
            let task = catalog
                .tasks
                .iter()
                .find(|task| task.id == "check")
                .unwrap();
            assert_eq!(task.status, ScriptTaskStatus::Degraded);
            assert_eq!(task.degraded_reason.as_deref(), Some(expected));
            fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn incompatible_requirements_remain_visible_but_cannot_resolve() {
        let (root, manifest) = fixture();
        let contents = fs::read_to_string(&manifest)
            .unwrap()
            .replace(
                r#""minimum": 2, "maximum": 2"#,
                r#""minimum": 3, "maximum": 3"#,
            )
            .replace(
                r#"{"id": "check", "entry": "scripts/check.rhai"}"#,
                r#"{"id": "second", "entry": "scripts/check.rhai"}"#,
            );
        fs::write(&manifest, contents).unwrap();
        let catalog = load_task_catalog(&manifest).unwrap();
        assert!(!catalog.compatible);
        assert!(
            catalog
                .compatibility_reason
                .as_deref()
                .is_some_and(|reason| reason.starts_with("script_api_version:"))
        );
        assert_eq!(catalog.tasks[0].status, ScriptTaskStatus::Ready);
        assert!(
            resolve_task(&catalog, "check")
                .unwrap_err()
                .starts_with("task_project_incompatible:")
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn provenance_hooks_accept_only_bounded_non_locator_identities() {
        let (root, manifest) = fixture();
        let contents = fs::read_to_string(&manifest).unwrap().replace(
            r#""origin": {"kind": "repository", "id": "agenterm"}"#,
            r#""origin": {"kind": "repository", "id": "https://token@example"}"#,
        );
        fs::write(&manifest, contents).unwrap();
        assert!(
            load_task_catalog(&manifest)
                .unwrap_err()
                .starts_with("task_project_origin_id:")
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn discovery_walks_to_the_project_root() {
        let (root, manifest) = fixture();
        assert_eq!(
            discover_task_manifest(&root.join("scripts")).unwrap(),
            manifest
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn task_graph_rejects_unknown_self_and_cyclic_dependencies() {
        let (root, manifest) = fixture();
        let contents = fs::read_to_string(&manifest)
            .unwrap()
            .replace(r#""broken": {"#, r#""unknown": {"#)
            .replace(
                "  },\n  \"tasks\": [",
                r#"    ,
    "self": {
      "inputs": ["source-tree"],
      "outputs": ["self-result"],
      "budget": {"timeout_ms": 10000, "max_operations": 1000000, "max_output_bytes": 65536},
      "network": [],
      "evidence": ["task.self"]
    },
    "cycle-a": {
      "inputs": ["source-tree"],
      "outputs": ["cycle-a-result"],
      "budget": {"timeout_ms": 10000, "max_operations": 1000000, "max_output_bytes": 65536},
      "network": [],
      "evidence": ["task.cycle-a"]
    },
    "cycle-b": {
      "inputs": ["source-tree"],
      "outputs": ["cycle-b-result"],
      "budget": {"timeout_ms": 10000, "max_operations": 1000000, "max_output_bytes": 65536},
      "network": [],
      "evidence": ["task.cycle-b"]
    }
  },
  "tasks": ["#,
            )
            .replace(
                r#"{"id": "broken", "entry": "../outside.rhai"}"#,
                r#"{"id": "unknown", "entry": "scripts/check.rhai", "dependencies": ["missing"]}"#,
            )
            .replace(
                r#"{"id": "check", "entry": "scripts/check.rhai"}"#,
                r#"{"id": "self", "entry": "scripts/check.rhai", "dependencies": ["self"]},
    {"id": "cycle-a", "entry": "scripts/check.rhai", "dependencies": ["cycle-b"]},
    {"id": "cycle-b", "entry": "scripts/check.rhai", "dependencies": ["cycle-a"]}"#,
            );
        fs::write(&manifest, contents).unwrap();
        let catalog = load_task_catalog(&manifest).unwrap();
        for (id, reason) in [
            ("unknown", "task_dependency_unknown: missing"),
            ("self", "task_dependency_self: self"),
            ("cycle-a", "task_dependency_cycle: cycle-a"),
            ("cycle-b", "task_dependency_cycle: cycle-b"),
        ] {
            let task = catalog.tasks.iter().find(|task| task.id == id).unwrap();
            assert_eq!(task.status, ScriptTaskStatus::Degraded);
            assert_eq!(task.degraded_reason.as_deref(), Some(reason));
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn module_resolver_loads_root_relative_modules_and_rejects_escape() {
        let (root, _) = fixture();
        fs::write(root.join("scripts/math.rhai"), "export const answer = 42;").unwrap();
        let mut engine = Engine::new();
        engine.set_module_resolver(ProjectModuleResolver::new(&root).unwrap());
        assert_eq!(
            engine
                .eval::<rhai::INT>(r#"import "scripts/math" as math; math::answer"#)
                .unwrap(),
            42
        );
        let error = engine
            .eval::<Dynamic>(r#"import "../outside" as outside;"#)
            .unwrap_err()
            .to_string();
        assert!(error.contains("script_module_root_escape"));

        fs::write(
            root.join("compile-only.rhai"),
            r#"throw "must not execute";"#,
        )
        .unwrap();
        validate_project_imports(&engine, &root, r#"import "compile-only" as compile_only;"#)
            .unwrap();

        fs::write(root.join("cycle-a.rhai"), r#"import "cycle-b" as b;"#).unwrap();
        fs::write(root.join("cycle-b.rhai"), r#"import "cycle-a" as a;"#).unwrap();
        let error = engine
            .compile_into_self_contained(&rhai::Scope::new(), r#"import "cycle-a" as cycle;"#)
            .unwrap_err()
            .to_string();
        assert!(error.contains("script_module_cycle"));
        fs::remove_dir_all(root).unwrap();
    }
}
