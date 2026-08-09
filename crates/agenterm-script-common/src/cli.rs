//! Shared `check-many` argv parsing. All three engines carried a
//! byte-identical `parse_check_many_cli` (same flags, same validation,
//! same accepted-but-ignored rhai-compat flags — rh's comment: "so the
//! same wrapper scripts can call either engine's check-many with
//! identical argv") differing only in the error type each wrapped the
//! message strings into. This is the one implementation; engines adapt
//! the `String` error into their own type at the call site
//! (`map_err(RhError::Parse)` / `map_err(QjsError::Check)` / pass-through
//! for lua, which already uses `String`).

use std::path::PathBuf;

use crate::check_many::{CheckManyOptions, DEFAULT_SOURCE_BYTES, DEFAULT_WALL_TIME_MS, ParsedCheckManyCli};

/// Parse `check-many` argv: `--manifest FILE` (required),
/// `--project-root DIR`, `--timeout-ms N`, `--max-output-bytes N`
/// (clamps the per-file source budget downward), `--profile
/// local|pure|observe` (validated, otherwise ignored), `--json`, plus
/// `--max-operations`/`--max-collection-items`/`--max-string-bytes`
/// accepted-but-ignored for rhai-wrapper compatibility. Unknown flags are
/// an error, matching every engine's existing behavior.
pub fn parse_check_many_cli<I>(mut args: I) -> Result<ParsedCheckManyCli, String>
where
    I: Iterator<Item = String>,
{
    let mut manifest_path = None::<PathBuf>;
    let mut project_root = PathBuf::from(".");
    let mut wall_time_ms = DEFAULT_WALL_TIME_MS;
    let mut source_bytes = DEFAULT_SOURCE_BYTES;
    let mut json = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--manifest" => {
                manifest_path = Some(PathBuf::from(next_value(&mut args, "--manifest")?));
            }
            "--project-root" => {
                project_root = PathBuf::from(next_value(&mut args, "--project-root")?);
            }
            "--timeout-ms" => {
                wall_time_ms = next_value(&mut args, "--timeout-ms")?
                    .parse()
                    .map_err(|err| format!("timeout-ms: {err}"))?;
            }
            "--max-output-bytes" => {
                let value = next_value(&mut args, "--max-output-bytes")?
                    .parse::<usize>()
                    .map_err(|err| format!("max-output-bytes: {err}"))?;
                source_bytes = source_bytes.min(value);
            }
            "--profile" => {
                let profile = next_value(&mut args, "--profile")?;
                if !matches!(profile.as_str(), "local" | "pure" | "observe") {
                    return Err(format!("unknown script profile: {profile}"));
                }
            }
            "--max-operations" | "--max-collection-items" | "--max-string-bytes" => {
                let _ = next_value(&mut args, arg.as_str())?;
            }
            "--json" => json = true,
            other => return Err(format!("unknown check-many option `{other}`")),
        }
    }
    let manifest_path =
        manifest_path.ok_or_else(|| "check-many requires --manifest FILE".to_owned())?;
    Ok(ParsedCheckManyCli {
        manifest_path,
        options: CheckManyOptions {
            project_root,
            wall_time_ms,
            source_bytes,
        },
        json,
    })
}

fn next_value<I>(args: &mut I, option: &str) -> Result<String, String>
where
    I: Iterator<Item = String>,
{
    args.next()
        .ok_or_else(|| format!("missing value after {option}"))
}

// ── slice-based flag/positional helpers ─────────────────────────────────
//
// `agenterm-lua`'s and `agenterm-qjs`'s `main.rs` each hand-rolled a small
// family of argv helpers for their CLI verbs (`require_arg`,
// `require_flag_value`, `require_flag_value_opt`/`optional_flag_value`,
// `find_flag_value`). qjs's version caused a REAL bug (documented in
// `plan/plan-v0.1.16.md`'s QJS-M5d entry): an iterator-based
// `require_flag_value`-style helper internally did `args.collect()`,
// draining the iterator, so a *second* flag lookup on the same `args`
// iterator (e.g. `pack build --dir X --project-root Y` looking up both
// `--dir` and `--project-root`) always found nothing for the second flag —
// not because it was absent, but because the first lookup had already
// consumed the whole tail. qjs's fix at the time was "collect once into a
// Vec, query the same slice twice"; these helpers make that the *only*
// shape available; every helper below takes `&[String]` and returns
// borrowed data, so calling one twice on the same slice is just... calling
// a function twice. There is no iterator left to accidentally drain.
//
// Each engine's `main.rs` has its own message-formatting conventions for
// "missing positional"/"missing flag" (lua prefixes `"missing argument:
// {usage}"`; qjs uses `"usage: agenterm-qjs {command} <file.js>"` — see
// each engine's own call sites). These helpers deliberately do NOT bake in
// a prefix: `usage`/`context` strings are used verbatim as the error, so
// callers control the exact observable message by passing in the string
// they already relied on.

/// Find `flag`'s value — the argument immediately following it — in
/// `args`. Returns `None` both when `flag` is absent and when `flag` is
/// present but is the last element (no following value): callers that want
/// to distinguish those two cases should use [`require_flag_value`]
/// instead. Safe to call repeatedly on the same slice for different flags
/// (this is the whole point — see the module-level doc above).
pub fn find_flag_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|pos| args.get(pos + 1))
        .map(String::as_str)
}

/// Like [`find_flag_value`], but requires the flag to be present (erroring
/// with `usage` verbatim if not) and its value to follow (erroring with
/// `"{flag} requires a value"` if `flag` is the last element).
pub fn require_flag_value<'a>(
    args: &'a [String],
    flag: &str,
    usage: &str,
) -> Result<&'a str, String> {
    let pos = args
        .iter()
        .position(|a| a == flag)
        .ok_or_else(|| usage.to_owned())?;
    args.get(pos + 1)
        .map(String::as_str)
        .ok_or_else(|| format!("{flag} requires a value"))
}

/// A positional argument at `index`. Errors with `usage` verbatim if
/// `args` is too short. Matches both engines' existing behavior of
/// indexing blindly (no rejection of flag-shaped values at that index) —
/// every real call site's positional always precedes any flags in its
/// verb's accepted argv shape, so this has never needed to be smarter.
pub fn positional<'a>(args: &'a [String], index: usize, usage: &str) -> Result<&'a str, String> {
    args.get(index)
        .map(String::as_str)
        .ok_or_else(|| usage.to_owned())
}

/// Whether `flag` appears anywhere in `args` (a boolean switch, not a
/// flag-with-value).
pub fn has_flag(args: &[String], flag: &str) -> bool {
    args.iter().any(|a| a == flag)
}

#[cfg(test)]
mod flag_helper_tests {
    use super::*;

    fn owned(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn find_flag_value_returns_value_when_present() {
        let args = owned(&["--dir", "out"]);
        assert_eq!(find_flag_value(&args, "--dir"), Some("out"));
    }

    #[test]
    fn find_flag_value_none_when_absent() {
        let args = owned(&["--dir", "out"]);
        assert_eq!(find_flag_value(&args, "--project-root"), None);
    }

    #[test]
    fn find_flag_value_none_when_flag_is_last_element() {
        // Flag present, no following value — collapsed into `None`, same
        // as "absent", matching qjs's original `find_flag_value` semantics
        // (callers that need to distinguish the two cases use
        // `require_flag_value`).
        let args = owned(&["--dir"]);
        assert_eq!(find_flag_value(&args, "--dir"), None);
    }

    #[test]
    fn find_flag_value_regression_two_lookups_on_same_slice_both_succeed() {
        // The actual qjs bug: an iterator-draining helper made a *second*
        // flag lookup on the same argv tail always return None. Slice-based
        // lookups can't exhibit this — prove it by looking up two distinct
        // flags from one collected slice, in both orders.
        let args = owned(&["--dir", "out", "--project-root", "proj"]);
        assert_eq!(find_flag_value(&args, "--dir"), Some("out"));
        assert_eq!(find_flag_value(&args, "--project-root"), Some("proj"));

        let reordered = owned(&["--project-root", "proj", "--dir", "out"]);
        assert_eq!(find_flag_value(&reordered, "--dir"), Some("out"));
        assert_eq!(find_flag_value(&reordered, "--project-root"), Some("proj"));
    }

    #[test]
    fn require_flag_value_returns_value_when_present() {
        let args = owned(&["--manifest", "m.json"]);
        assert_eq!(
            require_flag_value(&args, "--manifest", "usage: x"),
            Ok("m.json")
        );
    }

    #[test]
    fn require_flag_value_errors_with_usage_when_flag_absent() {
        let args = owned(&["--json"]);
        let err = require_flag_value(&args, "--manifest", "check-many requires --manifest <file>")
            .expect_err("missing flag");
        assert_eq!(err, "check-many requires --manifest <file>");
    }

    #[test]
    fn require_flag_value_errors_cleanly_when_flag_is_last_with_no_value() {
        // "flag at the end with no value errors cleanly" — distinct
        // message from the "flag absent" case, unlike `find_flag_value`.
        let args = owned(&["--manifest"]);
        let err = require_flag_value(&args, "--manifest", "usage: x").expect_err("dangling flag");
        assert_eq!(err, "--manifest requires a value");
    }

    #[test]
    fn require_flag_value_repeated_lookup_on_same_slice_still_works() {
        // Regression: a lookup for one flag must not disturb a later
        // lookup for another flag on the same slice.
        let args = owned(&["--dir", "out", "--project-root", "proj"]);
        assert_eq!(require_flag_value(&args, "--dir", "usage: x"), Ok("out"));
        assert_eq!(
            require_flag_value(&args, "--project-root", "usage: y"),
            Ok("proj")
        );
    }

    #[test]
    fn positional_returns_value_when_present() {
        let args = owned(&["file.lua", "--dir", "out"]);
        assert_eq!(positional(&args, 0, "usage: x"), Ok("file.lua"));
    }

    #[test]
    fn positional_errors_with_usage_verbatim_when_missing() {
        let args: Vec<String> = owned(&[]);
        let err = positional(&args, 0, "missing argument: check <file.lua>")
            .expect_err("missing positional");
        assert_eq!(err, "missing argument: check <file.lua>");
    }

    #[test]
    fn positional_does_not_reject_flag_shaped_values() {
        // Matches existing engine behavior: no call site has ever needed
        // this, so it's a plain index, not a flag-aware scan.
        let args = owned(&["--dir", "out"]);
        assert_eq!(positional(&args, 0, "usage: x"), Ok("--dir"));
    }

    #[test]
    fn has_flag_true_when_present_anywhere() {
        let args = owned(&["--manifest", "m.json", "--json"]);
        assert!(has_flag(&args, "--json"));
    }

    #[test]
    fn has_flag_false_when_absent() {
        let args = owned(&["--manifest", "m.json"]);
        assert!(!has_flag(&args, "--json"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<ParsedCheckManyCli, String> {
        parse_check_many_cli(args.iter().map(|s| (*s).to_owned()))
    }

    #[test]
    fn parses_all_flags() {
        let parsed = parse(&[
            "--manifest",
            "m.json",
            "--project-root",
            "proj",
            "--timeout-ms",
            "5000",
            "--max-output-bytes",
            "1024",
            "--profile",
            "local",
            "--max-operations",
            "1000000",
            "--json",
        ])
        .expect("parse");
        assert_eq!(parsed.manifest_path, PathBuf::from("m.json"));
        assert_eq!(parsed.options.project_root, PathBuf::from("proj"));
        assert_eq!(parsed.options.wall_time_ms, 5000);
        assert_eq!(parsed.options.source_bytes, 1024);
        assert!(parsed.json);
    }

    #[test]
    fn max_output_bytes_only_clamps_downward() {
        let parsed = parse(&[
            "--manifest",
            "m.json",
            "--max-output-bytes",
            &(DEFAULT_SOURCE_BYTES * 2).to_string(),
        ])
        .expect("parse");
        assert_eq!(parsed.options.source_bytes, DEFAULT_SOURCE_BYTES);
    }

    #[test]
    fn requires_manifest() {
        let err = parse(&["--json"]).expect_err("missing manifest");
        assert!(err.contains("--manifest"), "{err}");
    }

    #[test]
    fn rejects_unknown_flags() {
        let err = parse(&["--manifest", "m.json", "--nope"]).expect_err("unknown flag");
        assert!(err.contains("--nope"), "{err}");
    }

    #[test]
    fn rejects_unknown_profile() {
        let err =
            parse(&["--manifest", "m.json", "--profile", "weird"]).expect_err("bad profile");
        assert!(err.contains("weird"), "{err}");
    }

    #[test]
    fn rejects_flag_missing_its_value() {
        let err = parse(&["--manifest"]).expect_err("dangling flag");
        assert!(err.contains("missing value"), "{err}");
    }
}
