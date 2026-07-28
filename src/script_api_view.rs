use std::collections::BTreeMap;

use anyhow::{Result, anyhow};

use crate::script_catalog::{ScriptApiEntry, entries as script_api_entries};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScriptApiStatusFilter {
    All,
    Shipped,
    Planned,
}

impl ScriptApiStatusFilter {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Shipped => "shipped",
            Self::Planned => "planned",
        }
    }

    fn matches(self, status: &str) -> bool {
        self == Self::All || self.as_str() == status
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScriptApiView {
    pub module: Option<String>,
    pub status: ScriptApiStatusFilter,
}

pub fn parse_script_api_view(arguments: &[String]) -> Result<ScriptApiView, String> {
    let mut module = None;
    let mut status = ScriptApiStatusFilter::All;
    let mut status_set = false;
    let mut position = 2;
    while position < arguments.len() {
        match arguments[position].as_str() {
            "--json" => position += 1,
            "--status" => {
                if status_set {
                    return Err(
                        "script_api_status_duplicate: --status may be specified only once"
                            .to_owned(),
                    );
                }
                let Some(value) = arguments.get(position + 1) else {
                    return Err(
                        "script_api_status_missing: --status requires shipped, planned, or all"
                            .to_owned(),
                    );
                };
                status = match value.as_str() {
                    "all" => ScriptApiStatusFilter::All,
                    "shipped" => ScriptApiStatusFilter::Shipped,
                    "planned" => ScriptApiStatusFilter::Planned,
                    _ => {
                        return Err(format!(
                            "script_api_status_invalid: unsupported status '{value}'; \
                             expected shipped, planned, or all"
                        ));
                    }
                };
                status_set = true;
                position += 2;
            }
            value if value.starts_with('-') => {
                return Err(format!("script_api_option_unknown: {value}"));
            }
            value if module.is_none() => {
                module = Some(normalize_script_api_path(value));
                position += 1;
            }
            value => {
                return Err(format!(
                    "script_api_module_duplicate: unexpected second module selector '{value}'"
                ));
            }
        }
    }
    if module.as_deref().is_some_and(str::is_empty) {
        return Err("script_api_module_invalid: module selector cannot be empty".to_owned());
    }
    if let Some(selector) = module.as_deref()
        && !script_api_entries()
            .iter()
            .any(|entry| script_api_entry_matches(entry, selector))
    {
        return Err(format!(
            "script_api_module_unknown: no API module matches '{selector}'"
        ));
    }
    Ok(ScriptApiView { module, status })
}

pub fn filter_script_api_catalog(
    catalog: &mut serde_json::Value,
    view: &ScriptApiView,
) -> Result<()> {
    let object = catalog
        .as_object_mut()
        .ok_or_else(|| anyhow!("catalog root is not an object"))?;
    let entries = object
        .get_mut("entries")
        .and_then(serde_json::Value::as_array_mut)
        .ok_or_else(|| anyhow!("catalog entries are not an array"))?;
    entries.retain(|entry| {
        let status_matches = entry
            .get("status")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|status| view.status.matches(status));
        let module_matches = view.module.as_deref().is_none_or(|selector| {
            ["stable_id", "surface_path", "catalog_path"]
                .iter()
                .any(|field| {
                    entry
                        .get(field)
                        .and_then(serde_json::Value::as_str)
                        .is_some_and(|path| script_api_path_matches(path, selector))
                })
        });
        status_matches && module_matches
    });
    let entry_count = entries.len();
    object.insert(
        "view".to_owned(),
        serde_json::json!({
            "module": view.module,
            "status": view.status.as_str(),
            "entry_count": entry_count,
        }),
    );
    Ok(())
}

pub fn render_script_api_tree(catalog: &serde_json::Value) -> Result<String> {
    let schema_version = catalog
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| anyhow!("catalog schema_version is missing"))?;
    let api_version = catalog
        .get("api_version")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| anyhow!("catalog api_version is missing"))?;
    let view = catalog
        .get("view")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| anyhow!("catalog view is missing"))?;
    let module = view
        .get("module")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("all");
    let status = view
        .get("status")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| anyhow!("catalog view status is missing"))?;
    let entries = catalog
        .get("entries")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| anyhow!("catalog entries are not an array"))?;

    let mut root = ScriptApiTreeNode::default();
    for entry in entries {
        let stable_id = entry
            .get("stable_id")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| anyhow!("catalog entry stable_id is missing"))?;
        let mut node = &mut root;
        for component in stable_id.split('.') {
            node = node.children.entry(component.to_owned()).or_default();
        }
        node.entry = Some(ScriptApiTreeEntry {
            status: entry
                .get("status")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("unknown")
                .to_owned(),
            signature: entry
                .get("signature")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("")
                .to_owned(),
            rust_path: entry
                .get("rust_path")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned),
            rust_mapping: entry
                .get("rust_mapping")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("none")
                .to_owned(),
        });
    }

    let mut lines = vec![format!(
        "AgenTerm Script API v{api_version} · catalog schema {schema_version} · \
         module={module} · status={status} · entries={}",
        entries.len()
    )];
    render_script_api_tree_children(&root, "", &mut lines);
    Ok(lines.join("\n"))
}

fn normalize_script_api_path(value: &str) -> String {
    value
        .trim()
        .replace("::", ".")
        .replace(['/', '\\'], ".")
        .trim_matches('.')
        .to_owned()
}

fn script_api_path_matches(candidate: &str, selector: &str) -> bool {
    let candidate = normalize_script_api_path(candidate);
    candidate == selector
        || candidate
            .strip_prefix(selector)
            .is_some_and(|suffix| suffix.starts_with('.'))
}

fn script_api_entry_matches(entry: &ScriptApiEntry, selector: &str) -> bool {
    script_api_path_matches(entry.stable_id, selector)
        || script_api_path_matches(entry.surface_path, selector)
        || script_api_path_matches(entry.catalog_path, selector)
}

#[derive(Default)]
struct ScriptApiTreeNode {
    children: BTreeMap<String, ScriptApiTreeNode>,
    entry: Option<ScriptApiTreeEntry>,
}

struct ScriptApiTreeEntry {
    status: String,
    signature: String,
    rust_path: Option<String>,
    rust_mapping: String,
}

fn render_script_api_tree_children(
    node: &ScriptApiTreeNode,
    prefix: &str,
    lines: &mut Vec<String>,
) {
    let child_count = node.children.len();
    for (index, (name, child)) in node.children.iter().enumerate() {
        let last = index + 1 == child_count;
        let connector = if last { "└─" } else { "├─" };
        let mut line = format!("{prefix}{connector} {name}");
        if let Some(entry) = child.entry.as_ref() {
            line.push_str(&format!("  [{}] {}", entry.status, entry.signature));
            if let Some(rust_path) = entry.rust_path.as_deref() {
                line.push_str(&format!(" → {rust_path} ({})", entry.rust_mapping));
            }
        }
        lines.push(line);
        let next_prefix = format!("{prefix}{}", if last { "   " } else { "│  " });
        render_script_api_tree_children(child, &next_prefix, lines);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_module_and_status() {
        let arguments = vec![
            "script".to_owned(),
            "api".to_owned(),
            "std::fs".to_owned(),
            "--status".to_owned(),
            "shipped".to_owned(),
            "--json".to_owned(),
        ];
        let view = parse_script_api_view(&arguments).unwrap();
        assert_eq!(view.module.as_deref(), Some("std.fs"));
        assert_eq!(view.status, ScriptApiStatusFilter::Shipped);
    }

    #[test]
    fn rejects_invalid_status_and_unknown_module() {
        let invalid_status = vec![
            "script".to_owned(),
            "api".to_owned(),
            "--status".to_owned(),
            "experimental".to_owned(),
        ];
        assert!(
            parse_script_api_view(&invalid_status)
                .unwrap_err()
                .starts_with("script_api_status_invalid:")
        );

        let unknown_module = vec![
            "script".to_owned(),
            "api".to_owned(),
            "not-a-real-module".to_owned(),
        ];
        assert!(
            parse_script_api_view(&unknown_module)
                .unwrap_err()
                .starts_with("script_api_module_unknown:")
        );

        let duplicate_status = vec![
            "script".to_owned(),
            "api".to_owned(),
            "--status".to_owned(),
            "shipped".to_owned(),
            "--status".to_owned(),
            "planned".to_owned(),
        ];
        assert!(
            parse_script_api_view(&duplicate_status)
                .unwrap_err()
                .starts_with("script_api_status_duplicate:")
        );
    }

    #[test]
    fn catalog_filter_and_tree_are_deterministic() {
        let mut catalog = crate::script_catalog::catalog();
        let view = parse_script_api_view(&[
            "script".to_owned(),
            "api".to_owned(),
            "std/fs".to_owned(),
            "--status".to_owned(),
            "shipped".to_owned(),
        ])
        .unwrap();
        filter_script_api_catalog(&mut catalog, &view).unwrap();
        let entries = catalog["entries"].as_array().unwrap();
        assert!(!entries.is_empty());
        assert!(entries.iter().all(|entry| {
            entry["status"] == "shipped"
                && entry["stable_id"]
                    .as_str()
                    .is_some_and(|id| id.starts_with("std.fs."))
        }));
        assert_eq!(
            catalog["view"]["entry_count"].as_u64(),
            Some(entries.len() as u64)
        );

        let tree = render_script_api_tree(&catalog).unwrap();
        assert!(tree.contains("module=std.fs"));
        assert!(tree.contains("read-to-string  [shipped]"));
        assert!(tree.contains("std::fs::read_to_string"));
        assert_eq!(tree, render_script_api_tree(&catalog).unwrap());
    }
}
