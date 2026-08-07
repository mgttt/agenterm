//! Project-relative Rhai module import validation for check paths.

use std::collections::HashSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

use rhai::Engine;

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
