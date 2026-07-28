# Script Runtime daily check

This is the v0.1.9 north-star project for the public `agenterm-script.exe`
entry point. It combines an invocation-owned temporary directory, two
concurrent argv-safe child processes, a loopback HTTP task, JSON aggregation,
a typed Fleet tab-note mutation, verified receipt/event/post-state, atomic
result publication, and automatic resource cleanup.

Copy `config.example.json` to `config.json`, use an isolated AgenTerm server
and stable tab ID, then run:

```powershell
agenterm-script.exe check daily-check.rhai --profile local --project-root .
agenterm-script.exe task list --json
agenterm-script.exe task show daily-check --json
agenterm-script.exe task run daily-check --timeout-ms 10000 --json -- smoke
```

The example intentionally expects a loopback fixture. It does not contact a
public network service.
