---
name: agenterm-shared-checkout
description: Work safely in the AgenTerm repository while other agents commit to the same checkout, and judge test-failure and "pre-existing flake" claims correctly. Use when dispatching subagents across platform swimlanes, when committing alongside concurrent agents, or when a test fails and its ownership is unclear.
---

# AgenTerm Shared Checkout

Several agents commit to this working tree at once — typically one per platform
swimlane (Windows, macOS, release chain). Their edits land in files you did not
touch, at moments you do not control.

## Commit rules

Stage explicit paths, always:

```bash
git add src/platform/adapters/unix/frontend/mod.rs
```

Never `git add -A`, `git add -u`, or `git add .` — they sweep in another agent's
in-flight work and attribute it to your commit message.

`cargo fmt` with no package scope reformats the whole workspace, including files
another agent is mid-edit. Scope it and check afterwards:

```bash
cargo fmt -p agenterm-platform
git status --short          # revert anything outside your domain
git checkout -- <path>
```

A `git commit` that reports `nothing to commit` right after a successful
`git add` usually means a concurrent agent wrote the index between the two
commands. Re-check `git status` before concluding your edits were lost — they
are normally still in the working tree.

After committing, verify scope before pushing:

```bash
git show --stat HEAD
```

## Dispatching subagents

Partition by **file domain**, not by topic. Two leaves that touch the same file
belong to one agent, run serially, no matter how separable they sound.

Give each subagent:

- its exclusive paths;
- an explicit forbidden list naming the files other agents hold right now;
- the pathspec/fmt/push rules above;
- which decisions are the user's, so it writes options up instead of choosing.

Tell agents not to push. Collect their local commits, verify each with
`git show --stat`, then push once as a coherent state.

## Judge failure claims before repeating them

"Not caused by my change" and "a random flake" are different conclusions.
A subagent that establishes the first and reports the second turns a **stable,
real failure** into noise that everyone walks past.

Distinguish them by measurement, not plausibility:

```bash
git checkout -q <commit>~1 && cargo test            # baseline, full suite
git checkout -q <commit>    && cargo test           # with the change
cargo test --test <name> <test_name>                # the test alone, at baseline
```

Four outcomes, four different actions:

| Baseline full | With change | Alone at baseline | Meaning |
|---|---|---|---|
| green | fail | — | the change caused it — fix it |
| green | fail | **fail** | pre-existing, and **stable** — find the real cause, name an owner |
| green | fail | pass | genuine cross-binary interference — worth investigating, not dismissing |
| fail | fail | fail | pre-existing red light on main |

A real example: `rhai_migration` failed with a change applied and passed in the
baseline full run, which looked like the change's fault. Run alone at baseline
it also failed. The actual cause was `prd_alignment_public_command_missing:delete-buffer`
— a command present in `src/commands.rs` but absent from the PRD catalog,
arriving via an earlier merge. Neither the change's fault nor a flake.

Read the assertion text. `cargo test` failure names in this repo usually state
the missing invariant outright.

## Verify subagent reports

Subagent reports are evidence, not conclusions. Spot-check the load-bearing
claim before repeating it to the user — especially "I verified X", "it was
already broken", and "nothing else changed". Re-run the one command that would
expose the claim if false; it is cheap next to reporting something untrue.
