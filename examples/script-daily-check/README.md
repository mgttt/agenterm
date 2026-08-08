# Script Runtime daily check

This is the v0.1.9 north-star project for the public `agenterm-rhai.exe`
entry point. It combines an invocation-owned temporary directory, two
concurrent argv-safe child processes, a loopback HTTP task, JSON aggregation,
a typed Fleet tab-note mutation, verified receipt/event/post-state, atomic
result publication, and automatic resource cleanup.

Copy `config.example.json` to `config.json`, use an isolated AgenTerm server
and stable tab ID, then run:

```powershell
agenterm-rhai.exe check daily-check.rh --profile local --project-root .
agenterm-rhai.exe task list --json
agenterm-rhai.exe task show daily-check --json
agenterm-rhai.exe task run daily-check --timeout-ms 10000 --json -- smoke
```

The example intentionally expects a loopback fixture. It does not contact a
public network service.
