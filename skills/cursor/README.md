# Cursor Cloud Agent skills

Repository-local playbooks for **this project's** Cloud Agent workflows. These
files are for agents and humans working in Cursor Cloud VMs — not product PRD.

| Skill | When to use |
|-------|-------------|
| [spawn-sibling-cloud-agent.md](spawn-sibling-cloud-agent.md) | Owner asks for a second Cloud Agent on cursor.com while the current session continues |

**Security:** never commit API keys, tokens, or full `CURSOR_*` secret values.
Skills use placeholders and environment variables only.
