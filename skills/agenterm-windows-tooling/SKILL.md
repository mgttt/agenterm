---
name: agenterm-windows-tooling
description: Install CLI tools on Windows and make them immediately available in new terminals without logout. Avoid the PATH propagation traps that waste time.
---

# Windows tooling: install once, available everywhere

Installing a CLI tool on Windows and making it available in **new terminals**
(not just the current shell) is not trivial. Three approaches were tried in
order; only one works immediately.

## Proven approach: symlink into an already-active PATH directory

```powershell
New-Item -ItemType SymbolicLink -Path "$env:USERPROFILE\.local\bin\<tool>.exe" `
    -Target "$env:LOCALAPPDATA\Programs\<tool>\bin\<tool>.exe" -Force
```

**Why this works immediately:** `~/.local/bin` is already in the PATH and the
current shell environment block already knows about it. No registry write, no
broadcast, no logout.

If the tool updates via its own installer, the symlink follows the updated
binary automatically.

## Failed approaches (do not repeat)

| Approach | Mechanism | Result |
|----------|-----------|--------|
| `[Environment]::SetEnvironmentVariable("Path", ..., "User")` | .NET API writes registry | Does **not** broadcast `WM_SETTINGCHANGE`; new cmd windows don't pick it up |
| `setx Path "..."` | Writes registry + sends `WM_SETTINGCHANGE` | Broadcast unreliable; running shells cache old PATH; 1024-char truncation risk |

Neither approach makes the tool available in a new terminal without a
logout/login cycle. Never ask the user to log out.

## If the tool is an MSI

Download and install silently:

```powershell
Invoke-WebRequest -Uri "<download-url>" -OutFile "$env:TEMP\<tool>.msi"
msiexec /i "$env:TEMP\<tool>.msi" /quiet /norestart
```

Then create the symlink as above.

## If the tool is a standalone .exe

Drop it directly into `~/.local/bin` — no symlink needed:

```powershell
Invoke-WebRequest -Uri "<download-url>" -OutFile "$env:USERPROFILE\.local\bin\<tool>.exe"
```
