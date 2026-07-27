param(
    [string]$CliExe = (Join-Path $PSScriptRoot '..\dist\agentermctl.exe'),
    [string]$MuxExe = (Join-Path $PSScriptRoot '..\dist\agenterm-mux.exe')
)

$ErrorActionPreference = 'Stop'
$root = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$prdPath = Join-Path $root 'docs\PRD.md'
$commandsPath = Join-Path $root 'src\commands.rs'
$CliExe = [IO.Path]::GetFullPath($CliExe)
$MuxExe = [IO.Path]::GetFullPath($MuxExe)

foreach ($path in @($prdPath, $commandsPath, $CliExe, $MuxExe)) {
    if (-not (Test-Path -LiteralPath $path)) {
        throw "PRD alignment input does not exist: $path"
    }
}

function Compare-ExactList {
    param(
        [Parameter(Mandatory = $true)][string]$Label,
        [Parameter(Mandatory = $true)][string[]]$Expected,
        [Parameter(Mandatory = $true)][string[]]$Actual
    )

    $expectedText = $Expected -join "`n"
    $actualText = $Actual -join "`n"
    if ($expectedText -ne $actualText) {
        $difference = Compare-Object -ReferenceObject $Expected -DifferenceObject $Actual |
            Out-String
        throw "$Label is out of alignment:`n$difference"
    }
}

function Invoke-JsonCommand {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][string[]]$CommandArgs
    )

    $output = & $Path @CommandArgs 2>&1
    if ($LASTEXITCODE -ne 0) {
        throw "$([IO.Path]::GetFileName($Path)) $($CommandArgs -join ' ') failed:`n$($output -join "`n")"
    }
    return (($output -join "`n") | ConvertFrom-Json)
}

$prd = Get-Content -LiteralPath $prdPath -Raw
$commandsSource = Get-Content -LiteralPath $commandsPath -Raw

$contractMatch = [regex]::Match(
    $prd,
    '(?s)<!--\s*agenterm-alignment-contract\s*(?<json>\{.*?\})\s*-->'
)
if (-not $contractMatch.Success) {
    throw 'PRD.md is missing the agenterm-alignment-contract JSON block.'
}
$contract = $contractMatch.Groups['json'].Value | ConvertFrom-Json
if ($contract.schema_version -ne 1) {
    throw "Unsupported PRD alignment schema: $($contract.schema_version)"
}

$catalogMatch = [regex]::Match(
    $commandsSource,
    '(?ms)pub\(crate\) const SUPPORTED_COMMANDS: &str = "\\\r?\n(?<body>.*?)";'
)
if (-not $catalogMatch.Success) {
    throw 'Could not read SUPPORTED_COMMANDS from src/commands.rs.'
}
$catalogLines = @(
    $catalogMatch.Groups['body'].Value -split '\r?\n' |
        ForEach-Object { $_.Trim() } |
        Where-Object { $_ }
)

$runtimeCatalog = @(& $CliExe list-commands 2>&1)
if ($LASTEXITCODE -ne 0) {
    throw "agentermctl list-commands failed:`n$($runtimeCatalog -join "`n")"
}
$runtimeCatalog = @($runtimeCatalog | ForEach-Object { "$_".Trim() } | Where-Object { $_ })
Compare-ExactList -Label 'source and runtime command catalogs' `
    -Expected $catalogLines -Actual $runtimeCatalog

$publicNames = [Collections.Generic.HashSet[string]]::new(
    [StringComparer]::Ordinal
)
foreach ($line in $catalogLines) {
    $lineMatch = [regex]::Match(
        $line,
        '^(?<name>[a-z0-9-]+)(?: \((?<aliases>[^)]+)\))?$'
    )
    if (-not $lineMatch.Success) {
        throw "Malformed command catalog line: $line"
    }
    [void]$publicNames.Add($lineMatch.Groups['name'].Value)
    if ($lineMatch.Groups['aliases'].Success) {
        foreach ($alias in $lineMatch.Groups['aliases'].Value -split '\s*,\s*') {
            [void]$publicNames.Add($alias)
        }
    }
}

foreach ($name in $publicNames) {
    $pattern = "(?<![A-Za-z0-9-])$([regex]::Escape($name))(?![A-Za-z0-9-])"
    if ($prd -notmatch $pattern) {
        throw "Public command '$name' is implemented but absent from PRD.md."
    }
}
foreach ($rootName in @($contract.planned_command_roots)) {
    if ($publicNames.Contains($rootName)) {
        throw "Planned command root '$rootName' is already public; update its PRD state and contract."
    }
}

$protocol = Invoke-JsonCommand -Path $CliExe -CommandArgs @('protocol-info')
$runtimeFeatureNames = @($protocol.features.PSObject.Properties.Name | Sort-Object)
$contractFeatureNames = @(
    $contract.runtime_features.PSObject.Properties.Name | Sort-Object
)
Compare-ExactList -Label 'protocol features and PRD evidence contract' `
    -Expected $contractFeatureNames -Actual $runtimeFeatureNames

foreach ($feature in $protocol.features.PSObject.Properties) {
    if ($feature.Value -ne $true) {
        throw "protocol-info advertises non-shipped feature '$($feature.Name)'."
    }
    $declaration = $contract.runtime_features.($feature.Name)
    $shippedPattern = "(?m)^\s*-\s+\[x\][^\r\n]*$([regex]::Escape($declaration.prd))"
    if ($prd -notmatch $shippedPattern) {
        throw "Feature '$($feature.Name)' is not declared [x] on its referenced PRD line: $($declaration.prd)"
    }
    if (@($declaration.evidence).Count -eq 0) {
        throw "Feature '$($feature.Name)' has no declared verification evidence."
    }
    foreach ($evidence in @($declaration.evidence)) {
        $evidencePath = Join-Path $root $evidence.path
        if (-not (Test-Path -LiteralPath $evidencePath)) {
            throw "Feature '$($feature.Name)' evidence file is missing: $($evidence.path)"
        }
        $evidenceText = Get-Content -LiteralPath $evidencePath -Raw
        if (-not $evidenceText.Contains($evidence.token)) {
            throw "Feature '$($feature.Name)' evidence token is missing from $($evidence.path): $($evidence.token)"
        }
    }
}

foreach ($extension in @($protocol.extensions)) {
    if (-not $publicNames.Contains($extension)) {
        throw "protocol-info extension '$extension' is absent from the public command catalog."
    }
}
foreach ($command in @(
    $protocol.compatibility.tmux_rmux
    $protocol.compatibility.partial
)) {
    if (-not $publicNames.Contains($command)) {
        throw "protocol-info compatibility command '$command' is absent from the public command catalog."
    }
}

$muxCompatibility = Invoke-JsonCommand -Path $MuxExe `
    -CommandArgs @('compatibility', '--json')
$muxLines = @(& $MuxExe list-commands 2>&1)
if ($LASTEXITCODE -ne 0) {
    throw "agenterm-mux list-commands failed:`n$($muxLines -join "`n")"
}
$muxLines = @($muxLines | ForEach-Object { "$_".Trim() } | Where-Object { $_ })
$supportedMuxLines = @($muxLines | Where-Object { $_ -notmatch ' \(unsupported:' })
$unsupportedMuxNames = @(
    $muxLines |
        Where-Object { $_ -match ' \(unsupported:' } |
        ForEach-Object { ($_ -split ' ', 2)[0] }
)
Compare-ExactList -Label 'mux supported registry and compatibility JSON' `
    -Expected @($muxCompatibility.supported) -Actual $supportedMuxLines
Compare-ExactList -Label 'mux unsupported registry and compatibility JSON' `
    -Expected @($muxCompatibility.explicitly_unsupported.name) `
    -Actual $unsupportedMuxNames

foreach ($name in @($muxCompatibility.supported) + $unsupportedMuxNames) {
    $pattern = "(?<![A-Za-z0-9-])$([regex]::Escape($name))(?![A-Za-z0-9-])"
    if ($prd -notmatch $pattern) {
        throw "Mux compatibility command '$name' is absent from PRD.md."
    }
}

Write-Host (
    (
        "PASS: PRD aligns with {0} catalog entries, {1} public names, " +
        "{2} protocol features, and {3} mux commands"
    ) -f
    $catalogLines.Count,
    $publicNames.Count,
    $runtimeFeatureNames.Count,
    $muxLines.Count
)
