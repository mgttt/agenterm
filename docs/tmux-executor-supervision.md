# Tmux executor supervision (condensed)

How a **supervisor** session keeps a **named tmux executor** moving on this
repo, and how that executor must orchestrate **background subagents**. This is
not architecture SSOT (`plan/ARCHITECTURE.md`) and not product PRD. Redact
every write the same way as `Agents.md` (repo-relative or `~/...`; no host-home
absolutes, no mail, no tokens, no personal host names).

Use this after a run, not instead of the owning PRD / goal file.

---

## 1. Roles

| Role | Lives in | Owns | Must not |
|------|----------|------|----------|
| **Supervisor** | a separate agent session | brief, 15-minute nudge, work log, prompt revision | implement product leaves; compete for `CARGO_TARGET_DIR`; broad `git add` |
| **Executor** | one named tmux window (coding agent, full workspace access) | DAG, spawn, integrate, pathspec commit → rebase → push | idle with 0 workers while NOT-DONE boxes are open; mark leftovers “later” |
| **Subagent** | executor-spawned, background | one exclusive file set + owning tests | commit, push, rebase, touch hot files, share a target dir |

Hot/shared (executor parent only, serial): `src/lib.rs`, `PRD.md`, workspace
`Cargo.toml`, `Agents.md`, `prd/alignment-contract.json`.

---

## 2. Settled loop

```text
supervisor                  executor                     subagents
   |                           |                              |
   |-- write session brief --> |                              |
   |   (outside the tree)      |-- same-turn spawn 2-4 ------>|
   |                           |   exclusive paths + tests    |
   |                           |<-- unstaged handoff ---------|
   |                           |-- review + owning tests      |
   |                           |-- pathspec commit            |
   |                           |-- git pull --rebase origin main
   |                           |-- git push origin main       |
   |                           |-- refill workers if boxes open
   |-- every 15 min: capture --|                              |
   |   count workers           |                              |
   |   nudge (short)           |                              |
   |-- append 6-line log       |                              |
```

Session scratch (not committed): a brief file and a supervisor log under a
temp or `~/...` ops path. Durable lessons land here or in `Agents.md`.

---

## 3. Executor protocol (paste-ready)

Keep the **pane message short**. Put the long spec in the brief. The executor
mostly obeys the **latest** pane line.

1. Work only in this clone (`main`). Do not edit sibling repos.
2. You are **orchestrator**. Independent leaves are not implemented on the
   parent “because it is faster”.
3. Same turn: spawn **2–4** background subagents. Each prompt has: leaf name,
   exclusive write paths, “do not touch …”, tests to leave green, “do not
   commit”, own `CARGO_TARGET_DIR`, product hard-no list.
4. **Live workers ≥ 2** while ≥ 2 free leaves exist. When one returns:
   integrate → test → pathspec commit → `git pull --rebase origin main` →
   `git push` → **refill**. Do not drain to 0 and write “done”.
5. Ship **by progress**, not by mood. One green exclusive slice = one commit.
   This repo rebases; do not merge unless rebase is impossible (say why).
   Subagents never commit.
6. Grow **owning tests** with the slice. New `.rh`: `mode_probe` must print
   `mode=native host_eval_int=0`. Chassis crate: `cargo test --locked -p
   agenterm-chassis` (never whole workspace).
7. Status line every nudge: `workers=N leaves=… sha=… blocked=…` (external
   block only).

### NOT-DONE (example shape — replace boxes per goal)

Idle at the composer with 0 workers is allowed only when every box is checked
**or** there is a real external block (human Candidate SHA, Promotion, missing
credential you must not mint).

Writing “留待后续 / later / 阻塞：无” on an **open** box is a protocol fail.

---

## 4. Supervisor nudge

Interval: 15 minutes. Capture the pane first. Then:

| Seen | Nudge |
|------|--------|
| Idle, 0 workers, open boxes | Hard: spawn 2–4 **this turn**; name the open boxes; forbid “later” |
| Parent editing exclusive files, 0 workers | Same hard nudge |
| ≥2 workers in flight, parent waiting | Light: refill on return; commit-rebase-push green slices |
| Asking a question / waiting for you | Answer in one line or tell it to assume and move |
| Pane not the coding agent | Report; do not invent a restart unless a shell prompt is visible |

Do not send a second novel-length brief. Point at the brief section name.

---

## 5. What worked vs what did not

**Keep**

- Exclusive file table + “do not commit” + per-leaf `CARGO_TARGET_DIR`
- Counting workers every 15 minutes; idle+0 = fail
- Mandatory `workers + leaves + sha` in the reply
- Progress push (small pathspec commits actually reached `origin/main`)
- Owning tests as the leaf exit gate
- Isolated target dirs for leaf `cargo test` / clippy

**Drop / invert**

- Soft “use parallel thinking” with no same-turn spawn
- Planning `TaskCreate(L1/L2/L3)` then implementing all three on the parent
- Long brief as the only live instruction
- Allowing “no blocker; X left for later” as a completion status
- Waiting for `./lint.sh` or a full rh corpus check before spawning the next leaf
- Parent re-running `mode_probe` / crate tests a leaf already owns
- Two heavy Cargo/rh jobs sharing the default package cache at once (leaf
  isolation must include target dir **and** not overlapping a parent lint)

---

## 6. Prompt revision loop

After each nudge (or at end of day), append six lines to the **session** log
(outside the tree):

```text
time: <ISO date only, no host>
workers: <n>  sha: <short>  mode: idle|serial|orchestrating
useful: <one phrase>
useless: <one phrase>
tweak: <one phrase or none>
open-boxes: <ids>
```

When a tweak repeats twice, patch the brief (and this file if it is now
settled). Do not copy pane dumps, mail, account names, or absolute home paths
into the tree. Session SHAs may stay in the scratch log; this playbook keeps
only patterns.

---

## 7. Redaction checklist (before any commit)

```bash
# from repository root
./scripts/doc-redact-check.sh docs/tmux-executor-supervision.md
```

Also strip: personal mailbox, pane/window names that embed a person, sibling
repo names that are not this product, LAN IPs, tokens, expanded `pwd`. Prefer
`<executor>`, `<supervisor>`, `~/...`, `plan/...`.
