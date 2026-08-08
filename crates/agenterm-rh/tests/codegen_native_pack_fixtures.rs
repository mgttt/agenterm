//! Native / host-eval locks for crate-local Phase B codegen fixtures.

use std::path::PathBuf;

use agenterm_rh::{CdylibExecutionMode, RH_CODEGEN_REVISION, check, transpile_cdylib_with_mode};

const FIXTURES: [(&str, &[&str], &[&str]); 28] = [
    (
        "rh_empty_map_fn_return.rh",
        &["rh_json_set_path_key(&mut env"],
        &["HashSet::<String>::new()"],
    ),
    (
        "rh_json_map_key_chain_via_locals.rh",
        &["rh_json_set_path_key(&mut m, &[], "],
        &[],
    ),
    (
        "rh_array_index_property_via_local.rh",
        &[
            "rh_json_array_get(&ordered, ",
            "rh_json_int_path(&item, &[\"id\"])",
        ],
        &["rh_json_get_path_key(&ordered"],
    ),
    (
        "rh_null_unit_compare.rh",
        &[".is_null()"],
        &["rh_host_run_script(RH_SCRIPT_SOURCE)"],
    ),
    (
        "rh_snapshot_tabs_index_return.rh",
        &["rh_json_get_path_index(&snapshot, &[\"tabs\"], index)"],
        &["rh_json_string_path_index(&snapshot, &[\"tabs\"]"],
    ),
    (
        "rh_working_context_dot_chain_via_locals.rh",
        &[
            "rh_json_get_path(&tab, &[\"working_context\"])",
            "rh_json_get_path(&wc, &[\"proxy\"])",
            "rh_json_string_path(&proxy, &[\"source\"])",
        ],
        &[],
    ),
    (
        "rh_tab_active_map_key_vs_dot.rh",
        &[
            "rh_json_get_path(&tab, &[\"active\"])",
            "rh_json_get_path_key(&tab, &[], &String::from(\"active\"))",
        ],
        &[],
    ),
    (
        "rh_snapshot_tabs_map_key_active.rh",
        &[
            "rh_json_array_get(&tabs, ",
            "rh_json_get_path_key(&tab, &[], &String::from(\"active\"))",
        ],
        &["rh_json_string_path_index(&snapshot, &[\"tabs\"]"],
    ),
    (
        "rh_gui_window_control_visible_click.rh",
        &[
            "rh_child_window_control(&mut gui, 2104)",
            "rh_window_control_visible(&mut tabs_button)",
            "rh_window_control_click(&mut tabs_button)",
            "rh_child_window_message(&mut gui, ",
            "rh_child_window_pointer(&mut gui, ",
            "rh_child_window_rect(&mut gui, true)",
        ],
        &[
            "rh_host_eval_int(\"tabs_button.visible",
            "rh_host_eval_int(\"settings_button.click",
        ],
    ),
    (
        "rh_clipboard_get_set_text.rh",
        &[
            "rh_clipboard_get_text()",
            "rh_clipboard_set_text(&String::from(\"native-clipboard-probe\"))",
        ],
        &[
            "rh_host_eval_int(\"rh::clipboard::",
            "rh_host_eval_int(\"rhai::clipboard::",
        ],
    ),
    (
        "rh_host_api_json_task.rh",
        &[
            "rh_json_parse(",
            "std::thread::sleep(std::time::Duration::from_millis(",
        ],
        &[
            "rh_host_eval_int(\"rh::json::",
            "rh_host_eval_int(\"rhai::json::",
            "rh_host_eval_int(\"rh::task::",
            "rh_host_eval_int(\"rhai::task::",
        ],
    ),
    (
        "rh_runtime_atomic_write.rh",
        &["rh_atomic_write("],
        &["rh_host_eval_int(\"rh::runtime::atomic_write"],
    ),
    (
        "rh_crypto_sha256_file.rh",
        &["rh_sha256_file("],
        &["rh_host_eval_int(\"rh::crypto::sha256_file"],
    ),
    (
        "rh_legacy_rhai_json_parse.rh",
        &["rh_json_parse("],
        &["rh_host_eval_int(\"rhai::json::"],
    ),
    (
        "rh_bytes_from_array.rh",
        &["rh_bytes_from_array(&[])", "rh_bytes_from_array(&[0, 65, 255])"],
        &["rh_host_eval_int(\"rh::bytes::from_array"],
    ),
    (
        "rh_process_kill.rh",
        &["rh_process_kill(4242)"],
        &["rh_host_eval_int(\"std::process::kill"],
    ),
    (
        "rh_command_arg.rh",
        &["rh_command_arg(&mut command", "String::from(\"--probe\")"],
        &["rh_host_eval_int(\"command.arg"],
    ),
    (
        "rh_string_index_of.rh",
        &["rh_string_index_of(&haystack"],
        &["rh_host_eval_int(\"haystack.index_of"],
    ),
    (
        "rh_std_fs_write.rh",
        &["rh_std_fs_write(&rh_arg(0)"],
        &["rh_host_eval_int(\"std::fs::write"],
    ),
    (
        "rh_json_marker_run_properties.rh",
        &[
            "rh_json_string_path(&marker_run, &[\"text\"])",
            "rh_json_get_path(&marker_run, &[\"row\"])",
            "rh_json_get_path(&marker_run, &[\"column\"])",
        ],
        &["rh_host_eval_int(\"marker_run."],
    ),
    (
        "rh_bytes_append.rh",
        &[
            "rh_bytes_from_array(&[])",
            "rh_bytes_append(&mut framed",
            "rh_bytes_from_text(",
        ],
        &["rh_host_eval_int(\"framed.append"],
    ),
    (
        "rh_command_stdin_text.rh",
        &["rh_command_stdin_text(&mut worker", "String::from(\"hello\\n\")"],
        &["rh_host_eval_int(\"worker.stdin_text"],
    ),
    (
        "rh_child_stdout.rh",
        &["rh_child_stdout(&mut worker)"],
        &["rh_host_eval_int(\"worker.stdout"],
    ),
    (
        "rh_command_args_json.rh",
        &["rh_command_args(&mut command", "rh_json_string_argv(&arguments)"],
        &["rh_host_eval_int(\"command.args"],
    ),
    (
        "rh_json_stringify_pretty.rh",
        &["rh_json_stringify_pretty("],
        &["rh_host_eval_int(\"rh::json::stringify_pretty"],
    ),
    (
        "rh_hash_fnv1a64.rh",
        &["rh_hash_fnv1a64("],
        &["rh_host_eval_int(\"rh::hash::fnv1a64"],
    ),
    (
        "rh_env_has_get.rh",
        &["rh_env_has(", "rh_env_get("],
        &["rh_host_eval_int(\"std::env::has", "rh_host_eval_int(\"std::env::get"],
    ),
    (
        "rh_system_time_rfc3339.rh",
        &["rh_system_time_now_rfc3339("],
        &["rh_host_eval_int(\"std::time::SystemTime::now().rfc3339"],
    ),
];

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn read_fixture(name: &str) -> String {
    std::fs::read_to_string(fixture_dir().join(name))
        .unwrap_or_else(|error| panic!("read {name}: {error}"))
}

fn assert_native_or_host_eval(name: &str, needles: &[&str], anti_needles: &[&str]) {
    let source = read_fixture(name);
    check(&source).unwrap_or_else(|error| panic!("check {name}: {error}"));
    let output = transpile_cdylib_with_mode(&source)
        .unwrap_or_else(|error| panic!("transpile {name}: {error}"));
    assert!(
        matches!(
            output.execution_mode,
            CdylibExecutionMode::Native | CdylibExecutionMode::HostEval
        ),
        "{name}: expected native or host-eval, got {:?}\n{}",
        output.execution_mode,
        output.rust
    );
    assert!(
        output.execution_mode != CdylibExecutionMode::CompatDelegating,
        "{name} must not compat-delegate: {}",
        output.rust
    );
    for needle in needles {
        assert!(
            output.rust.contains(needle),
            "{name} missing {needle:?}: {}",
            output.rust
        );
    }
    for needle in anti_needles {
        assert!(
            !output.rust.contains(needle),
            "{name} must not contain {needle:?}: {}",
            output.rust
        );
    }
    assert!(
        !output.rust.contains("rh_host_run_script(RH_SCRIPT_SOURCE)"),
        "{name}: {}",
        output.rust
    );
}

#[test]
fn codegen_revision_is_eighty_three() {
    assert_eq!(RH_CODEGEN_REVISION, 83);
}

#[test]
fn crate_fixtures_compile() {
    for (name, _, _) in FIXTURES {
        let source = read_fixture(name);
        check(&source).unwrap_or_else(|error| panic!("check {name}: {error}"));
    }
}

#[test]
fn empty_map_fn_return_emits_native_json_not_set() {
    assert_native_or_host_eval(
        FIXTURES[0].0,
        FIXTURES[0].1,
        FIXTURES[0].2,
    );
}

#[test]
fn json_map_key_chain_via_locals_emits_native_path_key_writes() {
    assert_native_or_host_eval(
        FIXTURES[1].0,
        FIXTURES[1].1,
        FIXTURES[1].2,
    );
}

#[test]
fn array_index_property_via_local_emits_index_not_key_path() {
    assert_native_or_host_eval(
        FIXTURES[2].0,
        FIXTURES[2].1,
        FIXTURES[2].2,
    );
}

#[test]
fn null_unit_compare_emits_native_json_null_check() {
    assert_native_or_host_eval(
        FIXTURES[3].0,
        FIXTURES[3].1,
        FIXTURES[3].2,
    );
}

#[test]
fn snapshot_tabs_index_return_emits_path_index_not_string_index() {
    assert_native_or_host_eval(FIXTURES[4].0, FIXTURES[4].1, FIXTURES[4].2);
}

#[test]
fn working_context_dot_chain_via_locals_stays_native_without_extra_host_eval() {
    assert_native_or_host_eval(FIXTURES[5].0, FIXTURES[5].1, FIXTURES[5].2);
    let output = transpile_cdylib_with_mode(&read_fixture(FIXTURES[5].0))
        .expect("transpile working-context dot chain");
    assert_eq!(
        output.execution_mode,
        CdylibExecutionMode::Native,
        "{}",
        output.rust
    );
    assert_eq!(
        output.rust.matches("rh_host_eval_int(").count(),
        1,
        "{}",
        output.rust
    );
}

#[test]
fn tab_active_map_key_vs_dot_emits_native_key_and_field_reads() {
    assert_native_or_host_eval(FIXTURES[6].0, FIXTURES[6].1, FIXTURES[6].2);
}

#[test]
fn snapshot_tabs_map_key_active_emits_array_get_and_path_key_not_string_index() {
    assert_native_or_host_eval(FIXTURES[7].0, FIXTURES[7].1, FIXTURES[7].2);
}

#[test]
fn gui_window_control_visible_click_emits_native_gui_host_calls() {
    assert_native_or_host_eval(FIXTURES[8].0, FIXTURES[8].1, FIXTURES[8].2);
    let output = transpile_cdylib_with_mode(&read_fixture(FIXTURES[8].0))
        .expect("transpile gui window-control fixture");
    assert_eq!(
        output.execution_mode,
        CdylibExecutionMode::Native,
        "{}",
        output.rust
    );
    assert_eq!(
        output.rust.matches("rh_host_eval_int(").count(),
        1,
        "{}",
        output.rust
    );
}

#[test]
fn clipboard_get_set_text_emits_native_host_json_calls() {
    assert_native_or_host_eval(FIXTURES[9].0, FIXTURES[9].1, FIXTURES[9].2);
    let output = transpile_cdylib_with_mode(&read_fixture(FIXTURES[9].0))
        .expect("transpile clipboard fixture");
    assert_eq!(
        output.execution_mode,
        CdylibExecutionMode::Native,
        "{}",
        output.rust
    );
    assert_eq!(
        output.rust.matches("rh_host_eval_int(").count(),
        1,
        "{}",
        output.rust
    );
}

#[test]
fn rh_host_api_json_task_emits_native_json_and_sleep() {
    assert_native_or_host_eval(FIXTURES[10].0, FIXTURES[10].1, FIXTURES[10].2);
    let output = transpile_cdylib_with_mode(&read_fixture(FIXTURES[10].0))
        .expect("transpile rh host api json/task fixture");
    assert!(
        matches!(
            output.execution_mode,
            CdylibExecutionMode::Native | CdylibExecutionMode::HostEval
        ),
        "{}",
        output.rust
    );
    assert!(output.rust.contains("rh_json_parse("));
}

#[test]
fn runtime_atomic_write_emits_native_host_call() {
    assert_native_or_host_eval(FIXTURES[11].0, FIXTURES[11].1, FIXTURES[11].2);
    let output = transpile_cdylib_with_mode(&read_fixture(FIXTURES[11].0))
        .expect("transpile runtime atomic_write fixture");
    assert_eq!(
        output.execution_mode,
        CdylibExecutionMode::Native,
        "{}",
        output.rust
    );
    assert_eq!(
        output.rust.matches("rh_host_eval_int(").count(),
        1,
        "{}",
        output.rust
    );
}

#[test]
fn crypto_sha256_file_emits_native_host_call() {
    assert_native_or_host_eval(FIXTURES[12].0, FIXTURES[12].1, FIXTURES[12].2);
    let output = transpile_cdylib_with_mode(&read_fixture(FIXTURES[12].0))
        .expect("transpile crypto sha256_file fixture");
    assert_eq!(
        output.execution_mode,
        CdylibExecutionMode::Native,
        "{}",
        output.rust
    );
    assert_eq!(
        output.rust.matches("rh_host_eval_int(").count(),
        1,
        "{}",
        output.rust
    );
}

#[test]
fn legacy_rhai_json_parse_stays_native_via_dual_alias() {
    assert_native_or_host_eval(FIXTURES[13].0, FIXTURES[13].1, FIXTURES[13].2);
    let output = transpile_cdylib_with_mode(&read_fixture(FIXTURES[13].0))
        .expect("transpile legacy rhai::json::parse fixture");
    assert_eq!(
        output.execution_mode,
        CdylibExecutionMode::Native,
        "{}",
        output.rust
    );
    assert_eq!(
        output.rust.matches("rh_host_eval_int(").count(),
        1,
        "{}",
        output.rust
    );
}

#[test]
fn bytes_from_array_emits_native_bytes_constructors() {
    assert_native_or_host_eval(FIXTURES[14].0, FIXTURES[14].1, FIXTURES[14].2);
    let output = transpile_cdylib_with_mode(&read_fixture(FIXTURES[14].0))
        .expect("transpile bytes from_array fixture");
    assert_eq!(output.execution_mode, CdylibExecutionMode::Native, "{}", output.rust);
    assert_eq!(output.rust.matches("rh_host_eval_int(").count(), 1, "{}", output.rust);
}

#[test]
fn process_kill_emits_native_host_call() {
    assert_native_or_host_eval(FIXTURES[15].0, FIXTURES[15].1, FIXTURES[15].2);
    let output = transpile_cdylib_with_mode(&read_fixture(FIXTURES[15].0))
        .expect("transpile process kill fixture");
    assert_eq!(output.execution_mode, CdylibExecutionMode::Native, "{}", output.rust);
    assert_eq!(output.rust.matches("rh_host_eval_int(").count(), 1, "{}", output.rust);
}

#[test]
fn command_arg_emits_native_single_arg_append() {
    assert_native_or_host_eval(FIXTURES[16].0, FIXTURES[16].1, FIXTURES[16].2);
    let output = transpile_cdylib_with_mode(&read_fixture(FIXTURES[16].0))
        .expect("transpile command arg fixture");
    assert_eq!(output.execution_mode, CdylibExecutionMode::Native, "{}", output.rust);
    assert_eq!(output.rust.matches("rh_host_eval_int(").count(), 1, "{}", output.rust);
}

#[test]
fn string_index_of_emits_native_find_helper() {
    assert_native_or_host_eval(FIXTURES[17].0, FIXTURES[17].1, FIXTURES[17].2);
    let output = transpile_cdylib_with_mode(&read_fixture(FIXTURES[17].0))
        .expect("transpile string index_of fixture");
    assert_eq!(output.execution_mode, CdylibExecutionMode::Native, "{}", output.rust);
    assert_eq!(output.rust.matches("rh_host_eval_int(").count(), 1, "{}", output.rust);
}

#[test]
fn std_fs_write_emits_native_host_call() {
    assert_native_or_host_eval(FIXTURES[18].0, FIXTURES[18].1, FIXTURES[18].2);
    let output = transpile_cdylib_with_mode(&read_fixture(FIXTURES[18].0))
        .expect("transpile std fs write fixture");
    assert_eq!(output.execution_mode, CdylibExecutionMode::Native, "{}", output.rust);
    assert_eq!(output.rust.matches("rh_host_eval_int(").count(), 1, "{}", output.rust);
}

#[test]
fn json_marker_run_properties_emit_native_path_reads() {
    assert_native_or_host_eval(FIXTURES[19].0, FIXTURES[19].1, FIXTURES[19].2);
    let output = transpile_cdylib_with_mode(&read_fixture(FIXTURES[19].0))
        .expect("transpile json marker_run properties fixture");
    assert_eq!(output.execution_mode, CdylibExecutionMode::Native, "{}", output.rust);
    assert_eq!(output.rust.matches("rh_host_eval_int(").count(), 1, "{}", output.rust);
}

#[test]
fn bytes_append_emits_native_bytes_mutation() {
    assert_native_or_host_eval(FIXTURES[20].0, FIXTURES[20].1, FIXTURES[20].2);
    let output = transpile_cdylib_with_mode(&read_fixture(FIXTURES[20].0))
        .expect("transpile bytes append fixture");
    assert_eq!(output.execution_mode, CdylibExecutionMode::Native, "{}", output.rust);
    assert_eq!(output.rust.matches("rh_host_eval_int(").count(), 1, "{}", output.rust);
}

#[test]
fn command_stdin_text_emits_native_command_mutation() {
    assert_native_or_host_eval(FIXTURES[21].0, FIXTURES[21].1, FIXTURES[21].2);
    let output = transpile_cdylib_with_mode(&read_fixture(FIXTURES[21].0))
        .expect("transpile command stdin_text fixture");
    assert_eq!(output.execution_mode, CdylibExecutionMode::Native, "{}", output.rust);
    assert_eq!(output.rust.matches("rh_host_eval_int(").count(), 1, "{}", output.rust);
}

#[test]
fn child_stdout_emits_native_stream_access() {
    assert_native_or_host_eval(FIXTURES[22].0, FIXTURES[22].1, FIXTURES[22].2);
    let output = transpile_cdylib_with_mode(&read_fixture(FIXTURES[22].0))
        .expect("transpile child stdout fixture");
    assert_eq!(output.execution_mode, CdylibExecutionMode::Native, "{}", output.rust);
    assert_eq!(output.rust.matches("rh_host_eval_int(").count(), 1, "{}", output.rust);
}

#[test]
fn command_args_json_emits_native_argv_coercion() {
    assert_native_or_host_eval(FIXTURES[23].0, FIXTURES[23].1, FIXTURES[23].2);
    let output = transpile_cdylib_with_mode(&read_fixture(FIXTURES[23].0))
        .expect("transpile command args json fixture");
    assert_eq!(output.execution_mode, CdylibExecutionMode::Native, "{}", output.rust);
    assert_eq!(output.rust.matches("rh_host_eval_int(").count(), 1, "{}", output.rust);
}

#[test]
fn json_stringify_pretty_emits_native_host_call() {
    assert_native_or_host_eval(FIXTURES[24].0, FIXTURES[24].1, FIXTURES[24].2);
    let output = transpile_cdylib_with_mode(&read_fixture(FIXTURES[24].0))
        .expect("transpile json stringify_pretty fixture");
    assert_eq!(output.execution_mode, CdylibExecutionMode::Native, "{}", output.rust);
    assert_eq!(output.rust.matches("rh_host_eval_int(").count(), 1, "{}", output.rust);
}

#[test]
fn hash_fnv1a64_emits_native_host_call() {
    assert_native_or_host_eval(FIXTURES[25].0, FIXTURES[25].1, FIXTURES[25].2);
    let output = transpile_cdylib_with_mode(&read_fixture(FIXTURES[25].0))
        .expect("transpile hash fnv1a64 fixture");
    assert_eq!(output.execution_mode, CdylibExecutionMode::Native, "{}", output.rust);
    assert_eq!(output.rust.matches("rh_host_eval_int(").count(), 1, "{}", output.rust);
}

#[test]
fn env_has_get_emits_native_host_calls() {
    assert_native_or_host_eval(FIXTURES[26].0, FIXTURES[26].1, FIXTURES[26].2);
    let output = transpile_cdylib_with_mode(&read_fixture(FIXTURES[26].0))
        .expect("transpile env has/get fixture");
    assert_eq!(output.execution_mode, CdylibExecutionMode::Native, "{}", output.rust);
    assert_eq!(output.rust.matches("rh_host_eval_int(").count(), 1, "{}", output.rust);
}

#[test]
fn system_time_rfc3339_emits_native_host_call() {
    assert_native_or_host_eval(FIXTURES[27].0, FIXTURES[27].1, FIXTURES[27].2);
    let output = transpile_cdylib_with_mode(&read_fixture(FIXTURES[27].0))
        .expect("transpile system time rfc3339 fixture");
    assert_eq!(output.execution_mode, CdylibExecutionMode::Native, "{}", output.rust);
    assert_eq!(output.rust.matches("rh_host_eval_int(").count(), 1, "{}", output.rust);
}
