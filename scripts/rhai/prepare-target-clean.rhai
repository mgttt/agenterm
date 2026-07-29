// Validate an exact repository-local Cargo target and install CACHEDIR.TAG.
//
// Args:
//   REPO [TARGET]

fn require(condition, message) {
    if !condition {
        throw message;
    }
}

require(args.len >= 1 && args.len <= 2, "expected: REPO [TARGET]");

let repo = std::path::absolute(args[0]).display;
let git = std::process::command("git");
git.args(["-C", repo, "rev-parse", "--show-toplevel"]);
git.timeout(std::time::Duration::from_secs(10));
let git_output = git.output();
require(git_output.success, git_output.error("target-clean-git-root"));
let git_root = git_output.stdout_text();
git_root.trim();
let git_root = std::path::absolute(git_root).display;
require(
    repo.to_lower() == git_root.to_lower(),
    "target_clean_repo_not_exact_git_root:" + repo
);

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
let development_target = std::path::absolute(
    std::path::join(repo, "target").display
).display;
let release_target = std::path::absolute(
    std::path::join(repo, "target-release").display
).display;
require(
    target.to_lower() == development_target.to_lower()
        || target.to_lower() == release_target.to_lower(),
    "target_clean_path_not_allowed:" + target
);
require(std::fs::exists(target), "target_clean_path_missing:" + target);
let metadata = std::fs::symlink_metadata(target);
require(metadata.is_dir, "target_clean_path_not_directory:" + target);
require(
    !metadata.is_symlink && !metadata.is_reparse_point,
    "target_clean_path_indirect:" + target
);

let signature = "Signature: 8a477f597d28d172789f06886806bc55";
let content =
    signature + "\n" +
    "# This file is a cache directory tag created by cargo.\n" +
    "# For information about cache directory tags see https://bford.info/cachedir/\n";
let tag = std::path::join(target, "CACHEDIR.TAG").display;
if std::fs::exists(tag) {
    require(
        std::fs::metadata(tag).is_file,
        "target_clean_cache_tag_not_file:" + tag
    );
    require(
        std::fs::read_to_string(tag).starts_with(signature + "\n"),
        "target_clean_cache_tag_invalid:" + tag
    );
} else {
    std::fs::write(tag, content);
}

print("Prepared Cargo cache tag for exact repo target: " + target);
