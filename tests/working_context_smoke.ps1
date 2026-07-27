param(
    [string]$GuiExe = (Join-Path $PSScriptRoot '..\dist\agenterm.exe'),
    [string]$CliExe = (Join-Path $PSScriptRoot '..\dist\agenterm-cli.exe'),
    [switch]$ListEvidence
)

$ErrorActionPreference = 'Stop'
$declaredEvidence = @('ux.working-context-proxy')
if ($ListEvidence) {
    $declaredEvidence
    exit 0
}

function Write-Evidence {
    param([Parameter(Mandatory = $true)][string]$Id)
    if ($declaredEvidence -notcontains $Id) {
        throw "Working-context smoke emitted undeclared evidence ID: $Id"
    }
    Write-Host "EVIDENCE $Id"
}

function Assert-NoSecret {
    param(
        [Parameter(Mandatory = $true)][AllowEmptyString()][string]$Text,
        [Parameter(Mandatory = $true)][string]$Where
    )
    if ($Text.Contains($script:secret)) {
        throw "Proxy credential leaked through $Where"
    }
}

function Invoke-Cli {
    param([Parameter(ValueFromRemainingArguments = $true)][string[]]$CommandArgs)
    $output = & $CliExe @CommandArgs 2>&1
    if ($LASTEXITCODE -ne 0) {
        throw "agenterm-cli $($CommandArgs -join ' ') failed:`n$($output -join "`n")"
    }
    ($output -join "`n")
}

function Invoke-ExpectedFailure {
    param([Parameter(ValueFromRemainingArguments = $true)][string[]]$CommandArgs)
    $oldPreference = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        $output = & $CliExe @CommandArgs 2>&1
        $exitCode = $LASTEXITCODE
    }
    finally {
        $ErrorActionPreference = $oldPreference
    }
    if ($exitCode -eq 0) {
        throw "agenterm-cli $($CommandArgs -join ' ') unexpectedly succeeded"
    }
    ($output -join "`n")
}

function Wait-Server {
    param([Parameter(Mandatory = $true)][Diagnostics.Process]$Process)
    $deadline = [DateTime]::UtcNow.AddSeconds(10)
    do {
        $Process.Refresh()
        if ($Process.HasExited) {
            $stderr = Get-Content -LiteralPath $stderrFile -Raw -ErrorAction SilentlyContinue
            throw "AgenTerm exited before its server became ready:`n$stderr"
        }
        $oldPreference = $ErrorActionPreference
        $ErrorActionPreference = 'SilentlyContinue'
        try {
            & $CliExe ui-snapshot 2>$null | Out-Null
            $ready = $LASTEXITCODE -eq 0
        }
        finally {
            $ErrorActionPreference = $oldPreference
        }
        if ($ready) { return }
        Start-Sleep -Milliseconds 50
    } while ([DateTime]::UtcNow -lt $deadline)
    throw 'AgenTerm server did not become ready within 10 seconds'
}

if (-not ('AgenTermProxyNativeTest' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;

public static class AgenTermProxyNativeTest {
    [DllImport("user32.dll")]
    static extern IntPtr GetDlgItem(IntPtr parent, int id);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    static extern IntPtr FindWindowExW(
        IntPtr parent, IntPtr after, string className, string windowName);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    static extern IntPtr SendMessageW(
        IntPtr window, uint message, UIntPtr wparam, StringBuilder lparam);

    public static string ComposerText(IntPtr parent) {
        IntPtr edit = GetDlgItem(parent, 1002);
        if (edit == IntPtr.Zero) {
            edit = FindWindowExW(parent, IntPtr.Zero, "EDIT", null);
        }
        var text = new StringBuilder(32768);
        SendMessageW(edit, 0x000D, (UIntPtr)text.Capacity, text);
        return text.ToString();
    }
}
'@
}

$GuiExe = [IO.Path]::GetFullPath($GuiExe)
$CliExe = [IO.Path]::GetFullPath($CliExe)
$previousAddress = $env:AGENTERM_IPC_ADDRESS
$previousWorkspace = $env:AGENTERM_WORKSPACE_PATH
$previousInstances = $env:AGENTERM_INSTANCE_DIR
$previousHttp = $env:HTTP_PROXY
$previousHttps = $env:HTTPS_PROXY
$address = "127.0.0.1:$((55500 + ($PID % 400)))"
$workspace = Join-Path $env:TEMP "agenterm-proxy-$PID.json"
$instances = Join-Path $env:TEMP "agenterm-proxy-instances-$PID"
$stderrFile = Join-Path $env:TEMP "agenterm-proxy-stderr-$PID.txt"
$script:secret = ('credential-' + $PID + '-sentinel')
$proxyUrl = "https://alice:$script:secret@proxy.example:8443/private?token=$script:secret#fragment"
$process = $null

try {
    $env:AGENTERM_IPC_ADDRESS = $address
    $env:AGENTERM_WORKSPACE_PATH = $workspace
    $env:AGENTERM_INSTANCE_DIR = $instances
    $env:HTTP_PROXY = $proxyUrl
    $env:HTTPS_PROXY = $proxyUrl
    $process = Start-Process -FilePath $GuiExe -RedirectStandardError $stderrFile -PassThru
    Remove-Item Env:HTTP_PROXY -ErrorAction SilentlyContinue
    Remove-Item Env:HTTPS_PROXY -ErrorAction SilentlyContinue

    Wait-Server $process
    Invoke-Cli wait-ui -t 0 --tab-state running --timeout-ms 10000 | Out-Null
    $snapshotText = Invoke-Cli ui-snapshot
    Assert-NoSecret $snapshotText 'launch ui-snapshot'
    $snapshot = $snapshotText | ConvertFrom-Json
    $tab = $snapshot.tabs | Where-Object active
    if (-not $tab.working_context.proxy.configured -or
        $tab.working_context.proxy.source -ne 'launch' -or
        $tab.working_context.proxy.endpoint_visible -or
        $tab.working_context.proxy.credential_revealed) {
        throw 'Launch proxy state was not exposed as safe metadata'
    }
    $tabId = $tab.id
    $epoch = $snapshot.event_position.epoch
    $launchPane = Invoke-Cli pane-snapshot -t $tabId | ConvertFrom-Json
    $inputWrites = [int]$launchPane.windows[0].input_writes

    $visibleText = Invoke-Cli ui-action proxy-toggle-visibility -t $tabId
    Assert-NoSecret $visibleText 'visible endpoint snapshot'
    $visible = $visibleText | ConvertFrom-Json
    $visibleTab = $visible.tabs | Where-Object id -eq $tabId
    if (-not $visibleTab.working_context.proxy.endpoint_visible -or
        $visibleTab.working_context.proxy.credential_revealed) {
        throw 'Proxy eye did not toggle only the sanitized endpoint state'
    }
    $eye = $visible.layout.status_bar.proxy.eye_bounds
    if ($eye.width -le 0 -or $eye.height -le 0) {
        throw 'Proxy eye did not expose a bounded GDI hit target'
    }
    $png = Join-Path $env:TEMP "agenterm-proxy-eye-$PID.png"
    Invoke-Cli screenshot '-o' $png | Out-Null
    if (-not (Test-Path -LiteralPath $png) -or (Get-Item $png).Length -le 1000) {
        throw 'Proxy GDI eye screenshot evidence was not created'
    }

    Invoke-Cli ui-action open-proxy-editor -t $tabId | Out-Null
    Invoke-Cli ui-action proxy-reveal-credentials | Out-Null
    $process.Refresh()
    $revealed = [AgenTermProxyNativeTest]::ComposerText($process.MainWindowHandle)
    if (-not $revealed.Contains($script:secret)) {
        throw "Explicit second reveal did not populate the proxy editor (length=$($revealed.Length), http=$($revealed.StartsWith('HTTP_PROXY=')), hwnd=$($process.MainWindowHandle))"
    }
    $blocked = Invoke-ExpectedFailure ui-action select-tab -t $tabId
    Assert-NoSecret $blocked 'focus-trap failure'
    $remasked = [AgenTermProxyNativeTest]::ComposerText($process.MainWindowHandle)
    if ($remasked.Contains($script:secret)) {
        throw 'Proxy credentials remained visible after a focus-trap escape attempt'
    }
    Invoke-Cli ui-action cancel | Out-Null

    $proxyInput = "HTTP_PROXY=$proxyUrl`nHTTPS_PROXY=$proxyUrl"
    $prepareOutput = $proxyInput | & $CliExe ui-action proxy-prepare -t $tabId --stdin 2>&1
    if ($LASTEXITCODE -ne 0) {
        throw "Proxy prepare failed:`n$($prepareOutput -join "`n")"
    }
    $prepareText = $prepareOutput -join "`n"
    Assert-NoSecret $prepareText 'prepare stdout/stderr'
    $preparedText = Invoke-Cli ui-snapshot
    Assert-NoSecret $preparedText 'prepared ui-snapshot'
    $prepared = $preparedText | ConvertFrom-Json
    $preparedTab = $prepared.tabs | Where-Object id -eq $tabId
    $preparedPaneText = Invoke-Cli pane-snapshot -t $tabId
    Assert-NoSecret $preparedPaneText 'prepared pane-snapshot'
    $preparedPane = $preparedPaneText | ConvertFrom-Json
    if ($preparedPane.windows[0].input_writes -ne $inputWrites -or
        -not $preparedTab.draft -or
        $preparedTab.working_context.proxy.source -ne 'user_requested' -or
        -not $preparedTab.working_context.proxy.request_pending) {
        throw 'Prepare was not a pending sensitive Composer operation'
    }

    $showFailure = Invoke-ExpectedFailure show-composer -t $tabId
    Assert-NoSecret $showFailure 'show-composer failure'
    if ($showFailure -notmatch 'sensitive proxy draft') {
        throw 'show-composer did not return a typed sensitive-draft failure'
    }
    $paneText = $preparedPaneText
    $pane = $preparedPane
    if (-not $pane.windows[0].composer_sensitive -or
        $pane.windows[0].composer -ne '<redacted>') {
        throw 'pane-snapshot did not redact the sensitive Composer draft'
    }
    Invoke-Cli save-workspace | Out-Null
    $workspaceText = Get-Content -LiteralPath $workspace -Raw
    Assert-NoSecret $workspaceText 'workspace persistence'
    $events = Invoke-Cli read-events --epoch $epoch --after 0 --limit 1024
    Assert-NoSecret $events 'observable event journal'

    $sendOutput = Invoke-Cli send-composer -t $tabId
    Assert-NoSecret $sendOutput 'explicit send response'
    Invoke-Cli wait-pane -t $tabId --submit-complete --timeout-ms 10000 | Out-Null
    $sentPane = Invoke-Cli pane-snapshot -t $tabId
    Assert-NoSecret $sentPane 'post-send pane-snapshot'
    $sentPaneObject = $sentPane | ConvertFrom-Json
    if ([int]$sentPaneObject.windows[0].input_bytes -ne ($inputWrites + 17)) {
        throw 'Sensitive input byte accounting exposed the real command length'
    }
    $sentSnapshot = Invoke-Cli ui-snapshot
    Assert-NoSecret $sentSnapshot 'post-send ui-snapshot'

    Invoke-Cli shutdown | Out-Null
    if (-not $process.WaitForExit(5000)) {
        throw 'First proxy test server did not stop'
    }
    $process = Start-Process -FilePath $GuiExe -RedirectStandardError $stderrFile -PassThru
    Wait-Server $process
    Invoke-Cli wait-ui -t 0 --tab-state running --timeout-ms 10000 | Out-Null
    $restoredText = Invoke-Cli ui-snapshot
    Assert-NoSecret $restoredText 'restart ui-snapshot'
    $restored = $restoredText | ConvertFrom-Json
    $restoredTab = $restored.tabs | Where-Object active
    if ($restoredTab.working_context.proxy.configured -or
        $restoredTab.working_context.proxy.source -ne 'off') {
        throw 'Proxy state was incorrectly restored from workspace persistence'
    }
    Assert-NoSecret (Get-Content -LiteralPath $stderrFile -Raw) 'GUI stderr'
    Write-Evidence 'ux.working-context-proxy'
    Write-Host 'PASS: proxy privacy, GDI eye, sensitive prepare/send, remask, and restart'
}
finally {
    & $CliExe kill-server 2>$null | Out-Null
    if ($null -ne $process -and -not $process.HasExited) {
        $process.WaitForExit(2000) | Out-Null
    }
    Remove-Item -LiteralPath $workspace -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $stderrFile -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $instances -Recurse -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath (Join-Path $env:TEMP "agenterm-proxy-eye-$PID.png") `
        -ErrorAction SilentlyContinue
    if ($null -eq $previousAddress) { Remove-Item Env:AGENTERM_IPC_ADDRESS -ErrorAction SilentlyContinue }
    else { $env:AGENTERM_IPC_ADDRESS = $previousAddress }
    if ($null -eq $previousWorkspace) { Remove-Item Env:AGENTERM_WORKSPACE_PATH -ErrorAction SilentlyContinue }
    else { $env:AGENTERM_WORKSPACE_PATH = $previousWorkspace }
    if ($null -eq $previousInstances) { Remove-Item Env:AGENTERM_INSTANCE_DIR -ErrorAction SilentlyContinue }
    else { $env:AGENTERM_INSTANCE_DIR = $previousInstances }
    if ($null -eq $previousHttp) { Remove-Item Env:HTTP_PROXY -ErrorAction SilentlyContinue }
    else { $env:HTTP_PROXY = $previousHttp }
    if ($null -eq $previousHttps) { Remove-Item Env:HTTPS_PROXY -ErrorAction SilentlyContinue }
    else { $env:HTTPS_PROXY = $previousHttps }
}
