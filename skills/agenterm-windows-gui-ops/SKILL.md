---
name: agenterm-windows-gui-ops
description: >
  Launch, attach, diagnose, and clean AgenTerm Windows GUI/server instances without
  killing work or lying about visibility. Use when the user cannot see AgenTerm
  windows, GUI dies after agent launch, multi-instance main/dev/work attach is
  needed, dual/stale servers must be cleaned, close-confirm is missing, or the
  desktop is Cairo (or any non-explorer shell). Triggers: "看不到窗口", "open GUI",
  "attach instance", "server-list cleanup", RDP multi-session, job-killed GUI.
---

# AgenTerm Windows GUI operations

Operational skill for **native Windows** hosts. Prefer `dist\agenterm.exe` and
`dist\agenterm-cli.exe` after a successful local build. Never invent a second
IPC authority when a live peer already holds the session.

## Hard rules (from production incidents)

1. **Do not assume `explorer.exe`.** Users may run **Cairo Desktop** or another
   shell. Starting explorer in their session is hostile: it fights their shell,
   steals the desktop, and does not fix AgenTerm. Detect shell only for
   diagnosis; never "fix" missing explorer by launching it unless the user
   explicitly asks.
2. **Agent Jobs kill child GUIs.** Processes started with plain `Start-Process`
   or `Process.Start` from an agent shell often **die when the tool turn ends**.
   IPC may look healthy for a few seconds (`ui-snapshot` shows a client), then
   the GUI PID is **DEAD**. That is not "user cannot see"; that is **launch
   non-durability**.
3. **`ui-snapshot` can lie after client death.** Server may keep a stale
   replaceable UI registration (`visible: true`, old `client_pid`) while the
   process is gone. Always corroborate with `Get-Process` / `Win32_Process` and
   a non-zero `MainWindowHandle`.
4. **Prove session identity before arguing about visibility.** Multi-user RDP
   is common. Windows only appear in the session that owns them. Compare
   agent shell `SessionId`, GUI `SessionId`, and `query session` (desensitize
   usernames in logs you paste into plans/skills).
5. **Attach, do not mint a second `main`.** Prefer live peer discovery / instance
   pin. A second server + workspace restore **looks like session reset** (empty
   shells, agent tabs "gone").
6. **Desensitize everything persisted.** No real usernames, hostnames, RDP
   client names, home paths, or raw customer workspace paths in skills, plans,
   or chat dumps destined for the repo. Use placeholders: `<user>`,
   `<session-id>`, `pipe:\\.\pipe\agenterm-agt-v1-<hash>`.

## Durable GUI launch (Windows)

Goal: GUI **survives** after the agent command exits and lands on the **same
interactive session** as the human.

### Preferred: breakaway CreateProcess

Spawn `agenterm.exe` with `CREATE_BREAKAWAY_FROM_JOB` (and usually
`CREATE_NEW_PROCESS_GROUP` + Unicode environment). Set:

| Env | Purpose |
|-----|---------|
| `AGENTERM_INSTANCE` | `main` / `dev` / `work` / custom logical name |
| clear `AGENTERM_IPC_ENDPOINT` / `AGENTERM_IPC_ADDRESS` | Let instance resolution win unless intentionally pinning |
| clear `AGENTERM_NO_ACTIVATE` | Human must **see** the window; smoke tests keep `AGENTERM_NO_ACTIVATE=1` |

If breakaway is denied (`ERROR_ACCESS_DENIED`), fall back without claiming
failure of the product: use an **out-of-job** launcher (below).

### Fallback: WMI / `cmd start` (out of job)

```text
Win32_Process.Create(
  cmd.exe /c set AGENTERM_INSTANCE=<name>&& start "" "<repo>\dist\agenterm.exe"
)
```

- The **cmd** PID exits quickly; the real GUI is a **child** `agenterm.exe`.
- Resolve the GUI by title (`AgenTerm * — <instance>`) or by matching
  `ui-snapshot.client_pid` **and** process liveness.
- Do **not** treat the WMI launcher PID as the GUI.

### Forbidden "fixes"

- Starting `explorer.exe` because it is missing.
- Claiming success from `ui-snapshot` alone without a live process + HWND.
- Spawning a new server while `server-list` already shows a **running** peer
  for that logical instance (unless the user wants a clean slate and accepts
  tab loss).

## Visibility checklist (human still says "I see nothing")

Run in order; stop when the human confirms:

1. **Alive?** `agenterm.exe` PID exists; not only server.
2. **Same session?** GUI `SessionId` equals the human interactive session
   (the session with their shell / Cairo / active RDP row marked current).
3. **HWND?** `MainWindowHandle != 0`. If zero, window not created yet or headless failure — capture stderr from a non-redirected durable launch.
4. **Iconic?** If minimized, `ShowWindow(SW_RESTORE)` / product
   `ui-action window-activate` (pin endpoint or instance first).
5. **On-screen rect?** `GetWindowRect` inside the virtual screen; if off-screen,
   `SetWindowPos` onto primary work area.
6. **Z-order?** Optional short `HWND_TOPMOST` then normal — helps under busy
   Cairo stacks; do not leave permanent topmost unless asked.
7. **Shell context:** Note Cairo/other shell for the report; **do not** replace
   their shell.

Product path for focus (preferred over raw user32 when UI client is connected):

```text
agenterm-cli --instance <name> ui-action window-activate
```

or pin `AGENTERM_IPC_ENDPOINT` to the live pipe from `server-list`.

## Attach to existing server (dev / work / main)

```text
# List authorities (desensitize before pasting into repo docs)
agenterm-cli server-list
agenterm-cli server-list --prune

# GUI attach via instance env (durable launch recipe above)
AGENTERM_INSTANCE=dev  -> dist\agenterm.exe
AGENTERM_INSTANCE=work -> dist\agenterm.exe

# CLI pin without changing defaults permanently
set AGENTERM_IPC_ENDPOINT=pipe:\\.\pipe\agenterm-agt-v1-<hash>
agenterm-cli list-windows -F "#{window_id}:#{window_name}"
agenterm-cli ui-snapshot
```

Success criteria:

- `ui-snapshot.server_pid` equals the existing server PID for that instance.
- Tab titles the user cares about are still present (not only fresh `cmd.exe`
  from workspace fake-restore).
- GUI process still alive **after** the agent turn ends (re-check with a second
  command).

## Server hygiene (stale / dual main)

Classify each `server-list` row:

| Kind | Action |
|------|--------|
| `stale`, 0 tabs, dead PID | Delete registration JSON under the instance dir; do not kill random PIDs |
| Test leftovers (`p0-*`, temp workspace under system temp) | Kill if live, remove registration |
| **Dual `main`** (two running mains) | Inspect tabs on **both** endpoints before kill. Keep the one with live agent work; stop the other with pinned `kill-server` / `shutdown` |
| Orphan `agenterm-server` not in `server-list` | Treat as unregistered; confirm no useful tabs via accidental IPC, then stop process |
| Intentional-shutdown markers for **live** endpoints | Remove wrong markers; they confuse recovery |

Pin before mutate:

```text
set AGENTERM_IPC_ENDPOINT=pipe:\\.\pipe\agenterm-agt-v1-<hash>
agenterm-cli list-windows -F "#{window_id}:#{window_name}"
agenterm-cli kill-server
```

Never use PowerShell automatic `$PID` / `$Args` as loop variables (read-only /
reserved); they silently break kill/enum scripts.

## Close confirmation (Windows GUI)

If title-bar close or taskbar close skips Keep/Stop/Cancel:

- Title-bar / Alt+F4 may arrive as `SC_CLOSE`; product must route to the same
  path as `WM_CLOSE` → window-close dialog.
- Minimized close must **restore before** laying out native Keep/Stop/Cancel
  buttons (iconic client rect parks controls off-surface).
- Second close while dialog open must re-assert visibility/geometry.
- Black-box: isolated instance, durable GUI, then
  `ui-action close-window` → `wait-ui --modal-kind confirm-window-close`.

## Build note when servers hold file locks

Live `agenterm-server.exe` can lock `dist` / `target` outputs. Prefer:

```text
cargo build --bin agenterm --bin agenterm-cli
copy into dist\
```

Do not kill the user's agent-bearing server just to complete a full package
build unless they approve.

## Evidence standard (before claiming "done")

Report only what was re-checked **after** the launch command finished:

- GUI PIDs + session IDs + window titles (redact user/machine).
- `server_pid` match for attach cases.
- Process still alive on a **follow-up** probe.
- Human confirmation when the ticket was "I cannot see the window".

If the human cannot see the window, do not argue from IPC alone. Re-enter the
visibility checklist; fix launch durability first.

## Related product code (orientation, not ownership map)

- Peer attach / refuse second server: `src/frontend_server.rs`, `src/instances.rs`, CLI pin in `src/client/mod.rs`
- Window close present: `src/platform/adapters/windows/remote_frontend.rs`, `SC_CLOSE` in platform `control_window`
- Process breakaway spawn: `crates/agenterm-platform` process_spawn (server autostart); GUI agent launches must still break away at the **agent** boundary

## Desensitization checklist for commits

- Replace account names with `<user>` / `wjc2022_*` instance labels only if already public product instance names (`main`/`dev`/`work`).
- Replace host/RDP client with `<host>` / `<rdp-client>`.
- Replace absolute user profile paths with `%LOCALAPPDATA%\AgenTerm\...` or `<workspace>`.
- Keep pipe hashes truncated if needed: `agt-v1-<12-hex>…`.
- No screenshots of personal terminal contents in the skill tree.
