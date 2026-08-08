//! Project-root-confined ES module resolver, QJS-M5a of
//! `plan/design-qjs-module-imports.md` §4.2.
//!
//! `rquickjs::loader::FileResolver` does **not** confine resolution to a
//! root directory — verified by reading its source
//! (`rquickjs-core-0.12.2/src/loader/file_resolver.rs`):
//! `RelativePath::join_normalized` lexically normalizes `..` segments but
//! never clamps the result to a boundary, so a naive
//! `FileResolver::default().with_path(project_root)` would let
//! `import "../../../../etc/x.js"` resolve (and then load-and-execute)
//! outside the project tree. This module is the actual security boundary;
//! `rquickjs::loader::ScriptLoader` is still used for the file *read*
//! (nothing wrong with its behavior once given an already-confined path).
//!
//! Confinement logic mirrors `agenterm_rh::project_import::checked_module_file`
//! (reject absolute/traversal-looking specifiers, canonicalize, verify
//! `starts_with(root)`) — same security posture, different mechanism
//! (rh resolves specifiers relative to *project root*; qjs resolves
//! relative to the *importing file's directory*, the ES-module-idiomatic
//! convention `./foo.js`/`../lib/bar.js` scripts will actually write).
//!
//! **Integration contract (for the caller wiring this into `Module::declare`,
//! see `plan/design-qjs-module-imports.md` QJS-M5b)**: the top-level entry
//! module MUST be declared under its canonical absolute path as the
//! `name` passed to `Module::declare`, not a bare/relative label — this
//! resolver computes each import's `base_dir` from `Path::new(base).parent()`,
//! which only means "the importing file's real directory" if `base` itself
//! is a real, absolute path. Getting this wrong doesn't fail loudly (a
//! relative label just resolves against the process's current directory
//! instead of the entry file's actual directory), which is exactly the
//! kind of silent-wrong-answer bug worth calling out here, not leaving
//! implicit.
//!
//! Deliberately unsupported: bare/"bare package" specifiers (`import x
//! from "some-package"`, no `./`/`../` prefix) — there is no
//! node_modules-style resolution here, and the design doc doesn't call
//! for one. Only relative specifiers resolve; everything else is a clear,
//! typed rejection, not a silent no-op.

use std::path::{Path, PathBuf};

use rquickjs::loader::{ImportAttributes, Resolver};
use rquickjs::{Ctx, Error as JsError};

const MAX_SPECIFIER_LEN: usize = 4096;

/// Resolves ES module specifiers relative to the importing file's
/// directory, confined to never escape `root`.
#[derive(Debug, Clone)]
pub struct ProjectModuleResolver {
    root: PathBuf,
}

impl ProjectModuleResolver {
    /// `root` need not exist yet at construction time (it's canonicalized
    /// per-resolve, not cached — a moved/deleted project root between
    /// resolves will simply fail the next resolve, not silently reuse a
    /// stale canonical path).
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }
}

impl Resolver for ProjectModuleResolver {
    fn resolve<'js>(
        &mut self,
        _ctx: &Ctx<'js>,
        base: &str,
        name: &str,
        _attributes: Option<ImportAttributes<'js>>,
    ) -> rquickjs::Result<String> {
        resolve_confined(&self.root, base, name)
            .map(|path| path.to_string_lossy().into_owned())
            .map_err(|_reason| JsError::new_resolving(base, name))
    }
}

/// Pure resolution logic, kept separate from the `Resolver` impl so it can
/// be unit-tested directly with a real error message (the `Resolver` trait
/// only lets us return `rquickjs::Error::new_resolving`, which doesn't
/// carry our specific rejection reason — see `resolve`'s `map_err`).
fn resolve_confined(root: &Path, base: &str, name: &str) -> Result<PathBuf, String> {
    if name.is_empty() || name.len() > MAX_SPECIFIER_LEN {
        return Err(format!("qjs_module_invalid_specifier: {name}"));
    }
    if !(name.starts_with("./") || name.starts_with("../")) {
        return Err(format!(
            "qjs_module_unsupported_specifier: {name} (only relative ./ or ../ \
             specifiers are supported; no bare-package resolution)"
        ));
    }

    let base_dir = Path::new(base).parent().unwrap_or_else(|| Path::new(""));
    let joined = base_dir.join(name);

    let canonical = std::fs::canonicalize(&joined)
        .map_err(|error| format!("qjs_module_missing: {name}: {error}"))?;
    let canonical_root = std::fs::canonicalize(root)
        .map_err(|error| format!("qjs_module_root: {}: {error}", root.display()))?;

    if !canonical.starts_with(&canonical_root) {
        return Err(format!("qjs_module_root_escape: {name}"));
    }
    if !canonical.is_file() {
        return Err(format!("qjs_module_missing: {name} is not a file"));
    }
    Ok(canonical)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn fixture() -> TempDir {
        TempDir::new().expect("tempdir")
    }

    #[test]
    fn resolves_a_valid_sibling_import() {
        let dir = fixture();
        std::fs::write(dir.path().join("entry.js"), "// entry").unwrap();
        std::fs::write(dir.path().join("leaf.js"), "// leaf").unwrap();
        let entry = dir.path().join("entry.js");
        let resolved =
            resolve_confined(dir.path(), &entry.to_string_lossy(), "./leaf.js").expect("resolve");
        assert_eq!(
            resolved,
            std::fs::canonicalize(dir.path().join("leaf.js")).unwrap()
        );
    }

    #[test]
    fn resolves_a_valid_nested_import() {
        let dir = fixture();
        std::fs::create_dir_all(dir.path().join("lib")).unwrap();
        std::fs::write(dir.path().join("entry.js"), "// entry").unwrap();
        std::fs::write(dir.path().join("lib/leaf.js"), "// leaf").unwrap();
        let entry = dir.path().join("entry.js");
        let resolved = resolve_confined(dir.path(), &entry.to_string_lossy(), "./lib/leaf.js")
            .expect("resolve");
        assert_eq!(
            resolved,
            std::fs::canonicalize(dir.path().join("lib/leaf.js")).unwrap()
        );
    }

    #[test]
    fn resolves_a_parent_relative_import_that_stays_inside_root() {
        let dir = fixture();
        std::fs::create_dir_all(dir.path().join("lib")).unwrap();
        std::fs::write(dir.path().join("lib/entry.js"), "// entry").unwrap();
        std::fs::write(dir.path().join("shared.js"), "// shared").unwrap();
        let entry = dir.path().join("lib/entry.js");
        let resolved =
            resolve_confined(dir.path(), &entry.to_string_lossy(), "../shared.js").expect("ok");
        assert_eq!(
            resolved,
            std::fs::canonicalize(dir.path().join("shared.js")).unwrap()
        );
    }

    #[test]
    fn rejects_escape_above_project_root() {
        let dir = fixture();
        std::fs::write(dir.path().join("entry.js"), "// entry").unwrap();
        let entry = dir.path().join("entry.js");
        let error = resolve_confined(dir.path(), &entry.to_string_lossy(), "../../outside.js")
            .expect_err("must reject root escape");
        assert!(
            error.starts_with("qjs_module_missing") || error.starts_with("qjs_module_root_escape"),
            "{error}"
        );
    }

    #[test]
    fn rejects_escape_that_resolves_to_a_real_file_outside_root() {
        // Unlike the previous test (target doesn't exist, so canonicalize
        // itself fails), this one plants a real file just outside root to
        // prove the containment check — not "file happens to be missing"
        // — is what's actually rejecting it.
        let outer = fixture();
        std::fs::write(outer.path().join("secret.js"), "// secret").unwrap();
        let inner = outer.path().join("project");
        std::fs::create_dir_all(&inner).unwrap();
        std::fs::write(inner.join("entry.js"), "// entry").unwrap();
        let entry = inner.join("entry.js");
        let error = resolve_confined(&inner, &entry.to_string_lossy(), "../secret.js")
            .expect_err("must reject root escape to a real file");
        assert_eq!(error, "qjs_module_root_escape: ../secret.js");
    }

    #[test]
    fn rejects_absolute_specifier() {
        let dir = fixture();
        std::fs::write(dir.path().join("entry.js"), "// entry").unwrap();
        let entry = dir.path().join("entry.js");
        let error = resolve_confined(dir.path(), &entry.to_string_lossy(), "/etc/passwd")
            .expect_err("must reject absolute specifier");
        assert!(error.starts_with("qjs_module_unsupported_specifier"), "{error}");
    }

    #[test]
    fn rejects_bare_specifier() {
        let dir = fixture();
        std::fs::write(dir.path().join("entry.js"), "// entry").unwrap();
        let entry = dir.path().join("entry.js");
        let error = resolve_confined(dir.path(), &entry.to_string_lossy(), "lodash")
            .expect_err("must reject bare specifier");
        assert!(error.starts_with("qjs_module_unsupported_specifier"), "{error}");
    }

    #[test]
    fn rejects_missing_file() {
        let dir = fixture();
        std::fs::write(dir.path().join("entry.js"), "// entry").unwrap();
        let entry = dir.path().join("entry.js");
        let error = resolve_confined(dir.path(), &entry.to_string_lossy(), "./missing.js")
            .expect_err("must reject missing file");
        assert!(error.starts_with("qjs_module_missing"), "{error}");
    }

    #[test]
    fn rejects_empty_specifier() {
        let dir = fixture();
        let entry = dir.path().join("entry.js");
        let error = resolve_confined(dir.path(), &entry.to_string_lossy(), "")
            .expect_err("must reject empty specifier");
        assert!(error.starts_with("qjs_module_invalid_specifier"), "{error}");
    }

    // ── wired into a real Runtime/Context, not just the pure function ──
    //
    // The tests above prove `resolve_confined` behaves correctly in
    // isolation. These prove the same claims hold once this resolver is
    // actually registered via `Runtime::set_loader` and driven by
    // `Module::declare` — the same call `check()` will use once M5c wires
    // this in — because a correct pure function wired incorrectly (wrong
    // loader pairing, wrong extension config, etc.) would still produce a
    // broken feature.

    use rquickjs::CatchResultExt;

    fn runtime_with_resolver(root: &Path) -> (rquickjs::Runtime, rquickjs::Context) {
        let runtime = rquickjs::Runtime::new().expect("runtime");
        runtime.set_loader(
            ProjectModuleResolver::new(root),
            rquickjs::loader::ScriptLoader::default().with_extension("mjs"),
        );
        let context = rquickjs::Context::full(&runtime).expect("context");
        (runtime, context)
    }

    #[test]
    fn declare_resolves_and_links_a_real_sibling_import() {
        let dir = fixture();
        std::fs::write(dir.path().join("leaf.js"), "export const value = 42;").unwrap();
        let entry_path = dir.path().join("entry.js");
        std::fs::write(
            &entry_path,
            "import { value } from './leaf.js';\nexport function entry() { return value; }",
        )
        .unwrap();

        let (_runtime, context) = runtime_with_resolver(dir.path());
        let source = std::fs::read_to_string(&entry_path).unwrap();
        let label = entry_path.to_string_lossy().into_owned();
        context.with(|ctx| {
            rquickjs::Module::declare(ctx.clone(), label, source)
                .expect("declare must resolve+link the sibling import");
        });
    }

    #[test]
    fn declare_rejects_an_import_that_escapes_the_real_project_root() {
        let outer = fixture();
        std::fs::write(outer.path().join("secret.js"), "export const value = 1;").unwrap();
        let inner = outer.path().join("project");
        std::fs::create_dir_all(&inner).unwrap();
        let entry_path = inner.join("entry.js");
        std::fs::write(
            &entry_path,
            "import { value } from '../secret.js';\nexport function entry() { return value; }",
        )
        .unwrap();

        let (_runtime, context) = runtime_with_resolver(&inner);
        let source = std::fs::read_to_string(&entry_path).unwrap();
        let label = entry_path.to_string_lossy().into_owned();
        context.with(|ctx| {
            // `.catch(&ctx)` (same pattern `check.rs` uses, for the same
            // reason): a bare `Result` here just says "Exception generated
            // by QuickJS" — useless for asserting anything specific. First
            // attempt at this test asserted on that generic message
            // directly and failed for the wrong reason (my assumption
            // about the message shape was wrong, not the resolver).
            let error = rquickjs::Module::declare(ctx.clone(), label, source)
                .catch(&ctx)
                .expect_err("declare must reject the escaping import");
            // Actual message (checked, not assumed, after the first
            // assertion attempt above also guessed wrong): "Error
            // resolving module '../secret.js' from '<entry path>'".
            let message = error.to_string();
            assert!(message.contains("Error resolving module"), "{message}");
            assert!(message.contains("../secret.js"), "{message}");
        });
    }

    #[test]
    fn declare_handles_a_circular_import_without_hanging_or_crashing() {
        // Empirically verifies the design doc's claim
        // (plan/design-qjs-module-imports.md §5: "ES modules handle
        // circular imports natively") against THIS engine, not just
        // general JS-spec knowledge — the claim was written before this
        // was ever run.
        let dir = fixture();
        std::fs::write(
            dir.path().join("a.js"),
            "import { b } from './b.js';\nexport const a = 1;\nexport function useB() { return b; }",
        )
        .unwrap();
        let entry_path = dir.path().join("b.js");
        std::fs::write(
            &entry_path,
            "import { a } from './a.js';\nexport const b = 2;\nexport function entry() { return a; }",
        )
        .unwrap();

        let (_runtime, context) = runtime_with_resolver(dir.path());
        let source = std::fs::read_to_string(&entry_path).unwrap();
        let label = entry_path.to_string_lossy().into_owned();
        context.with(|ctx| {
            rquickjs::Module::declare(ctx.clone(), label, source)
                .expect("declare must handle the a<->b cycle, not hang or error");
        });
    }
}
