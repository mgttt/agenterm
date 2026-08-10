use std::collections::HashSet;

use crate::RhError;

const LIST_MARKER: &str = "\"--list-evidence\"";
const DECLARATION_FN: &str = "fn declared_evidence_ids()";

/// Read the literal evidence declarations exposed by a task's
/// `--list-evidence` branch without compiling or executing the task.
pub fn static_evidence_declarations(source: &str) -> Result<Vec<String>, RhError> {
    let marker = source
        .rfind(LIST_MARKER)
        .ok_or_else(|| RhError::Parse("missing --list-evidence branch".into()))?;
    let branch = &source[marker + LIST_MARKER.len()..];
    let end = branch
        .find("return")
        .ok_or_else(|| RhError::Parse("--list-evidence branch must return".into()))?;
    let branch = &branch[..end];

    let mut values = direct_print_literals(branch)?;
    if values.is_empty() && branch.contains("declared_evidence_ids()") {
        values = declared_evidence_function_literals(source)?;
    }
    if values.is_empty() {
        return Err(RhError::Parse(
            "--list-evidence branch contains no static declarations".into(),
        ));
    }
    reject_duplicates(&values)?;
    Ok(values)
}

fn direct_print_literals(branch: &str) -> Result<Vec<String>, RhError> {
    let mut values = Vec::new();
    for line in branch.lines().map(str::trim) {
        if !line.starts_with("print(") {
            continue;
        }
        let Some(value) = quoted_call_argument(line, "print(", ");") else {
            if line == "print(id);" {
                continue;
            }
            return Err(RhError::Parse(format!(
                "--list-evidence print must contain one string literal: {line}"
            )));
        };
        values.push(value);
    }
    Ok(values)
}

fn declared_evidence_function_literals(source: &str) -> Result<Vec<String>, RhError> {
    let start = source
        .find(DECLARATION_FN)
        .ok_or_else(|| RhError::Parse("missing declared_evidence_ids function".into()))?;
    let body = &source[start + DECLARATION_FN.len()..];
    let end = body
        .find("\n}")
        .ok_or_else(|| RhError::Parse("unterminated declared_evidence_ids function".into()))?;
    let body = &body[..end];
    let mut values = Vec::new();
    for line in body.lines().map(str::trim) {
        if let Some(rest) = line.strip_prefix("let ids = [") {
            let value = rest
                .strip_suffix("];")
                .and_then(quoted_literal)
                .ok_or_else(|| {
                    RhError::Parse("declared_evidence_ids seed must be one literal".into())
                })?;
            values.push(value);
        } else if line.starts_with("ids.push(") {
            let value = quoted_call_argument(line, "ids.push(", ");").ok_or_else(|| {
                RhError::Parse("declared_evidence_ids push must contain one literal".into())
            })?;
            values.push(value);
        }
    }
    Ok(values)
}

fn quoted_call_argument(line: &str, prefix: &str, suffix: &str) -> Option<String> {
    line.strip_prefix(prefix)
        .and_then(|value| value.strip_suffix(suffix))
        .and_then(quoted_literal)
}

fn quoted_literal(value: &str) -> Option<String> {
    let value = value.trim();
    let inner = value.strip_prefix('"')?.strip_suffix('"')?;
    if inner.contains(['"', '\\', '\n', '\r']) {
        return None;
    }
    Some(inner.to_owned())
}

fn reject_duplicates(values: &[String]) -> Result<(), RhError> {
    let mut unique = HashSet::new();
    for value in values {
        if !unique.insert(value) {
            return Err(RhError::Parse(format!(
                "duplicate --list-evidence declaration: {value}"
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::static_evidence_declarations;

    #[test]
    fn reads_direct_literal_prints_without_running_the_script() {
        let source = r#"
fn entry() {
    if args.len == 1 && args[0] == "--list-evidence" {
        print("one.alpha");
        print("two.beta");
        return 0;
    }
    destructive_operation();
}
"#;
        assert_eq!(
            static_evidence_declarations(source).unwrap(),
            ["one.alpha", "two.beta"]
        );
    }

    #[test]
    fn reads_the_shared_declaration_function_shape() {
        let source = r#"
fn declared_evidence_ids() {
    let ids = ["one.alpha"];
    ids.push("two.beta");
    ids
}
fn entry() {
    if args.len == 1 && args[0] == "--list-evidence" {
        for id in declared_evidence_ids() { print(id); }
        return 0;
    }
}
"#;
        assert_eq!(
            static_evidence_declarations(source).unwrap(),
            ["one.alpha", "two.beta"]
        );
    }

    #[test]
    fn rejects_dynamic_or_duplicate_declarations() {
        let dynamic = r#"if args[0] == "--list-evidence" { print(value); return 0; }"#;
        assert!(static_evidence_declarations(dynamic).is_err());
        let duplicate = r#"
if args[0] == "--list-evidence" {
    print("one.alpha");
    print("one.alpha");
    return 0;
}
"#;
        assert!(static_evidence_declarations(duplicate).is_err());
    }
}
