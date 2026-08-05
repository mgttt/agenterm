# Cursor Cloud Agent skills

Repository-local playbooks for **this project's** Cloud Agent workflows. These
files are for agents and humans working in Cursor Cloud VMs — not product PRD.

| Skill | When to use |
|-------|-------------|
| [fleet-awareness.md](fleet-awareness.md) | **Organic fleet**: every seat keeps peer situational awareness for the whole lifecycle |
| [fleet-duty.md](fleet-duty.md) | **Auto-dream / 值班**: periodic proactive scan; Cursor Automations cron prompt |
| [hand-off-controller.md](hand-off-controller.md) | Hand control to a new 主控N session |
| [spawn-sibling-cloud-agent.md](spawn-sibling-cloud-agent.md) | Owner asks for a second Cloud Agent on cursor.com while the current session continues |
| [session-registry.md](session-registry.md) | Track 主控 vs 分身1/2… display names, bcIds, and roles (API cannot rename agents) |
| [inter-agent-comms.md](inter-agent-comms.md) | Day-to-day wake / mailbox / 请示 checklist |
| [mailbox.md](mailbox.md) | Durable shared board (facts, seats, 请示) |

**Agent chat CLI:** `scripts/cursor_agent_chat.sh --from <谁> --to <谁> '正文'`
silently wraps `<from::…><to::…>`, injects `<fleet-pulse>` by default, and
POSTs to Cursor Cloud Agents API. Defaults to **wait/retry** on `409 agent_busy`.
`--no-fleet-context` / `--no-wait` / `--wait-timeout SEC` override. See `--help`.

**Fleet pulse (local):** `scripts/cursor_agent_fleet_pulse.sh` prints the same
digest chat injects (registry + mailbox + optional live API). Requires
`CURSOR_API` only for the live merge; git-only mode always works.

**Fleet duty / auto-dream:** `scripts/cursor_agent_fleet_duty.sh` scans unmerged
`cursor/*` branches, stale live agents, and open 请示. `--apply --from 主控2`
sends nudge chats. Schedule via Cursor Automations using the prompt in
[fleet-duty.md](fleet-duty.md) — agents cannot create Automations cron themselves.

**Security:** never commit API keys, tokens, or full `CURSOR_*` secret values.
Skills use placeholders and environment variables only.
