// Parse the stage-0 bootstrap timing facts inherited by named Rhai tasks.

fn unavailable(reason) {
    #{
        schema_version: 1,
        kind: "agenterm-bootstrap-timing",
        state: "unavailable",
        reason: reason,
        setup_ms: (),
        cargo_build_ms: (),
        worker_copy_ms: (),
        other_setup_ms: (),
        clock_resolution_ms: (),
        cargo_lock_wait: #{
            state: "unavailable",
            duration_ms: (),
            included_in: ()
        }
    }
}

fn duration(name) {
    if !std::env::has(name) || std::env::get(name) == "" {
        throw "bootstrap_timing_missing:" + name;
    }
    let value = std::env::get(name).parse_int();
    if value < 0 {
        throw "bootstrap_timing_negative:" + name;
    }
    value
}

fn facts() {
    if !std::env::has("AGENTERM_BOOTSTRAP_TIMING_SCHEMA") {
        return unavailable("direct_worker_invocation");
    }
    try {
        if std::env::get("AGENTERM_BOOTSTRAP_TIMING_SCHEMA") != "1" {
            throw "bootstrap_timing_schema";
        }
        let setup_ms = duration("AGENTERM_BOOTSTRAP_SETUP_MS");
        let cargo_build_ms = duration("AGENTERM_BOOTSTRAP_CARGO_BUILD_MS");
        let worker_copy_ms = duration("AGENTERM_BOOTSTRAP_WORKER_COPY_MS");
        let other_setup_ms = duration("AGENTERM_BOOTSTRAP_OTHER_SETUP_MS");
        let resolution_ms = duration("AGENTERM_BOOTSTRAP_CLOCK_RESOLUTION_MS");
        if setup_ms + resolution_ms <
                cargo_build_ms + worker_copy_ms + other_setup_ms {
            throw "bootstrap_timing_phase_sum";
        }
        let lock_state = std::env::get("AGENTERM_BOOTSTRAP_LOCK_WAIT_STATE");
        if lock_state != "included_not_separable" {
            throw "bootstrap_timing_lock_state";
        }
        return #{
            schema_version: 1,
            kind: "agenterm-bootstrap-timing",
            state: "measured",
            reason: (),
            setup_ms: setup_ms,
            cargo_build_ms: cargo_build_ms,
            worker_copy_ms: worker_copy_ms,
            other_setup_ms: other_setup_ms,
            clock_resolution_ms: resolution_ms,
            cargo_lock_wait: #{
                state: lock_state,
                duration_ms: (),
                included_in: "cargo_build_ms"
            }
        };
    } catch (error) {
        unavailable("invalid_environment:" + error.to_string())
    }
}

fn wall_time(bootstrap, task_ms) {
    if bootstrap.state == "measured" {
        return #{
            state: "partial",
            task_ms: task_ms,
            accounted_ms: bootstrap.setup_ms + task_ms,
            accounted_scope:
                "bootstrap-timing-start-through-worker-setup plus timed-task-body",
            unmeasured_intervals: [
                "pre-timing-clock-probe",
                "worker-start-handoff",
                "post-task-bootstrap-cleanup"
            ]
        };
    }
    #{
        state: "unavailable",
        task_ms: task_ms,
        accounted_ms: task_ms,
        accounted_scope: "timed-task-body-only",
        unmeasured_intervals: ["entire-bootstrap"]
    }
}
