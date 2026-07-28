import "modules/value" as value;

#{
    answer: value::answer,
    args: args,
    cwd: std::env::current_dir().display,
    environment: std::env::get("AGENTERM_TASK_FIXTURE")
}
