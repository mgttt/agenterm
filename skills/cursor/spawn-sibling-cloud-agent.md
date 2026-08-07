# Spawn a sibling Cloud Agent (cursor.com)

Use when the owner wants a **second Cloud Agent session** on the same repo
while the current chat keeps running — for example Win full-gate work parallel
to Linux/planning in another session.

## Preconditions

- **Auth** (either):
  - Cloud Agent VM: **`CURSOR_API`** injected (length ~69 chars; **never log or
    commit the value**), or
  - Local host: `CURSOR_API` env **or** `~/env.jsonl` line with `CURSOR_API.api_key`
    (see [cloud.ts](cloud.ts)).
- This repo is wired to a Cursor **cloud environment** (discover name via MCP,
  not hardcoded in scripts).
- Creating agents uses the **REST API**; the bundled `cursor-cloud` MCP tools
  are **read/monitor only** (no create endpoint). Preferred wrapper:
  `bun skills/cursor/cloud.ts …`.

## Discover this run (no secrets)

```text
# Preferred cross-host CLI (env or ~/env.jsonl):
bun skills/cursor/cloud.ts me
bun skills/cursor/cloud.ts list --active --env mgttt/agenterm
bun skills/cursor/cloud.ts get 主控1

# On a cloud VM with cursor-cloud MCP:
cursor-cloud MCP → environment-info / run-info / list-cloud-agents
```

## Create a sibling agent (desensitized template)

When spawning from **主控**, set API `"name"` to `分身1` / `分身2` / … up front — the API
does not support renaming an existing agent (`PATCH` returns 404). Update
[session-registry.md](session-registry.md) after each spawn.

**Endpoint:** `POST https://api.cursor.com/v1/agents`

**Auth:** HTTP Basic — username = API key from `CURSOR_API`, password empty:

```bash
curl -sS --request POST \
  --url 'https://api.cursor.com/v1/agents' \
  -u "${CURSOR_API}:" \
  --header 'Content-Type: application/json' \
  --data '{
    "name": "short-task-label",
    "prompt": {
      "text": "Task brief: pull origin main, scope, constraints, evidence to return."
    },
    "model": { "id": "composer-2.5" },
    "env": {
      "type": "cloud",
      "name": "<cloud-environment-name-from-environment-info>"
    },
    "autoCreatePR": false
  }'
```

**Alternative (explicit repo instead of named env):**

```json
"repos": [
  {
    "url": "https://github.com/<org>/<repo>",
    "startingRef": "main"
  }
],
"autoCreatePR": false
```

**Response fields to surface to the owner:**

- `agent.url` → open on cursor.com
- `agent.id` → `bc-…` (public run id, safe to share in chat)
- `run.id` → `run-…`
- `agent.status`, `run.status`

Official reference: [Cloud Agents API — Create An Agent](https://cursor.com/docs/cloud-agent/api/endpoints)

## Prompt hygiene (desensitize)

- Do **not** paste `CURSOR_API`, GitHub tokens, MCP bearer tokens, or
  `envVars` values into `prompt.text`.
- Do **not** put secrets in git commits, PR bodies, or skill files.
- `envVars` names cannot start with `CURSOR_` (API rejects / ignores per docs).
- State repo state in generic terms (“latest `main`”, “open PR #N”) without
  copying dashboard cookies or internal tokens.

## Branch and PR defaults for parallel siblings

| Field | Recommended | Why |
|-------|-------------|-----|
| `autoCreatePR` | `false` | Owner merges deliberately; avoids duplicate PRs |
| `workOnCurrentBranch` | default `false` | Sibling uses `cursor/<name>-<suffix>` branch |
| `startingRef` | `main` | Single integration point; sibling should `git pull` first |

Align branch naming with cloud agent policy: `cursor/<descriptive-name>-<suffix>`.

## Monitor after spawn

1. Give the owner the `agent.url` immediately.
2. Poll with `cursor-cloud` → `list-cloud-agents` or `batch-fetch-details` if
   you need events/transcripts (large transcripts: use subagents, not raw read).
3. Do **not** assume the sibling shares this chat context — put full handoff
   in the API `prompt.text`.
4. Ongoing 主控↔分身 chat: use the desensitized helper
   `scripts/cursor_agent_chat.sh --from 主控 --to 分身N '…'`
   (auto envelope `<from::…><to::…>`; never prints `CURSOR_API`).

## Example handoff text (copy pattern, customize)

Always embed the [fleet-awareness](fleet-awareness.md) skeleton so the sibling
is not a silo. For 主控换防 use [hand-off-controller.md](hand-off-controller.md).

```text
你是舰队一员【分身N】。遵守 skills/cursor/fleet-awareness.md + inter-agent-comms.md。
Repo: mgttt/agenterm. Base: origin/main.

每轮：git pull → 读 session-registry + mailbox 全文（感知同伴）→ 更新本席位 → 再干活。
显示名：分身N。文件所有权：…（独占）。禁止：…。

Done elsewhere: <merged tips / plans — no secrets>.

Your scope:
1. …
2. Small commits on cursor/<branch>-*; 合 main 仅主控审合 unless 明确授权
3. 回报：状态/分支/tip/文件清单/证据/阻塞/下一步；IDLE 写「无新指令不开工」
4. 唤醒：scripts/cursor_agent_chat.sh --from 分身N --to 主控2 …

Constraints: follow AGENTS.md; <version/tag rules>.
```

## What this session validated (2026-07-30)

- Sibling create succeeded with `env.type=cloud` + environment name from
  `environment-info`; API also attached `repos[].startingRef=main`.
- Same principal can run **multiple** agents; list via `list-cloud-agents`.
- Parallel Win gate + planning session is viable; merge conflicts are avoided
  by rebasing siblings onto latest `main` before long GUI work.

## Failure modes

| Symptom | Check |
|---------|--------|
| `401` / auth error | `CURSOR_API` unset or wrong; user must set Dashboard API key on environment |
| `409 agent_id_conflict` | Idempotent re-create with same `agentId` — omit `agentId` for new runs |
| Sibling “knows nothing” | Expected — only `prompt.text` is initial context |
| MCP cannot create | Use REST API; MCP is diagnostics only |
