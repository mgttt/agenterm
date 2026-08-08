//! Top-level `import`/`export` detection — the routing decision from
//! `plan/design-qjs-module-imports.md` §4.1, not a parser.
//!
//! This decides whether a script goes through the module execution path
//! (`module_resolver.rs` + real ES module semantics) or the existing
//! classical-script path (`eval.rs`'s `entry()`-on-`globalThis`
//! convention, unchanged). A false positive just means QuickJS's own
//! module parser reports whatever syntax error it finds instead of the
//! classical-script one — not a silent misbehavior. A false negative only
//! matters for dynamic `import()` expressions, which are legal in
//! classical scripts too and don't need module mode, so this deliberately
//! excludes them (see `wants_module_mode`'s doc).
//!
//! Byte-scanner shape mirrors `agenterm_rh::project_import::literal_imports`
//! (skip comments/strings, scan identifiers) — same proven pattern, reused
//! rather than re-derived, extended to skip `'` single-quoted strings too
//! (rh's scanner only skips `"`/backtick because Rhai's own import syntax
//! doesn't need to worry about `'`; real JS strings can use any of the
//! three, so this one has to).

/// Does `source` contain a top-level static `import` or `export`
/// declaration? Excludes dynamic `import(...)` calls (don't need module
/// mode) and property accesses like `obj.import`/`obj.export` (not
/// declarations). Comments and string contents are skipped, so a string
/// merely containing the word "import" doesn't trigger a false positive.
pub fn wants_module_mode(source: &str) -> bool {
    let bytes = source.as_bytes();
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
            b'"' | b'\'' | b'`' => skip_string(bytes, &mut index),
            byte if byte.is_ascii_alphabetic() || byte == b'_' || byte == b'$' => {
                let start = index;
                index += 1;
                while index < bytes.len()
                    && (bytes[index].is_ascii_alphanumeric()
                        || bytes[index] == b'_'
                        || bytes[index] == b'$')
                {
                    index += 1;
                }
                let word = &source[start..index];
                let preceded_by_dot = start > 0 && bytes[start - 1] == b'.';
                if !preceded_by_dot {
                    if word == "export" {
                        return true;
                    }
                    if word == "import" && !followed_by_call_paren(bytes, index) {
                        return true;
                    }
                }
            }
            _ => index += 1,
        }
    }
    false
}

fn followed_by_call_paren(bytes: &[u8], mut index: usize) -> bool {
    while index < bytes.len() && bytes[index].is_ascii_whitespace() {
        index += 1;
    }
    bytes.get(index) == Some(&b'(')
}

fn skip_string(bytes: &[u8], index: &mut usize) {
    let delimiter = bytes[*index];
    *index += 1;
    while *index < bytes.len() && bytes[*index] != delimiter {
        if bytes[*index] == b'\\' {
            *index += 1;
        }
        *index = index.saturating_add(1);
    }
    // Land past the closing delimiter; an unterminated string just runs
    // the scan to end-of-source, which is fine — the real parser (whichever
    // path we route to) is what actually reports that as a syntax error.
    *index += 1;
}

#[cfg(test)]
mod tests {
    use super::wants_module_mode;

    #[test]
    fn plain_script_has_no_module_intent() {
        assert!(!wants_module_mode("function entry() { return 1; }"));
    }

    #[test]
    fn detects_top_level_import() {
        assert!(wants_module_mode(
            "import { x } from './a.js';\nfunction entry() { return x; }"
        ));
    }

    #[test]
    fn detects_top_level_export() {
        assert!(wants_module_mode("export function entry() { return 1; }"));
    }

    #[test]
    fn ignores_import_word_inside_a_double_quoted_string() {
        assert!(!wants_module_mode(
            "const s = \"this has the word import in it\";\nfunction entry() { return s; }"
        ));
    }

    #[test]
    fn ignores_import_word_inside_a_single_quoted_string() {
        assert!(!wants_module_mode(
            "const s = 'this has the word import in it';\nfunction entry() { return s; }"
        ));
    }

    #[test]
    fn ignores_import_word_inside_a_template_string() {
        assert!(!wants_module_mode(
            "const s = `this has the word import in it`;\nfunction entry() { return s; }"
        ));
    }

    #[test]
    fn ignores_import_word_inside_a_line_comment() {
        assert!(!wants_module_mode(
            "// import notes\nfunction entry() { return 1; }"
        ));
    }

    #[test]
    fn ignores_export_word_inside_a_block_comment() {
        assert!(!wants_module_mode(
            "/* export block comment */\nfunction entry() { return 1; }"
        ));
    }

    #[test]
    fn ignores_dynamic_import_call() {
        assert!(!wants_module_mode(
            "function entry() { return import('./x.js'); }"
        ));
    }

    #[test]
    fn ignores_property_access_named_import_or_export() {
        assert!(!wants_module_mode(
            "const obj = {};\nobj.import = 1;\nobj.export = 2;\nfunction entry() { return obj.import; }"
        ));
    }

    #[test]
    fn detects_import_meta() {
        // import.meta is only valid inside a real module — routing it to
        // the module path is the correct call, not an over-trigger.
        assert!(wants_module_mode(
            "function entry() { return import.meta.url; }"
        ));
    }
}
