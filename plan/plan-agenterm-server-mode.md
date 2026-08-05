# Plan: `agenterm server` authority entry

Status: **implemented on main** (2026-08-05).  
Product contract: [`prd/PRD_02_02_executable_family.md`](../prd/PRD_02_02_executable_family.md).  
Does not create a tag/Candidate/Release by itself.

## Outcome

User-facing authority entry is **`agenterm server`** (subcommand, separate
process). Windows still ships a thin **`agenterm-server(.exe)` image alias**
so the replaceable GUI PE is not locked by a long-lived authority mapping the
same path (v0.1.9 invariant). Kill the *third product brand*, not the
*second PE*.

Transitional: the short-lived `agenterm --server` flag remains accepted as an
alias and will be removed once no internal callers depend on it.

## Why not delete the PE in this leaf

```text
GUI maps agenterm.exe
Server maps agenterm-server.exe   ← distinct image → GUI upgrade/replace OK
```

If both processes map `agenterm.exe` (`agenterm server` spawned from
`current_exe`), Windows locks that image and `remote-ui-upgrade` / Keep Server
+ replace GUI regresses. Separate **process** ≠ separate **image**.

## Tree

```text
Outcome: agenterm server is the preferred authority entry
│
├─ A Entry / argv
│  ├─ A1 agenterm main: first token `server` → run_server_entry
│  ├─ A2 transitional `--server` flag alias still accepted
│  ├─ A3 configure_server_launch accepts remaining selectors only
│  └─ A4 agenterm-server bin remains thin alias → same entry
│
├─ B Spawn / discovery (Windows)
│  ├─ B1 autostart still launches sibling agenterm-server.exe
│  ├─ B2 ui_bridge keeps target_server_executable = agenterm-server.exe
│  └─ B3 docs: alias = image isolation, not a second product
│
├─ C Contract / docs
│  ├─ C1 PRD_02_02 executable family wording
│  ├─ C2 ARCHITECTURE bins table + README/AGENTS brief
│  └─ C3 plan note: full PE deletion deferred until upgrade story exists
│
└─ D Evidence
   ├─ D1 unit: `server` / `--server` token + selector parity
   └─ D2 local: cargo fmt + lib tests for touched modules
```

## Explicit non-goals

- Delete `agenterm-server` from `dist` / artifacts.json in this leaf
- Change Unix embedded GUI to split-process
- Raise release size budgets
- Merge mux/mcp/rhai
- Touch `server_app` Fleet ownership beyond argv branding

## Sequencing

1. Code A1–A4 + docs (single owner)
2. Unit tests
3. `cargo fmt` + `cargo test --lib` for `server_app`
4. Commit/push on `main`

## Success evidence

- `agenterm server --instance NAME` / `--endpoint` / `--address` work
- Unknown server argv → exit 2
- Thin `agenterm-server` still enters the same `run_server_entry`
- Autostart path still targets sibling `agenterm-server.exe` on Windows
- PRD states preferred entry + alias rationale

## Safe failure

- Missing sibling `agenterm-server.exe` on Windows autostart → existing
  NotFound error (unchanged)
- Unknown server argv → exit 2 (unchanged)
