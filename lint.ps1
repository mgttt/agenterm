param(
    [ValidateSet('All', 'Static', 'Rust', 'Rhai')]
    [string]$Mode = 'All',
    [string]$WorkerPath,
    [switch]$Json,
    [switch]$InternalSelfTest
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$repoRoot = $PSScriptRoot
$results = [Collections.Generic.List[object]]::new()
$started = [Diagnostics.Stopwatch]::StartNew()
$script:CargoProgram = 'cargo'

function Initialize-InstalledRustToolchain {
    $toolchainFile = Join-Path $repoRoot 'rust-toolchain.toml'
    $channelLine = Get-Content -LiteralPath $toolchainFile |
        Where-Object { $_ -match '^\s*channel\s*=' } |
        Select-Object -First 1
    if ($channelLine -notmatch '"(?<channel>[^"]+)"') {
        return
    }
    $channel = $Matches.channel
    $architecture = switch (
        [Runtime.InteropServices.RuntimeInformation]::OSArchitecture
    ) {
        ([Runtime.InteropServices.Architecture]::X64) { 'x86_64' }
        ([Runtime.InteropServices.Architecture]::Arm64) { 'aarch64' }
        default { return }
    }
    $platform = if (
        [Runtime.InteropServices.RuntimeInformation]::IsOSPlatform(
            [Runtime.InteropServices.OSPlatform]::Windows
        )
    ) {
        'pc-windows-msvc'
    }
    elseif (
        [Runtime.InteropServices.RuntimeInformation]::IsOSPlatform(
            [Runtime.InteropServices.OSPlatform]::Linux
        )
    ) {
        'unknown-linux-gnu'
    }
    elseif (
        [Runtime.InteropServices.RuntimeInformation]::IsOSPlatform(
            [Runtime.InteropServices.OSPlatform]::OSX
        )
    ) {
        'apple-darwin'
    }
    else {
        return
    }
    $rustupHome = if (-not [string]::IsNullOrWhiteSpace($env:RUSTUP_HOME)) {
        $env:RUSTUP_HOME
    }
    else {
        Join-Path ([Environment]::GetFolderPath('UserProfile')) '.rustup'
    }
    $bin = Join-Path $rustupHome (
        "toolchains\$channel-$architecture-$platform\bin"
    )
    $cargoName = if ($platform -eq 'pc-windows-msvc') {
        'cargo.exe'
    }
    else {
        'cargo'
    }
    $cargo = Join-Path $bin $cargoName
    if (-not (Test-Path -LiteralPath $cargo -PathType Leaf)) {
        return
    }
    $script:CargoProgram = $cargo
    $env:PATH = "$bin$([IO.Path]::PathSeparator)$env:PATH"
    $rustcName = if ($platform -eq 'pc-windows-msvc') {
        'rustc.exe'
    }
    else {
        'rustc'
    }
    $rustdocName = if ($platform -eq 'pc-windows-msvc') {
        'rustdoc.exe'
    }
    else {
        'rustdoc'
    }
    $env:RUSTC = Join-Path $bin $rustcName
    $env:RUSTDOC = Join-Path $bin $rustdocName
}

function Add-LintResult {
    param(
        [Parameter(Mandatory = $true)][string]$Id,
        [Parameter(Mandatory = $true)][bool]$Passed,
        [Parameter(Mandatory = $true)][long]$DurationMs,
        [string]$Message = ''
    )

    $results.Add([pscustomobject]@{
        id = $Id
        passed = $Passed
        duration_ms = $DurationMs
        message = $Message
    })
}

function Invoke-LintPhase {
    param(
        [Parameter(Mandatory = $true)][string]$Id,
        [Parameter(Mandatory = $true)][scriptblock]$Action
    )

    $watch = [Diagnostics.Stopwatch]::StartNew()
    try {
        & $Action
        Add-LintResult -Id $Id -Passed $true -DurationMs $watch.ElapsedMilliseconds
        if (-not $Json) {
            Write-Host "PASS lint $Id ($($watch.ElapsedMilliseconds) ms)"
        }
    }
    catch {
        $message = ($_ | Out-String).Trim()
        Add-LintResult -Id $Id -Passed $false `
            -DurationMs $watch.ElapsedMilliseconds -Message $message
        throw
    }
}

function Get-TrackedFiles {
    param([Parameter(Mandatory = $true)][string[]]$Patterns)

    $files = @(& git -C $repoRoot ls-files -- @Patterns 2>&1)
    if ($LASTEXITCODE -ne 0) {
        throw "git ls-files failed: $($files -join [Environment]::NewLine)"
    }
    return @($files | ForEach-Object {
        Join-Path $repoRoot ([string]$_)
    })
}

function Assert-PowerShellSyntax {
    param([Parameter(Mandatory = $true)][string[]]$Paths)

    $problems = [Collections.Generic.List[string]]::new()
    foreach ($path in $Paths) {
        $tokens = $null
        $errors = $null
        [Management.Automation.Language.Parser]::ParseFile(
            $path,
            [ref]$tokens,
            [ref]$errors
        ) | Out-Null
        foreach ($error in @($errors)) {
            $relative = [IO.Path]::GetRelativePath($repoRoot, $path)
            $problems.Add(
                "${relative}:$($error.Extent.StartLineNumber):" +
                "$($error.Extent.StartColumnNumber): $($error.Message)"
            )
        }
    }
    if ($problems.Count -gt 0) {
        throw "PowerShell parse errors:`n$($problems -join "`n")"
    }
}

function Assert-JsonSyntax {
    param([Parameter(Mandatory = $true)][string[]]$Paths)

    $problems = [Collections.Generic.List[string]]::new()
    foreach ($path in $Paths) {
        try {
            Get-Content -LiteralPath $path -Raw |
                ConvertFrom-Json -ErrorAction Stop | Out-Null
        }
        catch {
            $relative = [IO.Path]::GetRelativePath($repoRoot, $path)
            $problems.Add("${relative}: $($_.Exception.Message)")
        }
    }
    if ($problems.Count -gt 0) {
        throw "JSON parse errors:`n$($problems -join "`n")"
    }
}

function Assert-TrackedTextHygiene {
    param([Parameter(Mandatory = $true)][string[]]$Paths)

    $strictUtf8 = [Text.UTF8Encoding]::new($false, $true)
    $problems = [Collections.Generic.List[string]]::new()
    foreach ($path in $Paths) {
        try {
            $text = [IO.File]::ReadAllText($path, $strictUtf8)
        }
        catch {
            $relative = [IO.Path]::GetRelativePath($repoRoot, $path)
            $problems.Add("${relative}: invalid UTF-8")
            continue
        }
        if ($text -match '(?m)^(<<<<<<<|=======|>>>>>>>)') {
            $relative = [IO.Path]::GetRelativePath($repoRoot, $path)
            $problems.Add("${relative}: unresolved merge-conflict marker")
        }
        if ($text.Contains([char]0)) {
            $relative = [IO.Path]::GetRelativePath($repoRoot, $path)
            $problems.Add("${relative}: embedded NUL byte")
        }
    }
    if ($problems.Count -gt 0) {
        throw "Tracked text hygiene errors:`n$($problems -join "`n")"
    }
}

function Invoke-NativeLint {
    param(
        [Parameter(Mandatory = $true)][string]$Program,
        [Parameter(Mandatory = $true)][string[]]$Arguments
    )

    $output = @(& $Program @Arguments 2>&1)
    if ($LASTEXITCODE -ne 0) {
        throw "$Program $($Arguments -join ' ') failed:`n$($output -join "`n")"
    }
}

function Resolve-ScriptWorker {
    if (-not [string]::IsNullOrWhiteSpace($WorkerPath)) {
        $candidate = if ([IO.Path]::IsPathRooted($WorkerPath)) {
            $WorkerPath
        }
        else {
            Join-Path $repoRoot $WorkerPath
        }
        $resolved = [IO.Path]::GetFullPath($candidate)
        if (-not (Test-Path -LiteralPath $resolved -PathType Leaf)) {
            throw "Rhai lint worker does not exist: $resolved"
        }
        return $resolved
    }

    Invoke-NativeLint -Program $script:CargoProgram -Arguments @(
        'build', '--locked', '--bin', 'agenterm-script'
    )
    $isWindowsPlatform = [Runtime.InteropServices.RuntimeInformation]::IsOSPlatform(
        [Runtime.InteropServices.OSPlatform]::Windows
    )
    $name = if ($isWindowsPlatform) {
        'agenterm-script.exe'
    }
    else {
        'agenterm-script'
    }
    return Join-Path $repoRoot "target\debug\$name"
}

function Assert-RhaiSources {
    param(
        [Parameter(Mandatory = $true)][string]$Worker,
        [Parameter(Mandatory = $true)][string[]]$Paths
    )

    $problems = [Collections.Generic.List[string]]::new()
    $sequence = 0
    foreach ($path in $Paths) {
        $sequence += 1
        $source = [IO.File]::ReadAllText($path)
        $relative = [IO.Path]::GetRelativePath($repoRoot, $path)
        $invocation = [ordered]@{
            envelope_version = 2
            invocation_id = "lint-$PID-$sequence"
            api_version = 2
            operation = 'check'
            profile = 'local'
            source_label = $relative
            source = $source
            project_root = $repoRoot
            arguments = @()
            budgets = [ordered]@{
                source_bytes = 262144
                operations = 1000000
                call_depth = 64
                expression_depth = 64
                collection_items = 10000
                string_bytes = 262144
                output_bytes = 65536
                wall_time_ms = 2000
                broker_requests = 64
                broker_return_bytes = 262144
                capture_bytes = 65536
                event_items = 256
                wait_time_ms = 2000
            }
        }
        $wire = $invocation | ConvertTo-Json -Compress -Depth 10
        $output = @($wire | & $Worker --worker 2>&1)
        $exitCode = $LASTEXITCODE
        try {
            $result = ($output -join "`n") | ConvertFrom-Json -ErrorAction Stop
        }
        catch {
            $problems.Add("${relative}: worker returned invalid JSON")
            continue
        }
        if ($exitCode -ne 0 -or -not $result.ok) {
            $code = [string]$result.failure.code
            $message = [string]$result.failure.message
            $problems.Add("${relative}: ${code}: $message")
        }
    }
    if ($problems.Count -gt 0) {
        throw "Rhai lint errors:`n$($problems -join "`n")"
    }
}

function Invoke-InternalSelfTest {
    $temporary = Join-Path ([IO.Path]::GetTempPath()) "agenterm-lint-$PID"
    New-Item -ItemType Directory -Path $temporary -Force | Out-Null
    try {
        $badPowerShell = Join-Path $temporary 'bad.ps1'
        $badJson = Join-Path $temporary 'bad.json'
        [IO.File]::WriteAllText($badPowerShell, 'if ($true) {')
        [IO.File]::WriteAllText($badJson, '{"missing":')
        $caughtPowerShell = $false
        $caughtJson = $false
        try {
            Assert-PowerShellSyntax -Paths @($badPowerShell)
        }
        catch {
            $caughtPowerShell = $true
        }
        try {
            Assert-JsonSyntax -Paths @($badJson)
        }
        catch {
            $caughtJson = $true
        }
        if (-not $caughtPowerShell -or -not $caughtJson) {
            throw 'lint self-test did not reject malformed PowerShell and JSON'
        }
    }
    finally {
        Remove-Item -LiteralPath $temporary -Recurse -Force `
            -ErrorAction SilentlyContinue
    }
}

$failed = $null
Push-Location $repoRoot
try {
    if ($Mode -in @('All', 'Rust') -or
        ($Mode -eq 'Rhai' -and [string]::IsNullOrWhiteSpace($WorkerPath))) {
        Initialize-InstalledRustToolchain
    }
    if ($InternalSelfTest) {
        Invoke-LintPhase -Id 'self-test' {
            Invoke-InternalSelfTest
        }
    }
    else {
        if ($Mode -in @('All', 'Static')) {
            Invoke-LintPhase -Id 'powershell-ast' {
                Assert-PowerShellSyntax -Paths (
                    Get-TrackedFiles -Patterns @('*.ps1', '*.psm1')
                )
            }
            Invoke-LintPhase -Id 'json' {
                Assert-JsonSyntax -Paths (
                    Get-TrackedFiles -Patterns @('*.json')
                )
            }
            Invoke-LintPhase -Id 'text-hygiene' {
                Assert-TrackedTextHygiene -Paths (
                    Get-TrackedFiles -Patterns @(
                        '*.rs', '*.toml', '*.ps1', '*.psm1', '*.rhai',
                        '*.json', '*.md', '*.html', '*.yml', '*.yaml',
                        '*.sh', '*.bat'
                    )
                )
            }
        }
        if ($Mode -in @('All', 'Rust')) {
            Invoke-LintPhase -Id 'rustfmt' {
                Invoke-NativeLint -Program $script:CargoProgram -Arguments @(
                    'fmt', '--all', '--', '--check'
                )
            }
            Invoke-LintPhase -Id 'clippy' {
                Invoke-NativeLint -Program $script:CargoProgram -Arguments @(
                    'clippy', '--locked', '--all-targets', '--all-features',
                    '--', '-D', 'warnings'
                )
            }
        }
        if ($Mode -in @('All', 'Rhai')) {
            Invoke-LintPhase -Id 'rhai-check' {
                $worker = Resolve-ScriptWorker
                Assert-RhaiSources -Worker $worker -Paths (
                    Get-TrackedFiles -Patterns @('scripts/rhai/*.rhai')
                )
            }
        }
    }
}
catch {
    $failed = $_
}
finally {
    Pop-Location
}

$report = [ordered]@{
    schema_version = 1
    ok = $null -eq $failed
    mode = if ($InternalSelfTest) { 'SelfTest' } else { $Mode }
    duration_ms = $started.ElapsedMilliseconds
    phases = @($results)
}
if ($Json) {
    $report | ConvertTo-Json -Depth 5
}
elseif ($null -eq $failed) {
    Write-Host "PASS: repository lint ($($started.ElapsedMilliseconds) ms)"
}
else {
    Write-Error (($failed | Out-String).Trim())
}

if ($null -ne $failed) {
    exit 1
}
