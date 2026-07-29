// Deterministic Cargo target inventory.
//
// Usage:
//   agenterm-cli script run scripts/rhai/target-report.rhai --profile local \
//     --timeout-ms 10000 --max-operations 10000000 -- REPO [TARGET] [--json]

fn require(condition, message) {
    if !condition {
        throw message;
    }
}

require(args.len >= 1 && args.len <= 3, "expected: REPO [TARGET] [--json]");

let repo_path = std::path::absolute(args[0]).display;
let repo_target_path = std::path::absolute(
    std::path::join(repo_path, "target").display
).display;
let repo_release_target_path = std::path::absolute(
    std::path::join(repo_path, "target-release").display
).display;

let target_argument = if args.len >= 2 && args[1] != "" {
    args[1]
} else {
    "target"
};
let target_candidate = std::path::PathBuf::from(target_argument);
let target_path = if target_candidate.is_absolute {
    std::path::absolute(target_argument).display
} else {
    std::path::absolute(std::path::join(repo_path, target_argument).display).display
};
let json_output = args.len == 3 && args[2] == "--json";
require(args.len != 3 || json_output, "unknown target-report option");

let repo_local = target_path.to_lower() == repo_target_path.to_lower()
    || target_path.to_lower() == repo_release_target_path.to_lower();
let target_kind = if target_path.to_lower() == repo_release_target_path.to_lower() {
    "release-scratch"
} else if target_path.to_lower() == repo_target_path.to_lower() {
    "development"
} else {
    "external"
};
let exists = std::fs::exists(target_path);
if exists {
    require(std::fs::metadata(target_path).is_dir, "Cargo target path is not a directory");
}

let pending = [];
if exists {
    pending.push(#{path: target_path, profile: "(root)"});
}

let profiles = #{};
let file_count = 0;
let total_bytes = 0;
let oldest_millis = 0;
let oldest_text = ();
let newest_millis = 0;
let newest_text = ();

while pending.len > 0 {
    let item = pending.pop();
    for entry in std::fs::read_dir(item.path) {
        if entry.is_dir {
            let child_profile = if item.profile == "(root)" {
                entry.file_name
            } else {
                item.profile
            };
            pending.push(#{path: entry.path.display, profile: child_profile});
            continue;
        }
        if !entry.is_file {
            continue;
        }

        let metadata = entry.metadata;
        let modified = metadata.modified;
        let modified_millis = modified.unix_millis;
        let bytes = metadata.len;

        file_count += 1;
        total_bytes += bytes;
        if file_count == 1 || modified_millis < oldest_millis {
            oldest_millis = modified_millis;
            oldest_text = modified.rfc3339;
        }
        if file_count == 1 || modified_millis > newest_millis {
            newest_millis = modified_millis;
            newest_text = modified.rfc3339;
        }

        if !profiles.contains(item.profile) {
            profiles[item.profile] = #{
                name: item.profile,
                files: 0,
                bytes: 0
            };
        }
        let profile = profiles[item.profile];
        profile.files += 1;
        profile.bytes += bytes;
        profiles[item.profile] = profile;
    }
}

let now_millis = std::time::SystemTime::now().unix_millis;
let oldest_age_days = if file_count == 0 {
    ()
} else {
    (((now_millis - oldest_millis).to_float() / 86_400_000.0) * 1000.0).round()
        / 1000.0
};

let profile_reports = [];
for name in profiles.keys() {
    profile_reports.push(profiles[name]);
}

let report = #{
    schema_version: 1,
    target_path: target_path,
    exists: exists,
    repo_local: repo_local,
    cleanup_allowed: repo_local,
    target_kind: target_kind,
    files: file_count,
    bytes: total_bytes,
    oldest_write_utc: oldest_text,
    oldest_age_days: oldest_age_days,
    newest_write_utc: newest_text,
    profiles: profile_reports
};

if json_output {
    print(rhai::json::stringify_pretty(report));
} else {
    print(
        "Cargo target: " + target_path +
        "; kind=" + target_kind +
        "; repo_local=" + repo_local +
        "; files=" + file_count +
        "; bytes=" + total_bytes +
        "; oldest_age_days=" + oldest_age_days
    );
    for profile in profile_reports {
        print(
            "  " + profile.name +
            ": files=" + profile.files +
            ", bytes=" + profile.bytes
        );
    }
}
