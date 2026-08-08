// AgenTerm Script Runtime v2 north-star task.
//
// Copy config.example.json to config.json and replace all local paths,
// loopback URL, and the stable test tab ID before running.

fn require(condition, message) {
    if !condition {
        throw message;
    }
}

require(args.len == 1, "daily_check_arguments: expected TARGET");
let target = args[0];
let config = rhai::json::parse(std::fs::read_to_string("config.json"));
require(config.schema_version == 1, "daily_check_config_version");
require(config.tab_id.starts_with("@"), "daily_check_tab_id");

let temp_root = rhai::runtime::temp_dir().display;
let marker = std::path::join(temp_root, "owned-目录.txt").display;
std::fs::write(marker, config.message);

let child_a_command = std::process::command(config.child_program);
child_a_command.args(config.child_a_args);
child_a_command.timeout(std::time::Duration::from_secs(3));
let child_a = child_a_command.start();
let child_a_pid = child_a.id;

let child_b_command = std::process::command(config.child_program);
child_b_command.args(config.child_b_args);
child_b_command.timeout(std::time::Duration::from_secs(3));
let child_b = child_b_command.start();
let child_b_pid = child_b.id;

let http_task = rhai::http::start("GET", config.loopback_url, #{
    proxy: false,
    timeout: std::time::Duration::from_secs(3),
    max_body_bytes: 65536
});

let child_a_output = child_a.wait_with_output();
let child_b_output = child_b.wait_with_output();
let http_response = http_task.wait(std::time::Duration::from_secs(3));
let http_body = http_response.body.collect(65536).to_text();

require(child_a_output.success, "daily_check_child_a_failed");
require(child_b_output.success, "daily_check_child_b_failed");
require(http_response.status >= 200 && http_response.status < 300,
    "daily_check_http_status");

let note = "daily-check:" + target + ":ok";
let receipt = fleet.tabs.set_note(config.tab_id, note);
require(receipt.post_state.verified, "daily_check_fleet_post_state");
require(receipt.events.len() >= 1, "daily_check_fleet_event");

let result = #{
    schema_version: 1,
    target: target,
    message: config.message,
    child_a: #{
        pid: child_a_pid,
        exit_code: child_a_output.exit_code,
        complete: child_a_output.complete,
        stdout: child_a_output.stdout_text()
    },
    child_b: #{
        pid: child_b_pid,
        exit_code: child_b_output.exit_code,
        complete: child_b_output.complete,
        stdout: child_b_output.stdout_text()
    },
    http: #{
        status: http_response.status,
        body: http_body,
        task_state: http_task.state
    },
    fleet: #{
        tab_id: config.tab_id,
        note: note,
        operation_id: receipt.operation_id,
        outcome: receipt.outcome,
        event_kind: receipt.events[0].kind,
        verified: receipt.post_state.verified
    },
    temp_root: temp_root
};

rhai::runtime::atomic_write(
    config.result_path,
    rhai::json::stringify_pretty(result)
);
result
