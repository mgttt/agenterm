// Reclaim obsolete finalized rustc sessions without discarding the newest
// reusable cache for any Cargo compilation unit.
//
// Args:
//   REPO [TARGET]

// TARGET must be the exact repository-local `target` directory. This task is
// intentionally narrower than Cargo target cleanup: it only visits
// `target/debug/incremental`, holds Cargo's real profile lock for the whole
// mutation, and acquires every rustc session lock before deleting that session.
// Distinct compilation-unit roots may represent different target kinds,
// features, or profiles and are therefore retained; pruning those safely needs
// an exact Cargo-unit usage manifest rather than basename or age guesses.

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

require(args.len >= 1 && args.len <= 2, "expected: REPO [TARGET]");
let repo = std::path::absolute(args[0]).display;
exact_repo_root(repo);

let expected_target = std::path::absolute(
    std::path::join(repo, "target").display
).display;
let target_argument = if args.len == 2 && args[1] != "" {
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
    ", root_generations_retained=true" +
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
