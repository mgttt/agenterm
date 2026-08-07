//! Lua script validation: syntax check + API surface audit.
//! Mirrors rh's `shipped_surfaces` / `api_validate` pattern.

use crate::LuaError;

/// Shipped Lua API surfaces (path prefixes available in `std.*` and `fleet.*`).
pub const SHIPPED_SURFACES: &[&str] = &[
    // std.fs
    "std.fs.exists",
    "std.fs.read",
    "std.fs.write",
    "std.fs.metadata",
    "std.fs.copy",
    "std.fs.create_dir",
    "std.fs.read_dir",
    "std.fs.remove_file",
    "std.fs.rename",
    "std.fs.remove_dir_all",
    "std.fs.symlink_metadata",
    // std.process
    "std.process.command",
    "std.process.status",
    "std.process.stdout_file",
    "std.process.id",
    "std.process.list",
    // std.path
    "std.path.absolute",
    "std.path.join",
    "std.path.parent",
    "std.path.file_name",
    "std.path.is_absolute",
    "std.path.from",
    // std.env
    "std.env.get",
    "std.env.has",
    "std.env.current_dir",
    // std.time
    "std.time.now_unix_ms",
    "std.time.now_rfc3339",
    "std.time.Duration.from_millis",
    "std.time.Duration.from_secs",
    // std.json
    "std.json.parse",
    "std.json.stringify",
    // std.crypto
    "std.crypto.sha256",
    "std.crypto.sha256_file",
    // __host
    "__host.fleet_call",
    "__host.args_len",
    "__host.arg",
    "__host.print",
    // Lua builtins (allowed)
    "print",
    "error",
    "assert",
    "pcall",
    "xpcall",
    "type",
    "tostring",
    "tonumber",
    "string",
    "table",
    "math",
    "os.clock",
    "os.date",
    "os.time",
    "os.difftime",
    // rhai compatibility
    "rhai.runtime.atomic_write",
    "rhai.hash.fnv1a64",
    "rh.fail",
];

fn shipped(path: &str) -> bool {
    SHIPPED_SURFACES.contains(&path)
}

/// Result of a Lua API surface audit.
#[derive(Debug)]
pub struct AuditResult {
    /// True if syntax is valid.
    pub syntax_ok: bool,
    /// API calls that are not in the shipped allowlist.
    pub unknown_apis: Vec<String>,
}

/// Check Lua source syntax and audit its API surface against the allowlist.
pub fn check_with_surface_audit(source: &str) -> Result<AuditResult, LuaError> {
    let engine = crate::LuaEngine::new()?;
    engine.check(source)?;

    let unknown_apis = audit_api_surface(source);
    Ok(AuditResult {
        syntax_ok: true,
        unknown_apis,
    })
}

/// Audit API surface: scan source for `std.*.*(` and `fleet.*.*(` call patterns
/// and report any that are not in SHIPPED_SURFACES.
pub fn audit_api_surface(source: &str) -> Vec<String> {
    let mut unknown = Vec::new();
    let prefixes = ["std.", "fleet."];

    for prefix in &prefixes {
        let mut pos = 0usize;
        while let Some(found) = source[pos..].find(prefix) {
            let abs = pos + found;
            let start = abs;
            let mut end = abs + prefix.len();
            // Collect identifier chars (word chars and dots)
            let rest = &source[end..];
            for ch in rest.chars() {
                if ch.is_alphanumeric() || ch == '_' || ch == '.' {
                    end += ch.len_utf8();
                } else {
                    break;
                }
            }
            let path = &source[start..end];
            // Only flag if it looks like a function call (followed by `(` or `:`)
            let after = source[end..].trim_start();
            if (after.starts_with('(') || after.starts_with(':')) && !shipped(path) {
                let entry = path.to_string();
                if !unknown.contains(&entry) {
                    unknown.push(entry);
                }
            }
            pos = end;
        }
    }

    unknown
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn syntax_ok_known_apis() {
        let result = check_with_surface_audit(
            "local x = std.fs.exists('/tmp')\nstd.process.command('cmd', {'/c', 'echo'}, 5000)\nreturn 0"
        ).expect("check");
        assert!(result.syntax_ok);
        assert!(result.unknown_apis.is_empty(), "unexpected unknown: {:?}", result.unknown_apis);
    }

    #[test]
    fn unknown_api_detected() {
        let result = check_with_surface_audit(
            "std.fs.not_a_real_function('x')\nreturn 0"
        ).expect("check");
        assert!(result.syntax_ok);
        assert!(!result.unknown_apis.is_empty());
        assert!(result.unknown_apis.iter().any(|a| a.contains("not_a_real_function")));
    }

    #[test]
    fn fleet_api_audited() {
        let result = check_with_surface_audit(
            "fleet.tabs.list()\nfleet.ui.snapshot()\nreturn 0"
        ).expect("check");
        assert!(result.syntax_ok);
        // fleet.* not in SHIPPED_SURFACES by default — audit reports them
        // but they're valid if __host.fleet_call is provided
    }

    #[test]
    fn unknown_fleet_api_detected() {
        let result = check_with_surface_audit(
            "fleet.secret.backdoor()\nreturn 0"
        ).expect("check");
        assert!(!result.unknown_apis.is_empty());
    }

    #[test]
    fn syntax_error_reported() {
        let result = check_with_surface_audit("return !!");
        assert!(result.is_err());
    }
}
