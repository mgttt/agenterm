//! Flatten project-relative `import "…" as alias` graphs into one script source.
//!
//! Module functions are renamed to `{alias}__{name}` so two libraries may both
//! define `require` / `new_context` without `script_module_fn_conflict` after
//! flatten. Calls `alias::name` rewrite to `alias__name`; bare calls inside a
//! module rewrite to that module's prefixed name.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::RhError;
use crate::check::parse_rh_ast;

#[derive(Clone, Debug, PartialEq, Eq)]
struct ImportDecl {
    path: String,
    alias: String,
}

/// Resolve imports under `root` and return a single import-free script.
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

    let mut alias_fns: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (alias, module_source) in &modules {
        let stripped = strip_import_statements(module_source)?;
        alias_fns.insert(alias.clone(), collect_fn_names(&stripped)?);
    }

    let mut pieces = Vec::new();
    let mut seen_fns = BTreeSet::new();
    for (alias, module_source) in &modules {
        let stripped = strip_import_statements(module_source)?;
        let local = alias_fns
            .get(alias)
            .cloned()
            .unwrap_or_default();
        let prefixed = prefix_local_fns(&stripped, alias, &local)?;
        let rewritten = rewrite_aliased_calls(&prefixed, &alias_fns)?;
        reject_duplicate_fns(&rewritten, &mut seen_fns)?;
        if !rewritten.trim().is_empty() {
            pieces.push(rewritten);
        }
    }
    let entry = strip_import_statements(source)?;
    let entry = rewrite_aliased_calls(&entry, &alias_fns)?;
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
        return Err(format!("script_module_missing: {import}"));
    }
    Ok(canonical)
}

fn literal_import_decls(source: &str) -> Result<Vec<ImportDecl>, String> {
    let bytes = source.as_bytes();
    let mut imports = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'/' && bytes.get(index + 1) == Some(&b'/') {
            index += 2;
            while index < bytes.len() && bytes[index] != b'\n' {
                index += 1;
            }
            continue;
        }
        if bytes[index] == b'/' && bytes.get(index + 1) == Some(&b'*') {
            index += 2;
            while index + 1 < bytes.len() && !(bytes[index] == b'*' && bytes[index + 1] == b'/') {
                index += 1;
            }
            index = (index + 2).min(bytes.len());
            continue;
        }
        if matches!(bytes[index], b'"' | b'`') {
            skip_script_string(bytes, &mut index)?;
            continue;
        }
        if source[index..].starts_with("import")
            && bytes
                .get(index + "import".len())
                .is_none_or(|byte| !byte.is_ascii_alphanumeric() && *byte != b'_')
        {
            index += "import".len();
            skip_script_spacing(bytes, &mut index);
            if bytes.get(index) != Some(&b'"') && bytes.get(index) != Some(&b'`') {
                return Err("script_module_import_path: expected string path".to_owned());
            }
            let quote = bytes[index];
            index += 1;
            let path_start = index;
            while index < bytes.len() && bytes[index] != quote {
                if bytes[index] == b'\\' {
                    index = (index + 2).min(bytes.len());
                } else {
                    index += 1;
                }
            }
            if index >= bytes.len() {
                return Err("script_module_import_path: unterminated path".to_owned());
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
            skip_script_spacing(bytes, &mut index);
            if bytes.get(index) == Some(&b';') {
                index += 1;
            }
            continue;
        }
        index += 1;
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
        if source[index..].starts_with("import")
            && bytes
                .get(index + "import".len())
                .is_none_or(|byte| !byte.is_ascii_alphanumeric() && *byte != b'_')
        {
            index += "import".len();
            while index < bytes.len() && bytes[index] != b'\n' && bytes[index] != b';' {
                if matches!(bytes[index], b'"' | b'`') {
                    skip_script_string(bytes, &mut index).map_err(RhError::Compile)?;
                } else {
                    index += 1;
                }
            }
            if bytes.get(index) == Some(&b';') {
                index += 1;
            }
            if bytes.get(index) == Some(&b'\n') {
                index += 1;
            }
            continue;
        }
        out.push(bytes[index] as char);
        index += 1;
    }
    Ok(out)
}

fn collect_fn_names(source: &str) -> Result<BTreeSet<String>, RhError> {
    let ast = parse_rh_ast(source)?;
    let mut names = BTreeSet::new();
    for meta in ast.iter_functions() {
        if meta.name == "entry" || meta.name == "cc_lines" {
            continue;
        }
        names.insert(meta.name.to_string());
    }
    Ok(names)
}

fn mangled_name(alias: &str, name: &str) -> String {
    format!("{alias}__{name}")
}

/// Prefix top-level `fn name` definitions and bare `name(` calls that refer to
/// this module's functions.
fn prefix_local_fns(
    source: &str,
    alias: &str,
    local_fns: &BTreeSet<String>,
) -> Result<String, RhError> {
    if local_fns.is_empty() {
        return Ok(source.to_owned());
    }
    let bytes = source.as_bytes();
    let mut out = String::new();
    let mut index = 0;
    let mut previous_identifier = None::<String>;
    while index < bytes.len() {
        if bytes[index] == b'/' && bytes.get(index + 1) == Some(&b'/') {
            let start = index;
            index += 2;
            while index < bytes.len() && bytes[index] != b'\n' {
                index += 1;
            }
            out.push_str(&source[start..index]);
            previous_identifier = None;
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
            previous_identifier = None;
            continue;
        }
        if matches!(bytes[index], b'"' | b'`') {
            let start = index;
            skip_script_string(bytes, &mut index).map_err(RhError::Compile)?;
            out.push_str(&source[start..index]);
            previous_identifier = None;
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
            let mut next = index;
            while next < bytes.len() && bytes[next].is_ascii_whitespace() {
                next += 1;
            }
            let is_def = previous_identifier.as_deref() == Some("fn");
            let is_call = bytes.get(next) == Some(&b'(');
            let mut previous = start;
            while previous > 0 && bytes[previous - 1].is_ascii_whitespace() {
                previous -= 1;
            }
            let is_method_or_qualified =
                previous > 0 && matches!(bytes[previous - 1], b'.' | b':');
            if local_fns.contains(ident) && (is_def || (is_call && !is_method_or_qualified)) {
                out.push_str(&mangled_name(alias, ident));
            } else {
                out.push_str(ident);
            }
            previous_identifier = Some(ident.to_owned());
            continue;
        }
        if !bytes[index].is_ascii_whitespace() {
            previous_identifier = None;
        }
        out.push(bytes[index] as char);
        index += 1;
    }
    Ok(out)
}

/// Rewrite `alias::name` to `alias__name` using the module function inventory.
fn rewrite_aliased_calls(
    source: &str,
    alias_fns: &BTreeMap<String, BTreeSet<String>>,
) -> Result<String, RhError> {
    if alias_fns.is_empty() {
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
            if alias_fns.contains_key(ident)
                && source[index..].starts_with("::")
                && source
                    .as_bytes()
                    .get(index + 2)
                    .is_some_and(|byte| byte.is_ascii_alphabetic() || *byte == b'_')
            {
                index += 2;
                let name_start = index;
                index += 1;
                while index < bytes.len()
                    && (bytes[index].is_ascii_alphanumeric() || bytes[index] == b'_')
                {
                    index += 1;
                }
                let name = &source[name_start..index];
                if alias_fns
                    .get(ident)
                    .is_some_and(|fns| fns.contains(name))
                {
                    out.push_str(&mangled_name(ident, name));
                } else {
                    // Keep unresolved qualified call visible for later errors.
                    out.push_str(ident);
                    out.push_str("::");
                    out.push_str(name);
                }
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
    fn bundles_single_hop_import_into_prefixed_calls() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let source = r#"
import "fixtures/rh/modules/import_helper" as helper;
fn entry() { helper::add(40, 2) }
"#;
        let bundled = bundle_project_source(&root, source).expect("bundle");
        assert!(bundled.contains("fn helper__add("), "{bundled}");
        assert!(bundled.contains("helper__add(40, 2)"), "{bundled}");
        assert!(!bundled.contains("import "), "{bundled}");
        assert!(!bundled.contains("helper::"), "{bundled}");
        let _ = fs::metadata(root.join("fixtures/rh/modules/import_helper.rh"));
    }

    #[test]
    fn prefixes_collide_require_across_two_modules() {
        let dir = tempfile::tempdir().expect("temp");
        let root = dir.path();
        fs::create_dir_all(root.join("lib")).expect("lib");
        fs::write(
            root.join("lib/a.rh"),
            "fn require(c, m) { if c == 0 { throw m } else { 0 } }\n",
        )
        .expect("a");
        fs::write(
            root.join("lib/b.rh"),
            "fn require(c, m) { if c == 0 { throw m } else { 1 } }\n",
        )
        .expect("b");
        let source = r#"
import "lib/a" as a;
import "lib/b" as b;
fn entry() { a::require(1, `x`); b::require(1, `y`); 0 }
"#;
        let bundled = bundle_project_source(root, source).expect("bundle");
        assert!(bundled.contains("fn a__require("), "{bundled}");
        assert!(bundled.contains("fn b__require("), "{bundled}");
        assert!(bundled.contains("a__require(1, `x`)"), "{bundled}");
        assert!(bundled.contains("b__require(1, `y`)"), "{bundled}");
    }
}
