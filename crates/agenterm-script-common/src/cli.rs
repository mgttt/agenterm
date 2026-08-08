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
