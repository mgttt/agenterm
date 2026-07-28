use std::{
    collections::HashSet,
    fs,
    path::{Component, Path, PathBuf},
    sync::Mutex,
};

use rhai::{
    AST, Dynamic, Engine, EvalAltResult, Module, ModuleResolver, Position, Shared,
    module_resolvers::FileModuleResolver,
};
use serde::{Deserialize, Serialize};

pub const SCRIPT_TASK_MANIFEST: &str = "agenterm.tasks.json";
pub const SCRIPT_TASK_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawManifest {
    schema_version: u32,
    project: RawProject,
    tasks: Vec<serde_json::Value>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawProject {
    id: String,
    version: String,
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
}

#[derive(Clone, Debug, Serialize)]
pub struct ScriptTaskCatalog {
    pub schema_version: u32,
    pub manifest_path: String,
    pub project_root: String,
    pub project_id: String,
    pub project_version: String,
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
    if raw.schema_version != SCRIPT_TASK_SCHEMA_VERSION {
        return Err(format!(
            "task_manifest_version: supported {}, requested {}",
            SCRIPT_TASK_SCHEMA_VERSION, raw.schema_version
        ));
    }
    validate_identity(&raw.project.id, "task_project_id")?;
    validate_version(&raw.project.version)?;
    if raw.tasks.len() > 256 {
        return Err("task_manifest_tasks: maximum is 256".to_owned());
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
            Ok(task) => catalog_task(&project_root, task, &mut seen),
            Err(error) => degraded_task(
                fallback_id,
                format!("task_manifest_entry: index {index}: {error}"),
            ),
        };
        tasks.push(entry);
    }

    Ok(ScriptTaskCatalog {
        schema_version: SCRIPT_TASK_SCHEMA_VERSION,
        manifest_path: manifest_path.display().to_string(),
        project_root: project_root.display().to_string(),
        project_id: raw.project.id,
        project_version: raw.project.version,
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
    })
}

fn catalog_task(root: &Path, task: RawTask, seen: &mut HashSet<String>) -> ScriptTaskEntry {
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
        .and_then(|_| validate_env(&task.env));

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
    }
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
  "schema_version": 1,
  "project": {"id": "fixture", "version": "1.0.0"},
  "tasks": [
    {"id": "check", "entry": "scripts/check.rhai", "args": ["default"]},
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
        assert_eq!(catalog.tasks[0].status, ScriptTaskStatus::Ready);
        assert_eq!(catalog.tasks[1].status, ScriptTaskStatus::Degraded);
        assert_eq!(catalog.tasks[2].status, ScriptTaskStatus::Degraded);
        assert!(resolve_task(&catalog, "check").is_err());
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
