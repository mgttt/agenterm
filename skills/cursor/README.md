# Cursor Cloud Agent skills

Repository-local playbooks for **this project's** Cloud Agent workflows. These
files are for agents and humans working in Cursor Cloud VMs — not product PRD.

| Skill | When to use |
|-------|-------------|
| [spawn-sibling-cloud-agent.md](spawn-sibling-cloud-agent.md) | Owner asks for a second Cloud Agent on cursor.com while the current session continues |
| [session-registry.md](session-registry.md) | Track 主控 vs 分身1/2… display names, bcIds, and roles (API cannot rename agents) |
| [inter-agent-comms.md](inter-agent-comms.md) | Fleet coordination notes (git mailbox optional; API chat is primary wake path) |
| [mailbox.md](mailbox.md) | Optional shared board for durable status (not a substitute for agent chat) |

**Agent chat CLI:** `scripts/cursor_agent_chat.sh --from <谁> --to <谁> '正文'`
silently wraps `<from::…><to::…>` and POSTs to Cursor Cloud Agents API.
See `--help`. Requires `CURSOR_API` (never logged).

**Security:** never commit API keys, tokens, or full `CURSOR_*` secret values.
Skills use placeholders and environment variables only.
