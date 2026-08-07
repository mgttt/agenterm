//! Project-relative Rhai module import validation for check paths.

use std::collections::HashSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::RhError;
use crate::check::parse_rh_ast;

pub fn validate_project_imports(root: &Path, source: &str) -> Result<Vec<String>, String> {
    let root = fs::canonicalize(root)
        .map_err(|error| format!("script_project_root: {}: {error}", root.display()))?;
    let mut visiting = HashSet::new();
    let mut visited = HashSet::new();
    let mut module_sources = Vec::new();
    validate_import_tree(
        &root,
        source,
        &mut visiting,
        &mut visited,
        &mut module_sources,
    )?;
    Ok(module_sources)
}

fn validate_import_tree(
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
        parse_rh_ast(&module_source).map_err(|error| match error {
            RhError::Parse(message) => format!("script_module_parse: {import}: {message}"),
            error => format!("script_module_parse: {import}: {error}"),
        })?;
        validate_import_tree(root, &module_source, visiting, visited, module_sources)?;
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
    // Prefer native `.rh` modules; fall back to archived `.rhai` while callers migrate.
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

#[cfg(test)]
mod tests {
    use super::validate_project_imports;
    use std::fs;
    use std::path::PathBuf;

    fn fixture() -> PathBuf {
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
            "agenterm-rh-project-import-{}-{test_name}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("modules")).unwrap();
        root
    }

    #[test]
    fn validates_nested_imports_in_dependency_order() {
        let root = fixture();
        fs::write(
            root.join("modules/middle.rhai"),
            "import \"modules/leaf\" as leaf;\nexport const middle = leaf::value;",
        )
        .unwrap();
        fs::write(root.join("modules/leaf.rhai"), "export const value = 42;").unwrap();

        let sources =
            validate_project_imports(&root, "import \"modules/middle\" as middle;").unwrap();
        assert_eq!(
            sources,
            [
                "export const value = 42;",
                "import \"modules/leaf\" as leaf;\nexport const middle = leaf::value;",
            ]
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn prefers_rh_modules_over_rhai() {
        let root = fixture();
        fs::write(root.join("modules/leaf.rhai"), "fn value() { 1 }").unwrap();
        fs::write(root.join("modules/leaf.rh"), "fn value() { 2 }").unwrap();
        let sources =
            validate_project_imports(&root, "import \"modules/leaf\" as leaf;").unwrap();
        assert_eq!(sources, ["fn value() { 2 }"]);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_missing_import() {
        let root = fixture();
        let error = validate_project_imports(&root, "import \"missing\" as missing;").unwrap_err();
        assert!(error.starts_with("script_module_missing: missing:"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_import_cycle() {
        let root = fixture();
        fs::write(root.join("modules/a.rhai"), "import \"modules/b\" as b;").unwrap();
        fs::write(root.join("modules/b.rhai"), "import \"modules/a\" as a;").unwrap();

        let error = validate_project_imports(&root, "import \"modules/a\" as a;").unwrap_err();
        assert_eq!(error, "script_module_cycle: modules/a");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_import_root_escape() {
        let root = fixture();
        let error =
            validate_project_imports(&root, "import \"../outside\" as outside;").unwrap_err();
        assert_eq!(error, "script_module_root_escape: ../outside");
        fs::remove_dir_all(root).unwrap();
    }
}
