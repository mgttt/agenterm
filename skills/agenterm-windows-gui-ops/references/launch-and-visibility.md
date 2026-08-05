# Launch and visibility recipes (desensitized)

## Prove session (redact names before publishing)

```powershell
query session
# Note the row marked current (">") — that is the interactive session id.
# Compare with:
Get-CimInstance Win32_Process -Filter "Name='agenterm.exe'" |
  Select-Object ProcessId, SessionId
Get-CimInstance Win32_Process -Filter "Name='CairoDesktop.exe'" |
  Select-Object ProcessId, SessionId
# explorer may be absent by design — never treat that as failure.
```

## Durable instance GUI (WMI fallback)

```powershell
$gui = Join-Path (Get-Location) 'dist\agenterm.exe'
$instance = 'dev'  # or work / main
$cmd = "cmd.exe /c set AGENTERM_INSTANCE=$instance&& start `"`" `"$gui`""
$r = ([wmiclass]'Win32_Process').Create($cmd)
# $r.ProcessId is the short-lived cmd; find agenterm by title:
Get-Process agenterm |
  Where-Object { $_.MainWindowTitle -match $instance } |
  Select-Object Id, SessionId, MainWindowHandle, MainWindowTitle
```

## Follow-up liveness (mandatory)

After the agent tool returns, run a **second** command:

```powershell
Get-Process agenterm | Select-Object Id, MainWindowTitle
# If the PID from the previous turn is missing, the Job ate the GUI.
```

## Attach verification

```powershell
$env:AGENTERM_INSTANCE = 'dev'
# or: $env:AGENTERM_IPC_ENDPOINT = 'pipe:\\.\pipe\agenterm-agt-v1-<hash>'
.\dist\agenterm-cli.exe ui-snapshot
# Expect server_pid == server-list PID for that instance; detached=false when GUI up.
```

## Restore without touching the shell

```powershell
# Product:
.\dist\agenterm-cli.exe --instance dev ui-action window-activate
# Win32 last resort: SW_RESTORE + on-screen SetWindowPos; optional brief TOPMOST.
```

## Clean stale registrations only

Instance dir (generic):

`%LOCALAPPDATA%\AgenTerm\instances\`

Remove JSON whose PID is dead and row is `stale` / test fixture. Do not delete
the live main/dev/work registration the user is using.
