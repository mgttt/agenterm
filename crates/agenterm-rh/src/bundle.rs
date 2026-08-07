//! Flatten project-relative `import "…" as alias` graphs into one script source.

use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::RhError;
use crate::check::parse_rh_ast;

#[derive(Clone, Debug, PartialEq, Eq)]
struct ImportDecl {
    path: String,
    alias: String,
}

/// Resolve imports under `root` and return a single import-free script that
/// preserves module function names and rewrites `alias::fn` to bare `fn` calls.
pub fn bundle_project_source(root: &Path, source: &str) -> Result<String, RhError> {
    let root = fs::canonicalize(root).map_err(|error| {
        RhError::Compile(format!("script_project_root: {}: {error}", root.display()))
    })?;
    let mut visiting = HashSet::new();
    let mut visited = HashSet::new();
    let mut modules = Vec::new();
    collect_modules(&root, source, &mut visiting, &mut visited, &mut modules)?;
    let mut aliases = BTreeSet::new();
    for (alias, _) in &modules {
        if !aliases.insert(alias.clone()) {
            return Err(RhError::Compile(format!(
                "script_module_alias_conflict: {alias}"
            )));
        }
    }
    let mut pieces = Vec::new();
    let mut seen_fns = BTreeSet::new();
    for (_, module_source) in &modules {
        let stripped = strip_import_statements(module_source)?;
        let rewritten = rewrite_aliased_calls(&stripped, &aliases)?;
        reject_duplicate_fns(&rewritten, &mut seen_fns)?;
        if !rewritten.trim().is_empty() {
            pieces.push(rewritten);
        }
    }
    let entry = strip_import_statements(source)?;
    let entry = rewrite_aliased_calls(&entry, &aliases)?;
    reject_duplicate_fns(&entry, &mut seen_fns)?;
    pieces.push(entry);
    Ok(pieces.join("\n"))
}

fn collect_modules(
    root: &Path,
    source: &str,
    visiting: &mut HashSet<PathBuf>,
    visited: &mut HashSet<PathBuf>,
    modules: &mut Vec<(String, String)>,
) -> Result<(), RhError> {
    for import in literal_import_decls(source).map_err(RhError::Compile)? {
        let path = checked_module_file(root, &import.path).map_err(RhError::Compile)?;
        if visited.contains(&path) {
            continue;
        }
        if !visiting.insert(path.clone()) {
            return Err(RhError::Compile(format!(
                "script_module_cycle: {}",
                import.path
            )));
        }
        let bytes = fs::read(&path).map_err(|error| {
            RhError::Compile(format!("script_module_missing: {}: {error}", import.path))
        })?;
        if bytes.len() > 256 * 1024 {
            return Err(RhError::Compile(format!(
                "script_module_too_large: {} exceeds 262144 bytes",
                import.path
            )));
        }
        let module_source = String::from_utf8(bytes).map_err(|error| {
            RhError::Compile(format!("script_module_encoding: {}: {error}", import.path))
        })?;
        parse_rh_ast(&module_source).map_err(|error| match error {
            RhError::Parse(message) => {
                RhError::Compile(format!("script_module_parse: {}: {message}", import.path))
            }
            other => RhError::Compile(format!("script_module_parse: {}: {other}", import.path)),
        })?;
        collect_modules(root, &module_source, visiting, visited, modules)?;
        modules.push((import.alias, module_source));
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
    let mut rh_candidate = root.join(import);
    rh_candidate.set_extension("rh");
    let mut rhai_candidate = root.join(import);
    rhai_candidate.set_extension("rhai");
    let candidate = if rh_candidate.is_file() {
        rh_candidate
    } else {
        rhai_candidate
    };
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

fn literal_import_decls(source: &str) -> Result<Vec<ImportDecl>, String> {
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
                let path = source[path_start..index].to_owned();
                index += 1;
                skip_script_spacing(bytes, &mut index);
                if !source[index..].starts_with("as") {
                    return Err("script_module_import_alias: import requires `as alias`".to_owned());
                }
                index += 2;
                skip_script_spacing(bytes, &mut index);
                let alias_start = index;
                if index >= bytes.len()
                    || !(bytes[index].is_ascii_alphabetic() || bytes[index] == b'_')
                {
                    return Err("script_module_import_alias: missing import alias".to_owned());
                }
                index += 1;
                while index < bytes.len()
                    && (bytes[index].is_ascii_alphanumeric() || bytes[index] == b'_')
                {
                    index += 1;
                }
                let alias = source[alias_start..index].to_owned();
                imports.push(ImportDecl { path, alias });
            }
            _ => index += 1,
        }
    }
    Ok(imports)
}

fn strip_import_statements(source: &str) -> Result<String, RhError> {
    let bytes = source.as_bytes();
    let mut out = String::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'/' && bytes.get(index + 1) == Some(&b'/') {
            let start = index;
            index += 2;
            while index < bytes.len() && bytes[index] != b'\n' {
                index += 1;
            }
            out.push_str(&source[start..index]);
            continue;
        }
        if bytes[index] == b'/' && bytes.get(index + 1) == Some(&b'*') {
            let start = index;
            index += 2;
            while index + 1 < bytes.len() && !(bytes[index] == b'*' && bytes[index + 1] == b'/') {
                index += 1;
            }
            index = (index + 2).min(bytes.len());
            out.push_str(&source[start..index]);
            continue;
        }
        if matches!(bytes[index], b'"' | b'`') {
            let start = index;
            skip_script_string(bytes, &mut index).map_err(RhError::Compile)?;
            out.push_str(&source[start..index]);
            continue;
        }
        if bytes[index].is_ascii_alphabetic() || bytes[index] == b'_' {
            let start = index;
            index += 1;
            while index < bytes.len()
                && (bytes[index].is_ascii_alphanumeric() || bytes[index] == b'_')
            {
                index += 1;
            }
            if &source[start..index] == "import" {
                while index < bytes.len() && bytes[index] != b'\n' && bytes[index] != b';' {
                    index += 1;
                }
                if index < bytes.len() && bytes[index] == b';' {
                    index += 1;
                }
                if index < bytes.len() && bytes[index] == b'\n' {
                    index += 1;
                }
                continue;
            }
            out.push_str(&source[start..index]);
            continue;
        }
        out.push(bytes[index] as char);
        index += 1;
    }
    Ok(out)
}

fn rewrite_aliased_calls(source: &str, aliases: &BTreeSet<String>) -> Result<String, RhError> {
    if aliases.is_empty() {
        return Ok(source.to_owned());
    }
    let bytes = source.as_bytes();
    let mut out = String::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'/' && bytes.get(index + 1) == Some(&b'/') {
            let start = index;
            index += 2;
            while index < bytes.len() && bytes[index] != b'\n' {
                index += 1;
            }
            out.push_str(&source[start..index]);
            continue;
        }
        if bytes[index] == b'/' && bytes.get(index + 1) == Some(&b'*') {
            let start = index;
            index += 2;
            while index + 1 < bytes.len() && !(bytes[index] == b'*' && bytes[index + 1] == b'/') {
                index += 1;
            }
            index = (index + 2).min(bytes.len());
            out.push_str(&source[start..index]);
            continue;
        }
        if matches!(bytes[index], b'"' | b'`') {
            let start = index;
            skip_script_string(bytes, &mut index).map_err(RhError::Compile)?;
            out.push_str(&source[start..index]);
            continue;
        }
        if bytes[index].is_ascii_alphabetic() || bytes[index] == b'_' {
            let start = index;
            index += 1;
            while index < bytes.len()
                && (bytes[index].is_ascii_alphanumeric() || bytes[index] == b'_')
            {
                index += 1;
            }
            let ident = &source[start..index];
            if aliases.contains(ident)
                && source[index..].starts_with("::")
                && source
                    .as_bytes()
                    .get(index + 2)
                    .is_some_and(|byte| byte.is_ascii_alphabetic() || *byte == b'_')
            {
                index += 2;
                // Drop `alias::` and keep the function name.
                continue;
            }
            out.push_str(ident);
            continue;
        }
        out.push(bytes[index] as char);
        index += 1;
    }
    Ok(out)
}

fn reject_duplicate_fns(source: &str, seen: &mut BTreeSet<String>) -> Result<(), RhError> {
    let ast = parse_rh_ast(source)?;
    for meta in ast.iter_functions() {
        if meta.name == "entry" || meta.name == "cc_lines" {
            continue;
        }
        if !seen.insert(meta.name.to_string()) {
            return Err(RhError::Compile(format!(
                "script_module_fn_conflict: {}",
                meta.name
            )));
        }
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::bundle_project_source;
    use std::fs;
    use std::path::PathBuf;

    #[test]
    fn bundles_single_hop_import_into_bare_calls() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let source = r#"
import "fixtures/rh/modules/import_helper" as helper;
fn entry() { helper::add(40, 2) }
"#;
        let bundled = bundle_project_source(&root, source).expect("bundle");
        assert!(bundled.contains("fn add("), "{bundled}");
        assert!(bundled.contains("add(40, 2)"), "{bundled}");
        assert!(!bundled.contains("import "), "{bundled}");
        assert!(!bundled.contains("helper::"), "{bundled}");
        let _ = fs::metadata(root.join("fixtures/rh/modules/import_helper.rhai"));
    }
}
