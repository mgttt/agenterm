param(
    [string]$Exe = (Join-Path $PSScriptRoot '..\dist\agenterm-cli.exe')
)

$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'TestHarness.ps1')

$run = New-SmokeRunContext -Suite 'wake' -Executable $Exe -AllowPaneCapture
$Exe = $run.Executable
$runSucceeded = $false
$runFailure = $null
$clients = [Collections.Generic.List[object]]::new()

function Invoke-AgenTerm {
    param([string[]]$CommandArgs)
    Invoke-SmokeCli -Context $run -Arguments $CommandArgs
}

function Get-Fnv1a64 {
    param([Parameter(Mandatory = $true)][byte[]]$Bytes)

    $hash = [Numerics.BigInteger]::Parse('14695981039346656037')
    $prime = [Numerics.BigInteger]::Parse('1099511628211')
    $modulus = [Numerics.BigInteger]::One -shl 64
    foreach ($byte in $Bytes) {
        $hash = (($hash -bxor [int]$byte) * $prime) % $modulus
    }
    $hex = $hash.ToString('x').PadLeft(16, '0')
    if ($hex.Length -gt 16) {
        $hex = $hex.Substring($hex.Length - 16)
    }
    return "fnv1a64:$hex"
}

function Invoke-RawIpc {
    param([Parameter(Mandatory = $true)]$Request)

    $parts = $run.Address.Split(':')
    $client = [Net.Sockets.TcpClient]::new()
    try {
        $client.ReceiveTimeout = 10000
        $client.SendTimeout = 10000
        $client.Connect($parts[0], [int]$parts[1])
        $stream = $client.GetStream()
        $writer = [IO.StreamWriter]::new(
            $stream, [Text.UTF8Encoding]::new($false), 1024, $true
        )
        $reader = [IO.StreamReader]::new(
            $stream, [Text.UTF8Encoding]::new($false), $false, 1024, $true
        )
        try {
            $writer.NewLine = "`n"
            $writer.WriteLine(($Request | ConvertTo-Json -Depth 8 -Compress))
            $writer.Flush()
            $responseLine = $reader.ReadLine()
            if ([string]::IsNullOrWhiteSpace($responseLine)) {
                throw 'raw IPC returned an empty response'
            }
            return $responseLine
        }
        finally {
            $reader.Dispose()
            $writer.Dispose()
            $stream.Dispose()
        }
    }
    finally {
        $client.Dispose()
    }
}

try {
    $name = "wake-$($run.RunId)"
    $token = "WAKE_COMPLETE_$($run.RunId)"

    Write-Host 'STEP create an isolated terminal'
    Invoke-AgenTerm @('new-window', '-d', '-n', $name) | Out-Null
    Invoke-AgenTerm @(
        'wait-ui', '-t', $name, '--tab-state', 'running',
        '--timeout-ms', '10000'
    ) | Out-Null

    Write-Host 'STEP concurrent IPC replies and PTY output both make progress'
    $asyncDirectory = Join-Path $run.RunDirectory 'async'
    New-Item -ItemType Directory -Path $asyncDirectory -Force | Out-Null
    foreach ($index in 0..31) {
        $stdout = Join-Path $asyncDirectory "snapshot-$index.out"
        $stderr = Join-Path $asyncDirectory "snapshot-$index.err"
        $arguments = @('--address', $run.Address, 'ui-snapshot')
        $process = Start-Process -FilePath $Exe -ArgumentList $arguments `
            -RedirectStandardOutput $stdout -RedirectStandardError $stderr `
            -WindowStyle Hidden -PassThru
        Register-SmokeOwnedProcess -Context $run -Id $process.Id -Kind 'client'
        $clients.Add([pscustomobject]@{
            Process = $process
            Stdout = $stdout
            Stderr = $stderr
            Arguments = $arguments
        })
    }

    Invoke-AgenTerm @(
        'send-keys', '-t', $name, '-l',
        "for /L %i in (1,1,80) do @echo WAKE_$($run.RunId)`_%i"
    ) | Out-Null
    Invoke-AgenTerm @('send-keys', '-t', $name, 'Enter') | Out-Null
    Invoke-AgenTerm @(
        'send-keys', '-t', $name, '-l', "echo $token"
    ) | Out-Null
    Invoke-AgenTerm @('send-keys', '-t', $name, 'Enter') | Out-Null

    $clientDeadline = [DateTime]::UtcNow.AddSeconds(20)
    foreach ($client in $clients) {
        $remaining = [int][Math]::Max(
            1, ($clientDeadline - [DateTime]::UtcNow).TotalMilliseconds
        )
        if (-not $client.Process.WaitForExit($remaining)) {
            throw "concurrent IPC client $($client.Process.Id) exceeded its deadline"
        }
        $client.Process.Refresh()
        $stdout = Get-Content -LiteralPath $client.Stdout -Raw `
            -ErrorAction SilentlyContinue
        $stderr = Get-Content -LiteralPath $client.Stderr -Raw `
            -ErrorAction SilentlyContinue
        Add-SmokeCommandRecord -Context $run -Arguments $client.Arguments `
            -ExitCode $client.Process.ExitCode -ExpectedFailure $false `
            -Output "$stdout`n$stderr"
        if ($client.Process.ExitCode -ne 0) {
            throw (
                "concurrent IPC client $($client.Process.Id) failed with " +
                "exit $($client.Process.ExitCode): $stderr"
            )
        }
        $snapshot = $stdout | ConvertFrom-Json
        if ($snapshot.protocol_version -lt 1) {
            throw 'concurrent IPC client returned an invalid UI snapshot'
        }
        $client.Process.Dispose()
    }
    Invoke-AgenTerm @(
        'wait-pane', '-t', $name, '--contains', $token,
        '--timeout-ms', '10000'
    ) | Out-Null

    Write-Host 'STEP expired mutation is a typed no-op'
    $baseline = "wake-baseline-$($run.RunId)"
    Invoke-AgenTerm @('set-tab-note', '-t', $name, $baseline) | Out-Null
    $args = @('set-tab-note', '-t', $name, 'expired-mutation-must-not-commit')
    $argsJson = ConvertTo-Json -InputObject $args -Compress
    $fingerprint = Get-Fnv1a64 (
        [Text.UTF8Encoding]::new($false).GetBytes($argsJson)
    )
    $requestId = "wake-expired-$($run.RunId)"
    $rawResponse = Invoke-RawIpc ([ordered]@{
        args = $args
        control = [ordered]@{
            schema_version = 1
            request_id = $requestId
            operation_id = 'command.set.tab.note'
            payload_fingerprint = $fingerprint
            intent = 'mutation'
            deadline_unix_ms = 0
        }
    })
    Add-SmokeCommandRecord -Context $run `
        -Arguments @('raw-ipc', 'expired-set-tab-note') `
        -ExitCode 1 -ExpectedFailure $true -Output $rawResponse
    $expired = $rawResponse | ConvertFrom-Json
    if (
        $expired.ok -or
        $expired.receipt.outcome -ne 'no_op' -or
        $expired.receipt.error.code -ne 'request_deadline_expired' -or
        $expired.receipt.error.category -ne 'timeout' -or
        $expired.receipt.request_id -ne $requestId
    ) {
        throw 'expired mutation did not return the expected typed no-op receipt'
    }
    $after = Invoke-AgenTerm @('show-tab-note', '-t', $name)
    if ($after -ne $baseline) {
        throw 'expired mutation changed tab state'
    }

    $runSucceeded = $true
    Write-Host 'PASS: coalesced wake delivery preserved IPC, PTY, and mutation correctness'
}
catch {
    $runFailure = $_
    throw
}
finally {
    foreach ($client in $clients) {
        try {
            if (-not $client.Process.HasExited) {
                $client.Process.Kill()
            }
            $client.Process.Dispose()
        }
        catch {
            # The shared harness performs exact-PID cleanup if needed.
        }
    }
    Complete-SmokeRun -Context $run -Succeeded $runSucceeded `
        -FailureRecord $runFailure
}
