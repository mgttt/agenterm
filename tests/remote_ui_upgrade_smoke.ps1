param(
    [string]$PriorGuiExe = (Join-Path $PSScriptRoot '..\dist\agenterm.exe'),
    [string]$NextGuiExe = (
        Join-Path $PSScriptRoot '..\target\release-fast\agenterm.exe'
    ),
    [string]$CliExe = (Join-Path $PSScriptRoot '..\dist\agenterm-cli.exe'),
    [string]$ServerExe = (
        Join-Path $PSScriptRoot '..\dist\agenterm-server.exe'
    ),
    [switch]$ListEvidence
)

$ErrorActionPreference = 'Stop'
$declaredEvidence = @('ui.same-server-upgrade-rollback')
if ($ListEvidence) {
    $declaredEvidence
    exit 0
}

. (Join-Path $PSScriptRoot 'TestHarness.ps1')

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;

public static class AgenTermUpgradeNativeTest {
    [DllImport("user32.dll")]
    public static extern IntPtr SendMessage(
        IntPtr window, uint message, IntPtr wparam, IntPtr lparam);

    [DllImport("user32.dll")]
    public static extern IntPtr GetDlgItem(IntPtr window, int id);

    [DllImport("user32.dll")]
    public static extern bool IsWindowVisible(IntPtr window);
}
'@

$PriorGuiExe = [IO.Path]::GetFullPath($PriorGuiExe)
$NextGuiExe = [IO.Path]::GetFullPath($NextGuiExe)
$ServerExe = [IO.Path]::GetFullPath($ServerExe)
foreach ($path in @($PriorGuiExe, $NextGuiExe, $CliExe, $ServerExe)) {
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
        throw "same-server upgrade input does not exist: $path"
    }
}
$priorHash = (Get-FileHash -LiteralPath $PriorGuiExe -Algorithm SHA256).Hash
$nextHash = (Get-FileHash -LiteralPath $NextGuiExe -Algorithm SHA256).Hash
if ($priorHash -eq $nextHash) {
    throw 'same-server upgrade requires two genuinely different GUI binaries'
}

$run = New-SmokeRunContext -Suite 'remote-ui-upgrade' `
    -Executable $CliExe -DeclaredEvidence $declaredEvidence -AllowPaneCapture
$CliExe = $run.Executable
$server = $null
$activeGui = $null
$runSucceeded = $false
$runFailure = $null

function Invoke-AgenTerm {
    param([string[]]$CommandArgs)
    Invoke-SmokeCli -Context $run -Arguments $CommandArgs
}

function Write-Evidence {
    param([Parameter(Mandatory = $true)][string]$Id)
    Write-SmokeEvidence -Context $run -Id $Id
}

function Start-UpgradeUi {
    param(
        [Parameter(Mandatory = $true)][string]$Executable,
        [Parameter(Mandatory = $true)][string]$Label
    )

    $stderrPath = Join-Path $run.RunDirectory "$Label-stderr.txt"
    $process = Start-Process -FilePath $Executable `
        -ArgumentList @(
            '--ui-client', '--no-activate', '--address', $run.Address
        ) `
        -RedirectStandardError $stderrPath `
        -PassThru -WindowStyle Normal
    Register-SmokeOwnedProcess -Context $run -Id $process.Id `
        -Kind 'gui' -Address $run.Address

    $deadline = [DateTime]::UtcNow.AddSeconds(12)
    do {
        $process.Refresh()
        $leaseOutput = @(
            & $CliExe --address $run.Address ui-lease status 2>&1
        )
        if ($LASTEXITCODE -eq 0 -and $process.MainWindowHandle -ne 0) {
            $lease = ($leaseOutput -join "`n") | ConvertFrom-Json
            if ($lease.attached -and $lease.client_pid -eq $process.Id) {
                return [pscustomobject]@{
                    Process = $process
                    Lease = $lease
                    StderrPath = $stderrPath
                }
            }
        }
        Start-Sleep -Milliseconds 25
    } while ([DateTime]::UtcNow -lt $deadline)

    $stderr = Get-Content -LiteralPath $stderrPath -Raw `
        -ErrorAction SilentlyContinue
    throw "$Label GUI did not acquire the UI lease: $stderr"
}

function Stop-UpgradeUiKeepingServer {
    param([Parameter(Mandatory = $true)]$Ui)

    $process = $Ui.Process
    [AgenTermUpgradeNativeTest]::SendMessage(
        $process.MainWindowHandle, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero
    ) | Out-Null
    $deadline = [DateTime]::UtcNow.AddSeconds(5)
    $keepButton = [IntPtr]::Zero
    do {
        $keepButton = [AgenTermUpgradeNativeTest]::GetDlgItem(
            $process.MainWindowHandle, 2109
        )
        if ($keepButton -ne [IntPtr]::Zero -and
            [AgenTermUpgradeNativeTest]::IsWindowVisible($keepButton)) {
            break
        }
        Start-Sleep -Milliseconds 20
    } while ([DateTime]::UtcNow -lt $deadline)
    if ($keepButton -eq [IntPtr]::Zero -or
        -not [AgenTermUpgradeNativeTest]::IsWindowVisible($keepButton)) {
        throw 'GUI close did not expose the Keep Server Running choice'
    }
    [AgenTermUpgradeNativeTest]::SendMessage(
        $keepButton, 0x00F5, [IntPtr]::Zero, [IntPtr]::Zero
    ) | Out-Null
    if (-not $process.WaitForExit(5000)) {
        throw 'GUI did not exit after Keep Server Running'
    }

    $deadline = [DateTime]::UtcNow.AddSeconds(5)
    do {
        $lease = Invoke-AgenTerm @('ui-lease', 'status') |
            ConvertFrom-Json
        if (-not $lease.attached) {
            return
        }
        Start-Sleep -Milliseconds 20
    } while ([DateTime]::UtcNow -lt $deadline)
    throw 'GUI exit did not release its interactive lease'
}

function Get-UpgradeState {
    param([Parameter(Mandatory = $true)][string]$TabId)

    $bootstrap = Invoke-AgenTerm @('ui-bootstrap') | ConvertFrom-Json
    $tab = @($bootstrap.tabs | Where-Object id -eq $TabId)
    if ($tab.Count -ne 1) {
        throw "stable tab disappeared during upgrade: $TabId"
    }
    [pscustomobject]@{
        Bootstrap = $bootstrap
        Tab = $tab[0]
    }
}

try {
    Write-Host 'STEP start one stable headless server'
    $server = Start-Process -FilePath $ServerExe `
        -ArgumentList @('--address', $run.Address) `
        -PassThru -WindowStyle Hidden
    Register-SmokeOwnedProcess -Context $run -Id $server.Id `
        -Kind 'server' -Address $run.Address
    $deadline = [DateTime]::UtcNow.AddSeconds(10)
    do {
        $protocolOutput = @(
            & $CliExe --address $run.Address protocol-info --running 2>&1
        )
        if ($LASTEXITCODE -eq 0) {
            break
        }
        Start-Sleep -Milliseconds 25
    } while ([DateTime]::UtcNow -lt $deadline)
    if ($LASTEXITCODE -ne 0) {
        throw "headless server did not become ready: $($protocolOutput -join "`n")"
    }

    $baseline = Invoke-AgenTerm @('ui-bootstrap') | ConvertFrom-Json
    $tabId = [string]$baseline.active_tab_id
    $baselineTab = @($baseline.tabs | Where-Object id -eq $tabId)
    if ($baseline.server_pid -ne $server.Id -or
        $baselineTab.Count -ne 1 -or
        $null -eq $baselineTab[0].process_id) {
        throw 'upgrade baseline does not identify the stable server and ConPTY'
    }
    $serverPid = [int]$baseline.server_pid
    $serverEpoch = [string]$baseline.server_epoch
    $ptyPid = [int]$baselineTab[0].process_id
    $draft = "upgrade-draft-$($run.RunId)"
    Invoke-AgenTerm @('set-composer', '-t', $tabId, $draft) | Out-Null

    Write-Host 'STEP connect the prior GUI and reject a competing startup'
    $prior = Start-UpgradeUi -Executable $PriorGuiExe -Label 'prior'
    $activeGui = $prior.Process
    $priorIdentity = $prior.Lease.client_build | ConvertTo-Json -Compress
    if ([string]::IsNullOrWhiteSpace($priorIdentity) -or
        $prior.Lease.client_build.protocol_version -ne 1) {
        throw 'prior GUI lease did not expose its build identity'
    }
    $priorPid = $prior.Process.Id
    $priorHwnd = [int64]$prior.Process.MainWindowHandle

    $conflictPath = Join-Path $run.RunDirectory 'lease-conflict-stderr.txt'
    $conflict = Start-Process -FilePath $NextGuiExe `
        -ArgumentList @(
            '--ui-client', '--no-activate', '--address', $run.Address
        ) `
        -RedirectStandardError $conflictPath `
        -PassThru -WindowStyle Normal
    Register-SmokeOwnedProcess -Context $run -Id $conflict.Id `
        -Kind 'gui' -Address $run.Address
    $conflictDeadline = [DateTime]::UtcNow.AddSeconds(2)
    do {
        $conflictLease = Invoke-AgenTerm @('ui-lease', 'status') |
            ConvertFrom-Json
        if (-not $conflictLease.attached -or
            $conflictLease.client_pid -ne $priorPid) {
            throw 'competing GUI stole or released the prior live lease'
        }
        if ($conflict.HasExited) {
            break
        }
        Start-Sleep -Milliseconds 25
        $conflict.Refresh()
    } while ([DateTime]::UtcNow -lt $conflictDeadline)
    if (-not $conflict.HasExited) {
        Stop-Process -Id $conflict.Id -Force
        $conflict.WaitForExit(5000) | Out-Null
    }
    $healthyAfterConflict = Get-UpgradeState -TabId $tabId
    if ($healthyAfterConflict.Bootstrap.server_pid -ne $serverPid -or
        $healthyAfterConflict.Tab.process_id -ne $ptyPid) {
        throw 'failed GUI startup disturbed the server or ConPTY'
    }

    Write-Host 'STEP stream output while replacing the prior GUI'
    $streamPrefix = "UPGRADE_STREAM_$($run.RunId)"
    $streamCommand = (
        "for /L %i in (1,1,12) do @(echo ${streamPrefix}_%i " +
        '& ping -n 2 127.0.0.1 >nul)'
    )
    Invoke-AgenTerm @('send-keys', '-t', $tabId, '-l', $streamCommand) |
        Out-Null
    Invoke-AgenTerm @('send-keys', '-t', $tabId, 'Enter') | Out-Null
    Invoke-AgenTerm @(
        'wait-pane', '-t', $tabId, '--contains', "${streamPrefix}_2",
        '--timeout-ms', '5000'
    ) | Out-Null
    Stop-UpgradeUiKeepingServer -Ui $prior
    $activeGui = $null

    Write-Host 'STEP connect the next GUI to the unchanged server'
    $next = Start-UpgradeUi -Executable $NextGuiExe -Label 'next'
    $activeGui = $next.Process
    $nextIdentity = $next.Lease.client_build | ConvertTo-Json -Compress
    if ($nextIdentity -eq $priorIdentity -or
        $next.Process.Id -eq $priorPid -or
        [int64]$next.Process.MainWindowHandle -eq $priorHwnd) {
        throw 'next GUI did not expose different process, HWND, and build identity'
    }
    Invoke-AgenTerm @(
        'wait-pane', '-t', $tabId, '--contains', "${streamPrefix}_8",
        '--timeout-ms', '12000'
    ) | Out-Null
    $nextState = Get-UpgradeState -TabId $tabId
    if ($nextState.Bootstrap.server_pid -ne $serverPid -or
        $nextState.Bootstrap.server_epoch -ne $serverEpoch -or
        $nextState.Bootstrap.active_tab_id -ne $tabId -or
        $nextState.Tab.process_id -ne $ptyPid -or
        $nextState.Tab.composer.text -ne $draft -or
        $nextState.Tab.working_context.cwd -ne
            $baselineTab[0].working_context.cwd -or
        $nextState.Tab.working_context.proxy_configured -ne
            $baselineTab[0].working_context.proxy_configured) {
        throw 'next GUI attachment lost stable server, PTY, or workbench state'
    }

    Write-Host 'STEP reject an incompatible hello without ending live state'
    $incompatible = Invoke-AgenTerm @(
        'ui-hello', '--minimum', '2', '--maximum', '2',
        '--client-id', "incompatible-$($run.RunId)"
    ) | ConvertFrom-Json
    if ($incompatible.accepted -or
        $incompatible.compatibility -ne 'client_too_new') {
        throw 'incompatible UI protocol did not fail closed'
    }
    $server.Refresh()
    if ($server.HasExited) {
        throw 'incompatible UI negotiation ended the stable server'
    }

    Write-Host 'STEP roll back to the prior GUI and finish the same output stream'
    Stop-UpgradeUiKeepingServer -Ui $next
    $activeGui = $null
    $rollback = Start-UpgradeUi -Executable $PriorGuiExe -Label 'rollback'
    $activeGui = $rollback.Process
    $rollbackIdentity = $rollback.Lease.client_build |
        ConvertTo-Json -Compress
    if ($rollbackIdentity -ne $priorIdentity) {
        throw 'rollback GUI did not restore the prior compatible build identity'
    }
    Invoke-AgenTerm @(
        'wait-pane', '-t', $tabId, '--contains', "${streamPrefix}_12",
        '--submit-complete', '--timeout-ms', '12000'
    ) | Out-Null
    $finalState = Get-UpgradeState -TabId $tabId
    $capture = Invoke-AgenTerm @(
        'capture-pane', '-p', '-t', $tabId, '--max-bytes', '65536'
    )
    if ($finalState.Bootstrap.server_pid -ne $serverPid -or
        $finalState.Bootstrap.server_epoch -ne $serverEpoch -or
        $finalState.Bootstrap.active_tab_id -ne $tabId -or
        $finalState.Tab.process_id -ne $ptyPid -or
        $finalState.Tab.composer.text -ne $draft -or
        $finalState.Tab.screen.max_scrollback -lt
            $nextState.Tab.screen.max_scrollback -or
        -not $capture.Contains("${streamPrefix}_2") -or
        -not $capture.Contains("${streamPrefix}_8") -or
        -not $capture.Contains("${streamPrefix}_12")) {
        throw 'rollback did not preserve causal server, PTY, draft, and scrollback'
    }

    Write-Evidence 'ui.same-server-upgrade-rollback'
    Stop-UpgradeUiKeepingServer -Ui $rollback
    $activeGui = $null
    Invoke-AgenTerm @('shutdown') | Out-Null
    if (-not $server.WaitForExit(10000)) {
        throw 'stable server did not stop after completed upgrade proof'
    }
    $runSucceeded = $true
}
catch {
    $runFailure = $_
}
finally {
    Complete-SmokeRun -Context $run -Succeeded $runSucceeded `
        -FailureRecord $runFailure
}

if (-not $runSucceeded) {
    throw $runFailure
}
Write-Host (
    'PASS: real GUI bytes upgrade and roll back on one stable server and PTY'
)
