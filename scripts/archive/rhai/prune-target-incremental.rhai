// Reclaim obsolete finalized rustc sessions without discarding the newest
// reusable cache for any Cargo compilation unit.
//
// Args:
//   REPO [TARGET] [--manifest PATH --invocation ID]

// TARGET must be the exact repository-local `target` directory. This task is
// intentionally narrower than Cargo target cleanup: it only visits
// `target/debug/incremental`, holds Cargo's real profile lock for the whole
// mutation, and acquires every rustc session lock before deleting that session.
// Distinct compilation-unit roots may represent different target kinds,
// features, or profiles. A producer may authorize removal of an untouched
// whole root with an exact invocation manifest. `touched` is the only usage
// authority; metadata identities only detect stale/changed snapshots and must
// never infer whether rustc used a root.

fn require(condition, message) {
    if !condition {
        throw message;
    }
}

fn exact_repo_root(repo) {
    let command = std::process::command("git");
    command.args(["-C", repo, "rev-parse", "--show-prefix"]);
    command.timeout(std::time::Duration::from_secs(10));
    let output = command.output();
    require(output.success, output.error("incremental-prune-git-root"));
    let prefix = output.stdout_text();
    prefix.trim();
    require(prefix == "", "incremental_prune_repo_not_exact_git_root:" + repo);
}

fn direct_directory(path, code) {
    require(std::fs::exists(path), code + "_missing:" + path);
    let metadata = std::fs::symlink_metadata(path);
    require(metadata.is_dir, code + "_not_directory:" + path);
    require(
        !metadata.is_symlink && !metadata.is_reparse_point,
        code + "_indirect:" + path
    );
}

fn direct_file(path) {
    if !std::fs::exists(path) {
        return false;
    }
    let metadata = std::fs::symlink_metadata(path);
    metadata.is_file && !metadata.is_symlink && !metadata.is_reparse_point
}

fn inspect_tree(root) {
    let pending = [root];
    let bytes = 0;
    let files = 0;
    while pending.len > 0 {
        let directory = pending.pop();
        for entry in std::fs::read_dir(directory) {
            let path = entry.path.display;
            let metadata = std::fs::symlink_metadata(path);
            if metadata.is_symlink || metadata.is_reparse_point {
                return #{ valid: false, bytes: bytes, files: files };
            }
            if metadata.is_dir {
                pending.push(path);
            } else if metadata.is_file {
                bytes += metadata.len;
                files += 1;
            } else {
                return #{ valid: false, bytes: bytes, files: files };
            }
        }
    }
    #{ valid: true, bytes: bytes, files: files }
}

fn safe_invocation_id(value) {
    if value.len < 16 || value.len > 128 {
        return false;
    }
    let alphabet = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789._-";
    for character in value {
        if alphabet.index_of(character.to_string()) < 0 {
            return false;
        }
    }
    true
}

fn safe_root_name(value) {
    if value == "" || value == "." || value == ".." || value.len > 255 {
        return false;
    }
    !value.contains("/") && !value.contains("\\")
}

fn lowercase_sha256(value) {
    if value.len != 64 {
        return false;
    }
    let alphabet = "0123456789abcdef";
    for character in value {
        if alphabet.index_of(character.to_string()) < 0 {
            return false;
        }
    }
    true
}

// The serialization is deliberately a sorted JSON array of strings so the
// producer can reproduce it without depending on directory enumeration order.
// Length and mtime are observations for TOCTOU detection, not usage heuristics.
fn metadata_identity(root, maximum_entries) {
    try {
        let root_metadata = std::fs::symlink_metadata(root);
        if !root_metadata.is_dir
                || root_metadata.is_symlink
                || root_metadata.is_reparse_point {
            return #{ valid: false, working: false, identity: "", entries: 0 };
        }
        let records = [
            "d|.|" + root_metadata.len + "|" +
                root_metadata.modified.unix_millis
        ];
        let pending = [#{ path: root, relative: "", depth: 0 }];
        while pending.len > 0 {
            let directory = pending.pop();
            if directory.depth >= 64 {
                return #{ valid: false, working: false, identity: "", entries: records.len };
            }
            for entry in std::fs::read_dir(directory.path) {
                if records.len >= maximum_entries {
                    return #{ valid: false, working: false, identity: "", entries: records.len };
                }
                let relative = if directory.relative == "" {
                    entry.file_name
                } else {
                    directory.relative + "/" + entry.file_name
                };
                if relative.len > 4096 {
                    return #{ valid: false, working: false, identity: "", entries: records.len };
                }
                let path = entry.path.display;
                let metadata = std::fs::symlink_metadata(path);
                if metadata.is_symlink || metadata.is_reparse_point {
                    return #{ valid: false, working: false, identity: "", entries: records.len };
                }
                if metadata.is_dir {
                    if entry.file_name.ends_with("-working") {
                        return #{ valid: true, working: true, identity: "", entries: records.len };
                    }
                    records.push(
                        "d|" + relative + "|" + metadata.len + "|" +
                            metadata.modified.unix_millis
                    );
                    pending.push(#{
                        path: path,
                        relative: relative,
                        depth: directory.depth + 1
                    });
                } else if metadata.is_file {
                    records.push(
                        "f|" + relative + "|" + metadata.len + "|" +
                            metadata.modified.unix_millis
                    );
                } else {
                    return #{ valid: false, working: false, identity: "", entries: records.len };
                }
            }
        }
        records.sort();
        return #{
            valid: true,
            working: false,
            identity: rhai::crypto::sha256(
                rhai::bytes::from_text(rhai::json::stringify(records))
            ),
            entries: records.len
        };
    } catch (error) {
        return #{ valid: false, working: false, identity: "", entries: 0 };
    }
}

fn manifest_entry_for(entries, name) {
    for entry in entries {
        if entry.name == name {
            return #{
                found: true,
                touched: entry.touched,
                before_identity: entry.before_identity,
                after_identity: entry.after_identity
            };
        }
    }
    #{ found: false }
}

fn load_root_manifest(path, invocation, target, incremental) {
    try {
        if !safe_invocation_id(invocation) || !direct_file(path) {
            return #{ valid: false, entries: [] };
        }
        let metadata = std::fs::symlink_metadata(path);
        if metadata.len <= 0 || metadata.len > 2097152 {
            return #{ valid: false, entries: [] };
        }
        let document = rhai::json::parse_file(path);
        if document.kind != "agenterm-incremental-touch-manifest"
                || document.schema_version != 1
                || document.invocation_id != invocation
                || document.target != target
                || document.incremental_root != incremental
                || document.snapshot_complete != true
                || document.rustc_invocations <= 0
                || document.rustc_invocations > 100000
                || document.identity_algorithm != "full-tree-metadata-v1"
                || document.roots.len > 8192 {
            return #{ valid: false, entries: [] };
        }
        let names = [];
        let entries = [];
        for entry in document.roots {
            if !safe_root_name(entry.name)
                    || names.contains(entry.name)
                    || entry.touched.type_of() != "bool"
                    || !lowercase_sha256(entry.before_identity)
                    || !lowercase_sha256(entry.after_identity) {
                return #{ valid: false, entries: [] };
            }
            names.push(entry.name);
            entries.push(#{
                name: entry.name,
                touched: entry.touched,
                before_identity: entry.before_identity,
                after_identity: entry.after_identity
            });
        }
        return #{ valid: true, entries: entries };
    } catch (error) {
        return #{ valid: false, entries: [] };
    }
}

fn finalized_session_lock_name(name) {
    let parts = name.split("-");
    if parts.len != 4
            || parts[0] != "s"
            || parts[1] == ""
            || parts[2] == ""
            || parts[3] == ""
            || parts[3] == "working" {
        return "";
    }
    "s-" + parts[1] + "-" + parts[2] + ".lock"
}

fn whole_root_session_locks(root) {
    try {
        let lock_paths = [];
        let required_locks = [];
        for entry in std::fs::read_dir(root) {
            let path = entry.path.display;
            let metadata = std::fs::symlink_metadata(path);
            if metadata.is_symlink || metadata.is_reparse_point {
                return #{ valid: false, acquired: false, guards: [] };
            }
            if metadata.is_dir {
                if entry.file_name.ends_with("-working") {
                    return #{ valid: true, acquired: false, guards: [] };
                }
                let lock_name = finalized_session_lock_name(entry.file_name);
                if lock_name == "" {
                    return #{ valid: false, acquired: false, guards: [] };
                }
                required_locks.push(lock_name);
            } else if metadata.is_file && entry.file_name.ends_with(".lock") {
                lock_paths.push(#{ name: entry.file_name, path: path });
            } else {
                return #{ valid: false, acquired: false, guards: [] };
            }
        }
        for required in required_locks {
            let found = false;
            for candidate in lock_paths {
                if candidate.name == required {
                    found = true;
                }
            }
            if !found {
                return #{ valid: false, acquired: false, guards: [] };
            }
        }
        let guards = [];
        for candidate in lock_paths {
            if !direct_file(candidate.path) {
                return #{ valid: false, acquired: false, guards: guards };
            }
            let guard = std::fs::try_lock_exclusive(candidate.path);
            if !guard.acquired {
                return #{ valid: true, acquired: false, guards: guards };
            }
            guards.push(guard);
        }
        return #{ valid: true, acquired: true, guards: guards };
    } catch (error) {
        return #{ valid: false, acquired: false, guards: [] };
    }
}

fn base36(value) {
    if value.len == 0 || value.len > 12 {
        return -1;
    }
    let alphabet = "0123456789abcdefghijklmnopqrstuvwxyz";
    let result = 0;
    for character in value {
        let digit = alphabet.index_of(character.to_string());
        if digit < 0 {
            return -1;
        }
        result = result * 36 + digit;
    }
    result
}

require(
    args.len == 1 || args.len == 2 || args.len == 6,
    "expected: REPO [TARGET] [--manifest PATH --invocation ID]"
);
if args.len == 6 {
    require(
        args[2] == "--manifest" && args[4] == "--invocation",
        "expected: REPO TARGET --manifest PATH --invocation ID"
    );
}
let repo = std::path::absolute(args[0]).display;
exact_repo_root(repo);

let expected_target = std::path::absolute(
    std::path::join(repo, "target").display
).display;
let target_argument = if args.len >= 2 && args[1] != "" {
    args[1]
} else {
    "target"
};
let target_candidate = std::path::PathBuf::from(target_argument);
let target = if target_candidate.is_absolute {
    std::path::absolute(target_argument).display
} else {
    std::path::absolute(std::path::join(repo, target_argument).display).display
};
require(
    target == expected_target,
    "incremental_prune_target_not_exact_repo_target:" + target
);
direct_directory(target, "incremental_prune_target");

let debug_profile = std::path::join(target, "debug").display;
let incremental = std::path::join(debug_profile, "incremental").display;
if !std::fs::exists(incremental) {
    print("Cargo incremental prune: skipped (debug/incremental is absent)");
    return;
}
direct_directory(debug_profile, "incremental_prune_debug");
direct_directory(incremental, "incremental_prune_root");

let cargo_lock_path = std::path::join(debug_profile, ".cargo-lock").display;
if !direct_file(cargo_lock_path) {
    print("Cargo incremental prune: skipped (debug/.cargo-lock is absent or indirect)");
    return;
}
let cargo_lock = std::fs::try_lock_exclusive(cargo_lock_path);
if !cargo_lock.acquired {
    print("Cargo incremental prune: skipped (debug/.cargo-lock is active)");
    return;
}

let root_manifest_state = "absent";
let root_manifest_entries = [];
if args.len == 6 {
    let manifest_path = std::path::absolute(args[3]).display;
    let loaded = load_root_manifest(
        manifest_path,
        args[5],
        target,
        incremental
    );
    if loaded.valid {
        root_manifest_state = "valid";
        root_manifest_entries = loaded.entries;
    } else {
        root_manifest_state = "invalid";
    }
}

let removed_roots = 0;
let removed_root_files = 0;
let removed_root_logical_bytes = 0;
let skipped_root_touched = 0;
let skipped_root_identity = 0;
let skipped_root_working_or_lock = 0;
let skipped_root_shape = 0;
let skipped_root_remove_failure = 0;
if root_manifest_state == "valid" {
    for crate_entry in std::fs::read_dir(incremental) {
        if !crate_entry.is_dir {
            continue;
        }
        let crate_path = crate_entry.path.display;
        let manifest_entry = manifest_entry_for(
            root_manifest_entries,
            crate_entry.file_name
        );
        if !manifest_entry.found {
            continue;
        }
        if manifest_entry.touched {
            skipped_root_touched += 1;
            continue;
        }
        if manifest_entry.before_identity != manifest_entry.after_identity {
            skipped_root_identity += 1;
            continue;
        }
        let first_identity = metadata_identity(crate_path, 100000);
        if !first_identity.valid
                || first_identity.working
                || first_identity.identity != manifest_entry.after_identity {
            skipped_root_identity += 1;
            continue;
        }
        let root_locks = whole_root_session_locks(crate_path);
        if !root_locks.valid {
            skipped_root_shape += 1;
            continue;
        }
        if !root_locks.acquired {
            skipped_root_working_or_lock += 1;
            continue;
        }
        // Keep every guard alive while taking the second identity and removing
        // the whole root. This closes the check/delete window against rustc.
        let held_root_locks = root_locks.guards;
        let second_identity = metadata_identity(crate_path, 100000);
        if !second_identity.valid
                || second_identity.working
                || second_identity.identity != first_identity.identity
                || second_identity.identity != manifest_entry.after_identity {
            skipped_root_identity += 1;
            continue;
        }
        let tree = inspect_tree(crate_path);
        if !tree.valid {
            skipped_root_shape += 1;
            continue;
        }
        try {
            // Reading the length keeps the guard array live through deletion.
            let held_root_lock_count = held_root_locks.len;
            std::fs::remove_dir_all(crate_path);
            removed_roots += 1;
            removed_root_files += tree.files;
            removed_root_logical_bytes += tree.bytes;
        } catch (error) {
            skipped_root_remove_failure += 1;
        }
    }
}

let now_micros = std::time::SystemTime::now().unix_millis * 1000;
let minimum_age_micros = 60000000;
let held_session_locks = [];
let compilation_units = 0;
let finalized_sessions = 0;
let removed_sessions = 0;
let removed_files = 0;
let removed_logical_bytes = 0;
let skipped_indirect = 0;
let skipped_missing_lock = 0;
let skipped_active_lock = 0;
let skipped_young = 0;
let skipped_remove_failure = 0;

for crate_entry in std::fs::read_dir(incremental) {
    if !crate_entry.is_dir {
        continue;
    }
    let crate_path = crate_entry.path.display;
    let crate_metadata = std::fs::symlink_metadata(crate_path);
    if crate_metadata.is_symlink || crate_metadata.is_reparse_point {
        skipped_indirect += 1;
        continue;
    }

    let sessions = [];
    for entry in std::fs::read_dir(crate_path) {
        if !entry.is_dir {
            continue;
        }
        let parts = entry.file_name.split("-");
        if parts.len != 4
                || parts[0] != "s"
                || parts[1] == ""
                || parts[2] == ""
                || parts[3] == ""
                || parts[3] == "working" {
            continue;
        }
        let timestamp_micros = base36(parts[1]);
        if timestamp_micros < 0 {
            continue;
        }
        let session_path = entry.path.display;
        let metadata = std::fs::symlink_metadata(session_path);
        if !metadata.is_dir || metadata.is_symlink || metadata.is_reparse_point {
            skipped_indirect += 1;
            continue;
        }
        sessions.push(#{
            name: entry.file_name,
            path: session_path,
            modified: metadata.modified.unix_millis,
            timestamp_micros: timestamp_micros,
            lock_path: std::path::join(
                crate_path,
                "s-" + parts[1] + "-" + parts[2] + ".lock"
            ).display
        });
    }

    if sessions.len == 0 {
        continue;
    }
    compilation_units += 1;
    finalized_sessions += sessions.len;
    let newest = sessions[0];
    for index in 1..sessions.len {
        let candidate = sessions[index];
        if candidate.timestamp_micros > newest.timestamp_micros
                || (candidate.timestamp_micros == newest.timestamp_micros
                    && candidate.name > newest.name) {
            newest = candidate;
        }
    }

    for candidate in sessions {
        if candidate.path == newest.path {
            continue;
        }
        if now_micros - candidate.timestamp_micros < minimum_age_micros {
            skipped_young += 1;
            continue;
        }
        if !direct_file(candidate.lock_path) {
            skipped_missing_lock += 1;
            continue;
        }
        let session_lock = std::fs::try_lock_exclusive(candidate.lock_path);
        if !session_lock.acquired {
            skipped_active_lock += 1;
            continue;
        }
        // Retain every acquired guard through the whole task. Cargo's profile
        // lock prevents new Cargo work; the per-session lock also covers a
        // direct rustc reader and matches rustc's own GC protocol.
        held_session_locks.push(session_lock);

        if !std::fs::exists(candidate.path) {
            skipped_remove_failure += 1;
            continue;
        }
        let current = std::fs::symlink_metadata(candidate.path);
        if !current.is_dir
                || current.is_symlink
                || current.is_reparse_point
                || current.modified.unix_millis != candidate.modified {
            skipped_indirect += 1;
            continue;
        }
        let tree = inspect_tree(candidate.path);
        if !tree.valid {
            skipped_indirect += 1;
            continue;
        }
        try {
            std::fs::remove_dir_all(candidate.path);
            removed_sessions += 1;
            removed_files += tree.files;
            removed_logical_bytes += tree.bytes;
        } catch (error) {
            skipped_remove_failure += 1;
        }
    }
}

print(
    "Cargo incremental prune: scope=obsolete-finalized-sessions" +
    ", root_manifest=" + root_manifest_state +
    ", removed_roots=" + removed_roots +
    ", removed_root_files=" + removed_root_files +
    ", removed_root_logical_bytes=" + removed_root_logical_bytes +
    ", skipped_root_touched=" + skipped_root_touched +
    ", skipped_root_identity=" + skipped_root_identity +
    ", skipped_root_working_or_lock=" + skipped_root_working_or_lock +
    ", skipped_root_shape=" + skipped_root_shape +
    ", skipped_root_remove_failure=" + skipped_root_remove_failure +
    ", units=" + compilation_units +
    ", finalized=" + finalized_sessions +
    ", removed_sessions=" + removed_sessions +
    ", removed_files=" + removed_files +
    ", removed_logical_bytes=" + removed_logical_bytes +
    ", skipped_indirect=" + skipped_indirect +
    ", skipped_missing_lock=" + skipped_missing_lock +
    ", skipped_active_lock=" + skipped_active_lock +
    ", skipped_young=" + skipped_young +
    ", skipped_remove_failure=" + skipped_remove_failure
);
