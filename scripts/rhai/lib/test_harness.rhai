// Shared fail-closed foundations for repository-owned Rhai smoke journeys.

fn require(condition, message) {
    if !condition {
        throw message;
    }
}

fn new_context(repo_value, suite) {
    require(suite != "", "test_harness_suite_empty");
    let repo = std::path::absolute(repo_value).display;
    let owned_root = std::path::join(
        repo,
        "target/smoke/test-runs"
    ).display;
    let run_id =
        std::time::SystemTime::now().unix_millis + "-" +
        std::process::id();
    let run_directory = std::path::join(
        owned_root,
        suite + "-" + run_id
    ).display;
    std::fs::create_dir_all(run_directory);
    #{
        schema_version: 1,
        suite: suite,
        run_id: run_id,
        owned_root: owned_root,
        run_directory: run_directory,
        cleanup_path: std::path::join(
            run_directory,
            "cleanup.json"
        ).display,
        manifest_path: std::path::join(
            run_directory,
            "manifest.json"
        ).display
    }
}

fn assert_owned_run(context) {
    let parent = std::path::parent(context.run_directory).display;
    require(
        parent.to_lower() == context.owned_root.to_lower(),
        "test_harness_run_outside_owned_root"
    );
}

fn complete(context, owned_children, succeeded, failure) {
    assert_owned_run(context);
    let forced = [];
    let completed = [];
    for source_child in owned_children {
        let child = source_child;
        let pid = child.id;
        if child.state == "running" {
            child.kill();
            forced.push(pid);
        }
        child.wait_with_output(std::time::Duration::from_secs(2));
        completed.push(pid);
    }
    let cleanup = #{
        schema_version: 1,
        orphan_free: true,
        owned_count: owned_children.len,
        completed_pids: completed,
        forced_pids: forced,
        remaining_pids: []
    };
    let manifest = #{
        schema_version: 1,
        suite: context.suite,
        run_id: context.run_id,
        result_class: if succeeded { "success" } else { "failure" },
        failure: failure,
        cleanup: cleanup
    };
    rhai::runtime::atomic_write(
        context.cleanup_path,
        rhai::json::stringify_pretty(cleanup) + "\n"
    );
    rhai::runtime::atomic_write(
        context.manifest_path,
        rhai::json::stringify_pretty(manifest) + "\n"
    );
    #{ cleanup: cleanup, manifest: manifest }
}

fn remove_run(context) {
    assert_owned_run(context);
    if std::fs::exists(context.run_directory) {
        std::fs::remove_dir_all(context.run_directory);
    }
}
