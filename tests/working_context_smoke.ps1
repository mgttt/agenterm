param(
    [string]$GuiExe = (Join-Path $PSScriptRoot '..\dist\agenterm.exe'),
    [string]$CliExe = (Join-Path $PSScriptRoot '..\dist\agenterm-cli.exe'),
    [switch]$ListEvidence,
    [switch]$InternalFailureBundleProbe
)

$ErrorActionPreference = 'Stop'
$declaredEvidence = @('ux.working-context-proxy')
if ($ListEvidence) {
    $declaredEvidence
    exit 0
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

function Remove-FixtureSecrets {
    param([AllowNull()][string]$Text)
    if ($null -eq $Text) {
        return ''
    }
    return $Text.Replace($script:proxyUrl, '<fixture-proxy>').
        Replace($script:secret, '<fixture-secret>')
}

function Invoke-Cli {
    param([Parameter(ValueFromRemainingArguments = $true)][string[]]$CommandArgs)
    $outputItems = @(& $CliExe @CommandArgs 2>&1)
    $exitCode = $LASTEXITCODE
    $output = $outputItems -join "`n"
    Add-SmokeCommandRecord -Context $context -Arguments $CommandArgs `
        -ExitCode $exitCode -ExpectedFailure $false `
        -Output (Remove-FixtureSecrets $output)
    if ($exitCode -ne 0) {
        throw "agenterm-cli $($CommandArgs -join ' ') failed:`n$output"
    }
    $output
}

function Invoke-ExpectedFailure {
    param([Parameter(ValueFromRemainingArguments = $true)][string[]]$CommandArgs)
    $oldPreference = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        $outputItems = @(& $CliExe @CommandArgs 2>&1)
        $exitCode = $LASTEXITCODE
    }
    finally {
        $ErrorActionPreference = $oldPreference
    }
    $output = $outputItems -join "`n"
    Add-SmokeCommandRecord -Context $context -Arguments $CommandArgs `
        -ExitCode $exitCode -ExpectedFailure $true `
        -Output (Remove-FixtureSecrets $output)
    if ($exitCode -eq 0) {
        throw "agenterm-cli $($CommandArgs -join ' ') unexpectedly succeeded"
    }
    $output
}

function Wait-Server {
    param([Parameter(Mandatory = $true)][Diagnostics.Process]$Process)
    $deadline = [DateTime]::UtcNow.AddSeconds(10)
    do {
        $Process.Refresh()
        if ($Process.HasExited) {
            $stderr = Get-Content -LiteralPath $stderrFile -Raw -ErrorAction SilentlyContinue
            Add-SmokeCommandRecord -Context $context `
                -Arguments @('ui-snapshot') -ExitCode -1 `
                -ExpectedFailure $false `
                -Output (Remove-FixtureSecrets $stderr)
            throw "AgenTerm exited before its server became ready:`n$stderr"
        }
        $oldPreference = $ErrorActionPreference
        $ErrorActionPreference = 'SilentlyContinue'
        try {
            $probeOutput = @(& $CliExe ui-snapshot 2>&1)
            $probeExitCode = $LASTEXITCODE
            $ready = $probeExitCode -eq 0
        }
        finally {
            $ErrorActionPreference = $oldPreference
        }
        if ($ready) {
            Add-SmokeCommandRecord -Context $context `
                -Arguments @('ui-snapshot') -ExitCode $probeExitCode `
                -ExpectedFailure $false `
                -Output (Remove-FixtureSecrets ($probeOutput -join "`n"))
            return
        }
        Start-Sleep -Milliseconds 50
    } while ([DateTime]::UtcNow -lt $deadline)
    Add-SmokeCommandRecord -Context $context -Arguments @('ui-snapshot') `
        -ExitCode $probeExitCode -ExpectedFailure $false `
        -Output (Remove-FixtureSecrets ($probeOutput -join "`n"))
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
foreach ($path in @($GuiExe, $CliExe)) {
    if (-not (Test-Path -LiteralPath $path)) {
        throw "AgenTerm executable not found: $path"
    }
}
. (Join-Path $PSScriptRoot 'TestHarness.ps1')
$context = New-SmokeRunContext -Suite 'working-context' `
    -Executable $CliExe -DeclaredEvidence $declaredEvidence
$context.PreviousEnvironment['HTTP_PROXY'] = $env:HTTP_PROXY
$context.PreviousEnvironment['HTTPS_PROXY'] = $env:HTTPS_PROXY
$address = $context.Address
$workspace = $context.WorkspacePath
$instances = $context.InstanceDirectory
$stderrFile = Join-Path $context.RunDirectory 'gui-stderr.txt'
$script:secret = ('credential-' + $PID + '-sentinel')
$proxyUrl = "https://alice:$script:secret@proxy.example:8443/private?token=$script:secret#fragment"
$script:proxyUrl = $proxyUrl
$process = $null
$succeeded = $false
$failureRecord = $null

function Write-Evidence {
    param([Parameter(Mandatory = $true)][string]$Id)
    Write-SmokeEvidence -Context $context -Id $Id
}

function Protect-WorkingContextFailureBundle {
    foreach ($file in @(
        Get-ChildItem -LiteralPath $context.RunDirectory -Recurse -File `
            -ErrorAction SilentlyContinue
    )) {
        if ($file.Extension -notin @('.json', '.txt', '.log') -and
            $file.Name -ne 'emitted.txt') {
            continue
        }
        $text = Get-Content -LiteralPath $file.FullName -Raw `
            -ErrorAction SilentlyContinue
        if ($null -ne $text) {
            $scrubbed = Remove-FixtureSecrets $text
            [IO.File]::WriteAllText(
                $file.FullName,
                $scrubbed,
                [Text.UTF8Encoding]::new($false)
            )
        }
    }
}

try {
    $env:AGENTERM_INSTANCE_DIR = $instances
    $env:HTTP_PROXY = $proxyUrl
    $env:HTTPS_PROXY = $proxyUrl
    $process = Start-Process -FilePath $GuiExe -RedirectStandardError $stderrFile -PassThru
    Register-SmokeOwnedProcess -Context $context -Id $process.Id `
        -Kind 'gui' -Address $address
    Remove-Item Env:HTTP_PROXY -ErrorAction SilentlyContinue
    Remove-Item Env:HTTPS_PROXY -ErrorAction SilentlyContinue

    Wait-Server $process
    if ($InternalFailureBundleProbe) {
        throw "Injected failure containing fixture secret: $proxyUrl"
    }
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
    $proxyStatus = $snapshot.layout.status_bar.proxy
    if (-not $proxyStatus.archived -or $proxyStatus.available -or
        $proxyStatus.bounds.width -ne 0 -or $null -ne $proxyStatus.action -or
        $null -ne $proxyStatus.eye_action) {
        throw 'Archived Proxy status surface remained visible or actionable'
    }

    $visibleText = Invoke-Cli ui-action proxy-toggle-visibility -t $tabId
    Assert-NoSecret $visibleText 'visible endpoint snapshot'
    $visible = $visibleText | ConvertFrom-Json
    $visibleTab = $visible.tabs | Where-Object id -eq $tabId
    if (-not $visibleTab.working_context.proxy.endpoint_visible -or
        $visibleTab.working_context.proxy.credential_revealed) {
        throw 'Proxy visibility compatibility action changed more than sanitized endpoint state'
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
    $prepareArguments = @(
        'ui-action', 'proxy-prepare', '-t', $tabId, '--stdin'
    )
    $prepareOutput = @(
        $proxyInput | & $CliExe @prepareArguments 2>&1
    )
    $prepareExitCode = $LASTEXITCODE
    Add-SmokeCommandRecord -Context $context -Arguments $prepareArguments `
        -ExitCode $prepareExitCode -ExpectedFailure $false `
        -Output (Remove-FixtureSecrets ($prepareOutput -join "`n"))
    if ($prepareExitCode -ne 0) {
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
    Register-SmokeOwnedProcess -Context $context -Id $process.Id `
        -Kind 'gui' -Address $address
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
    Write-Host 'PASS: archived Proxy status surface, privacy compatibility, prepare/send, remask, and restart'
    $succeeded = $true
}
catch {
    $failureRecord = $_
}
finally {
    Remove-Item Env:HTTP_PROXY -ErrorAction SilentlyContinue
    Remove-Item Env:HTTPS_PROXY -ErrorAction SilentlyContinue
    $safeFailure = if ($null -eq $failureRecord) {
        $null
    } else {
        Remove-FixtureSecrets ($failureRecord | Out-String)
    }
    Complete-SmokeRun -Context $context -Succeeded $succeeded `
        -FailureRecord $safeFailure
    if ($null -ne $process -and -not $process.HasExited) {
        if (-not $process.WaitForExit(2000)) {
            Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
        }
    }
    if (-not $succeeded) {
        Protect-WorkingContextFailureBundle
    }
}
if (-not $succeeded) {
    throw $failureRecord
}
