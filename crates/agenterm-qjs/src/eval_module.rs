//! ES-module execution path for scripts that use top-level `import`/
//! `export` (see `module_sniff::wants_module_mode`), QJS-M5b of
//! `plan/design-qjs-module-imports.md` §4.4. Scripts that don't use
//! import/export keep going through `eval.rs`'s classical-script path
//! unchanged — this module is additive, not a replacement.
//!
//! Uses only rquickjs's safe public API (`Runtime::set_loader`,
//! `Module::declare`/`eval`/`get`, `Promise::finish`) — no new unsafe
//! surface, despite QJS-M2's GC-crash history with the host-binding code.

use std::path::Path;
use std::sync::{Arc, Mutex};

use rquickjs::loader::ScriptLoader;
use rquickjs::{CatchResultExt, Context, Function, Module, Runtime};

use crate::error::QjsError;
use crate::eval::EvalOutcome;
use crate::host::{QjsHostFunctions, inject_host};
use crate::module_resolver::ProjectModuleResolver;

/// Evaluate `source` (already read from `entry_path`, both required so the
/// import resolver has a real project-relative directory to resolve
/// against — see `module_resolver.rs`'s integration-contract note) as an ES
/// module, then call its exported `entry` function.
///
/// `entry_path` and `project_root` are canonicalized here (the caller does
/// not need to get this right themselves — the earlier version of this
/// design flagged "the caller must pass an already-canonical label" as an
/// integration footgun; requiring it here instead removes the footgun
/// rather than just documenting it). `entry_path` must resolve to a real
/// file inside `project_root` — same confinement `module_resolver.rs`
/// applies to every `import`, applied symmetrically to the entry file
/// itself, otherwise a caller could point `entry_path` outside the project
/// and its imports would never be checked against anything meaningful.
pub fn eval_module_entry_with_host(
    source: &str,
    entry_path: &Path,
    project_root: &Path,
    host: &QjsHostFunctions,
) -> Result<EvalOutcome, QjsError> {
    let canonical_root = std::fs::canonicalize(project_root).map_err(|err| {
        QjsError::Usage(format!(
            "qjs_module_root: {}: {err}",
            project_root.display()
        ))
    })?;
    let canonical_entry = std::fs::canonicalize(entry_path).map_err(|err| {
        QjsError::Usage(format!("qjs_module_entry: {}: {err}", entry_path.display()))
    })?;
    if !canonical_entry.starts_with(&canonical_root) {
        return Err(QjsError::Usage(format!(
            "qjs_module_entry_outside_root: {} is not inside {}",
            canonical_entry.display(),
            canonical_root.display()
        )));
    }

    let runtime = Runtime::new().map_err(|err| QjsError::Check(err.to_string()))?;
    runtime.set_loader(
        ProjectModuleResolver::new(canonical_root),
        ScriptLoader::default().with_extension("mjs"),
    );
    let context = Context::full(&runtime).map_err(|err| QjsError::Check(err.to_string()))?;
    let stdout = Arc::new(Mutex::new(String::new()));
    let label = canonical_entry.to_string_lossy().into_owned();

    let value = context.with(|ctx| {
        inject_host(&ctx, host, Arc::clone(&stdout))?;

        let declared = Module::declare(ctx.clone(), label.as_str(), source)
            .catch(&ctx)
            .map_err(|err| QjsError::Parse(err.to_string()))?;

        let (evaluated, promise) = declared.eval().catch(&ctx).map_err(|err| {
            QjsError::Check(format!(
                "{label}: module top-level evaluation failed: {err}"
            ))
        })?;

        // Drains the QuickJS job queue until the module's top-level
        // evaluation promise settles — real ES modules evaluate inside a
        // promise even when nothing in the source actually awaits
        // anything. `Promise::finish` (rquickjs's own convenience, not
        // hand-rolled) surfaces a rejection as a catchable exception, same
        // as every other failure path here.
        promise.finish::<()>().catch(&ctx).map_err(|err| {
            QjsError::Check(format!(
                "{label}: module top-level evaluation failed: {err}"
            ))
        })?;

        let entry: Function = evaluated.get("entry").map_err(|_| {
            QjsError::Check(format!(
                "{label}: no exported `entry` function (module scripts must \
                 `export function entry() {{ ... }}` or `export {{ entry }};`)"
            ))
        })?;

        let result: rquickjs::Value = entry
            .call(())
            .catch(&ctx)
            .map_err(|err| QjsError::Check(format!("{label}: entry() failed: {err}")))?;

        let json_string = ctx
            .json_stringify(result)
            .catch(&ctx)
            .map_err(|err| QjsError::Check(format!("{label}: entry() result: {err}")))?;

        match json_string {
            Some(js_string) => {
                let text = js_string
                    .to_string()
                    .map_err(|err| QjsError::Check(format!("{label}: entry() result: {err}")))?;
                Ok(Some(serde_json::from_str(&text).map_err(|err| {
                    QjsError::Check(format!("{label}: entry() result: {err}"))
                })?))
            }
            None => Ok(None),
        }
    })?;

    let stdout = stdout
        .lock()
        .map_err(|err| QjsError::Check(err.to_string()))?
        .clone();
    Ok(EvalOutcome { value, stdout })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::TempDir;

    #[test]
    fn evaluates_a_single_module_file_with_no_imports() {
        let dir = TempDir::new().expect("tempdir");
        let entry_path = dir.path().join("entry.js");
        std::fs::write(&entry_path, "export function entry() { return 40 + 2; }").unwrap();
        let source = std::fs::read_to_string(&entry_path).unwrap();
        let host = QjsHostFunctions::default();
        let outcome =
            eval_module_entry_with_host(&source, &entry_path, dir.path(), &host).expect("eval");
        assert_eq!(outcome.value, Some(json!(42)));
    }

    #[test]
    fn evaluates_across_a_real_multi_file_import() {
        let dir = TempDir::new().expect("tempdir");
        std::fs::create_dir_all(dir.path().join("lib")).unwrap();
        std::fs::write(dir.path().join("lib/value.js"), "export const value = 42;").unwrap();
        let entry_path = dir.path().join("entry.js");
        std::fs::write(
            &entry_path,
            "import { value } from './lib/value.js';\nexport function entry() { return value; }",
        )
        .unwrap();
        let source = std::fs::read_to_string(&entry_path).unwrap();
        let host = QjsHostFunctions::default();
        let outcome =
            eval_module_entry_with_host(&source, &entry_path, dir.path(), &host).expect("eval");
        assert_eq!(outcome.value, Some(json!(42)));
    }

    #[test]
    fn evaluates_across_a_circular_import() {
        let dir = TempDir::new().expect("tempdir");
        std::fs::write(
            dir.path().join("a.js"),
            "import { bValue } from './b.js';\nexport const aValue = 1;\nexport function useB() { return bValue; }",
        )
        .unwrap();
        let entry_path = dir.path().join("b.js");
        std::fs::write(
            &entry_path,
            "import { aValue } from './a.js';\nexport const bValue = 2;\nexport function entry() { return aValue; }",
        )
        .unwrap();
        let source = std::fs::read_to_string(&entry_path).unwrap();
        let host = QjsHostFunctions::default();
        let outcome =
            eval_module_entry_with_host(&source, &entry_path, dir.path(), &host).expect("eval");
        assert_eq!(outcome.value, Some(json!(1)));
    }

    #[test]
    fn fails_closed_without_an_exported_entry() {
        let dir = TempDir::new().expect("tempdir");
        let entry_path = dir.path().join("entry.js");
        std::fs::write(&entry_path, "export const notEntry = 1;").unwrap();
        let source = std::fs::read_to_string(&entry_path).unwrap();
        let host = QjsHostFunctions::default();
        let error = eval_module_entry_with_host(&source, &entry_path, dir.path(), &host)
            .expect_err("missing entry export");
        assert!(error.to_string().contains("no exported `entry`"), "{error}");
    }

    #[test]
    fn rejects_entry_path_outside_project_root() {
        let dir = TempDir::new().expect("tempdir");
        let outside = TempDir::new().expect("tempdir");
        let entry_path = outside.path().join("entry.js");
        std::fs::write(&entry_path, "export function entry() { return 1; }").unwrap();
        let host = QjsHostFunctions::default();
        let error = eval_module_entry_with_host(
            "export function entry() { return 1; }",
            &entry_path,
            dir.path(),
            &host,
        )
        .expect_err("entry outside project root");
        assert!(
            error.to_string().contains("qjs_module_entry_outside_root"),
            "{error}"
        );
    }

    #[test]
    fn rejects_import_that_escapes_project_root() {
        let outer = TempDir::new().expect("tempdir");
        std::fs::write(outer.path().join("secret.js"), "export const value = 1;").unwrap();
        let inner = outer.path().join("project");
        std::fs::create_dir_all(&inner).unwrap();
        let entry_path = inner.join("entry.js");
        std::fs::write(
            &entry_path,
            "import { value } from '../secret.js';\nexport function entry() { return value; }",
        )
        .unwrap();
        let source = std::fs::read_to_string(&entry_path).unwrap();
        let host = QjsHostFunctions::default();
        let error = eval_module_entry_with_host(&source, &entry_path, &inner, &host)
            .expect_err("escaping import must be rejected");
        assert!(
            error.to_string().contains("Error resolving module"),
            "{error}"
        );
    }

    #[test]
    fn print_and_host_globals_still_work_in_module_scope() {
        let dir = TempDir::new().expect("tempdir");
        let entry_path = dir.path().join("entry.js");
        std::fs::write(
            &entry_path,
            "export function entry() { print('hello from a module'); return 1; }",
        )
        .unwrap();
        let source = std::fs::read_to_string(&entry_path).unwrap();
        let host = QjsHostFunctions::default();
        let outcome =
            eval_module_entry_with_host(&source, &entry_path, dir.path(), &host).expect("eval");
        assert_eq!(outcome.stdout, "hello from a module\n");
    }
}
