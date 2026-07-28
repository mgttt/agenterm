param(
    [string]$Exe = (Join-Path $PSScriptRoot '..\dist\agenterm-cli.exe'),
    [switch]$ListEvidence
)

$ErrorActionPreference = 'Stop'
$evidence = @(
    'working-context.proxy-cmd-applied'
    'working-context.proxy-child-inheritance'
    'working-context.proxy-clear'
    'working-context.proxy-powershell-applied'
    'working-context.proxy-noninteractive-rejected'
)
if ($ListEvidence) {
    $evidence
    exit 0
}

function Write-Evidence {
    param([Parameter(Mandatory = $true)][string]$Id)
    if ($evidence -notcontains $Id) {
        throw "proxy smoke emitted undeclared evidence ID: $Id"
    }
    Write-Host "EVIDENCE $Id"
}

$Exe = [IO.Path]::GetFullPath($Exe)
$previousAddress = $env:AGENTERM_IPC_ADDRESS
$previousWorkspace = $env:AGENTERM_WORKSPACE_PATH
$previousSettings = $env:AGENTERM_SETTINGS_PATH
$previousNoActivate = $env:AGENTERM_NO_ACTIVATE
$env:AGENTERM_IPC_ADDRESS = "127.0.0.1:$((54000 + ($PID % 1000)))"
$env:AGENTERM_WORKSPACE_PATH =
    Join-Path $env:TEMP "agenterm-proxy-smoke-$PID.json"
$env:AGENTERM_SETTINGS_PATH =
    Join-Path $env:TEMP "agenterm-proxy-smoke-settings-$PID.json"
$env:AGENTERM_NO_ACTIVATE = '1'

function Invoke-AgenTerm {
    param([string[]]$CommandArgs)
    $output = & $Exe @CommandArgs 2>&1
    if ($LASTEXITCODE -ne 0) {
        throw "agenterm $($CommandArgs -join ' ') failed:`n$($output -join "`n")"
    }
    return ($output -join "`n")
}

function Set-ProxyNow {
    param(
        [Parameter(Mandatory = $true)][string]$Target,
        [string]$Http,
        [string]$Https
    )
    $inputText = "HTTP_PROXY=$Http`nHTTPS_PROXY=$Https"
    $output = $inputText |
        & $Exe ui-action proxy-send-now -t $Target --stdin 2>&1
    if ($LASTEXITCODE -ne 0) {
        throw "proxy-send-now failed:`n$($output -join "`n")"
    }
}

function Wait-ProxyState {
    param(
        [Parameter(Mandatory = $true)][string]$Target,
        [Parameter(Mandatory = $true)][string]$State
    )
    return Invoke-AgenTerm @(
        'wait-ui', '-t', $Target, '--proxy-state', $State, '--timeout-ms', '10000'
    ) | ConvertFrom-Json
}

function Send-Line {
    param(
        [Parameter(Mandatory = $true)][string]$Target,
        [Parameter(Mandatory = $true)][string]$Text
    )
    Invoke-AgenTerm @('send-keys', '-l', '-t', $Target, $Text) | Out-Null
    Invoke-AgenTerm @('send-keys', '-t', $Target, 'Enter') | Out-Null
}

try {
    $cmdId = Invoke-AgenTerm @(
        'new-window', '-d', '-n', "proxy-cmd-$PID", '-F', '#{window_id}'
    )
    $http = 'http://127.0.0.1:18080'
    Set-ProxyNow -Target $cmdId -Http $http
    $snapshot = Wait-ProxyState -Target $cmdId -State 'applied'
    $proxy = $snapshot.tabs |
        Where-Object id -eq $cmdId |
        Select-Object -ExpandProperty working_context |
        Select-Object -ExpandProperty proxy
    if (-not $proxy.configured -or $proxy.request_pending -or
        $proxy.source -ne 'user_requested') {
        throw "cmd proxy state was not truthfully applied: $($proxy | ConvertTo-Json -Compress)"
    }
    Write-Evidence 'working-context.proxy-cmd-applied'

    Send-Line -Target $cmdId -Text (
        'cmd.exe /c if "%HTTP_PROXY%"=="' + $http + '" echo PROXY_CHILD_OK'
    )
    Invoke-AgenTerm @(
        'wait-pane', '-t', $cmdId, '--contains', 'PROXY_CHILD_OK',
        '--timeout-ms', '5000'
    ) | Out-Null
    Write-Evidence 'working-context.proxy-child-inheritance'

    Set-ProxyNow -Target $cmdId
    $snapshot = Wait-ProxyState -Target $cmdId -State 'applied'
    $proxy = $snapshot.tabs |
        Where-Object id -eq $cmdId |
        Select-Object -ExpandProperty working_context |
        Select-Object -ExpandProperty proxy
    if ($proxy.configured -or $proxy.request_pending) {
        throw "proxy clear was not confirmed: $($proxy | ConvertTo-Json -Compress)"
    }
    Send-Line -Target $cmdId -Text (
        'cmd.exe /c if not defined HTTP_PROXY echo PROXY_CLEAR_OK'
    )
    Invoke-AgenTerm @(
        'wait-pane', '-t', $cmdId, '--contains', 'PROXY_CLEAR_OK',
        '--timeout-ms', '5000'
    ) | Out-Null
    Write-Evidence 'working-context.proxy-clear'

    $powershellId = Invoke-AgenTerm @(
        'new-window', '-d', '-n', "proxy-powershell-$PID", '-F', '#{window_id}',
        '--', 'powershell.exe', '-NoLogo', '-NoProfile'
    )
    Set-ProxyNow -Target $powershellId -Http $http
    $null = Wait-ProxyState -Target $powershellId -State 'applied'
    Send-Line -Target $powershellId -Text (
        'cmd.exe /c if "%HTTP_PROXY%"=="' + $http + '" echo PS_CHILD_OK'
    )
    Invoke-AgenTerm @(
        'wait-pane', '-t', $powershellId, '--contains', 'PS_CHILD_OK',
        '--timeout-ms', '5000'
    ) | Out-Null
    Write-Evidence 'working-context.proxy-powershell-applied'

    $directId = Invoke-AgenTerm @(
        'new-window', '-d', '-n', "proxy-direct-$PID", '-F', '#{window_id}',
        '--', 'cmd.exe', '/k'
    )
    $previousPreference = $ErrorActionPreference
    try {
        $ErrorActionPreference = 'Continue'
        $null = "HTTP_PROXY=$http`nHTTPS_PROXY=" |
            & $Exe ui-action proxy-send-now -t $directId --stdin 2>&1
        $directExitCode = $LASTEXITCODE
    }
    finally {
        $ErrorActionPreference = $previousPreference
    }
    if ($directExitCode -eq 0) {
        throw 'proxy-send-now accepted a direct/noninteractive cmd invocation'
    }
    Write-Evidence 'working-context.proxy-noninteractive-rejected'
    Write-Host 'PASS: proxy application is confirmed and inherited by real child processes'
}
finally {
    try {
        & $Exe kill-server 2>$null | Out-Null
    }
    catch {
        # Best-effort cleanup after a server-side failure.
    }
    Remove-Item -LiteralPath $env:AGENTERM_WORKSPACE_PATH -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $env:AGENTERM_SETTINGS_PATH -ErrorAction SilentlyContinue
    foreach ($entry in @(
        @('AGENTERM_IPC_ADDRESS', $previousAddress),
        @('AGENTERM_WORKSPACE_PATH', $previousWorkspace),
        @('AGENTERM_SETTINGS_PATH', $previousSettings),
        @('AGENTERM_NO_ACTIVATE', $previousNoActivate)
    )) {
        if ($null -eq $entry[1]) {
            Remove-Item "Env:$($entry[0])" -ErrorAction SilentlyContinue
        } else {
            Set-Item "Env:$($entry[0])" $entry[1]
        }
    }
}
