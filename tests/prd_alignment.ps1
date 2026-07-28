param(
    [string]$CliExe = (Join-Path $PSScriptRoot '..\dist\agenterm-cli.exe'),
    [string]$MuxExe = (Join-Path $PSScriptRoot '..\dist\agenterm-mux.exe')
)

$ErrorActionPreference = 'Stop'
$root = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$prdPath = Join-Path $root 'PRD.md'
$prdDetailDirectory = Join-Path $root 'prd'
$contractPath = Join-Path $prdDetailDirectory 'alignment-contract.json'
$CliExe = [IO.Path]::GetFullPath($CliExe)
$MuxExe = [IO.Path]::GetFullPath($MuxExe)

foreach ($path in @($prdPath, $prdDetailDirectory, $contractPath, $CliExe, $MuxExe)) {
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

$prdIndex = Get-Content -LiteralPath $prdPath -Raw
$linkedDetailNames = @(
    [regex]::Matches($prdIndex, '\(prd/(?<file>PRD_[^)]+\.md)\)') |
        ForEach-Object { $_.Groups['file'].Value }
)
$actualDetailNames = @(
    Get-ChildItem -LiteralPath $prdDetailDirectory -File -Filter 'PRD_*.md' |
        Sort-Object Name |
        ForEach-Object Name
)
Compare-ExactList `
    -Label 'PRD index links and detail modules' `
    -Expected @($actualDetailNames) `
    -Actual @($linkedDetailNames | Sort-Object)
$prdDocuments = @($prdPath) + @(
    $linkedDetailNames | ForEach-Object {
        Join-Path $prdDetailDirectory $_
    }
)
$prd = ($prdDocuments | ForEach-Object {
    Get-Content -LiteralPath $_ -Raw
}) -join "`n"
$contract = Get-Content -LiteralPath $contractPath -Raw | ConvertFrom-Json
if ($contract.schema_version -ne 2) {
    throw "Unsupported PRD alignment schema: $($contract.schema_version)"
}

$allowedKinds = @('architecture', 'behavior', 'decision', 'visual')
$allowedEvidenceModes = @(
    'black-box'
    'decision'
    'unit-source-partial'
)
$capabilities = @($contract.capabilities)
if ($capabilities.Count -eq 0) {
    throw 'PRD alignment contract has no capabilities.'
}
$capabilityIds = @($capabilities.id)
if (@($capabilityIds | Sort-Object -Unique).Count -ne $capabilityIds.Count) {
    throw 'PRD alignment contract contains duplicate capability IDs.'
}
foreach ($capability in $capabilities) {
    if ([string]::IsNullOrWhiteSpace($capability.id) -or
        $capability.id -notmatch '^[a-z0-9]+(?:[.-][a-z0-9]+)*$') {
        throw "Malformed capability ID: $($capability.id)"
    }
    if ($allowedKinds -notcontains $capability.kind) {
        throw "Capability '$($capability.id)' has unknown kind '$($capability.kind)'."
    }
    if ($allowedEvidenceModes -notcontains $capability.evidence_mode) {
        throw "Capability '$($capability.id)' has unknown evidence mode '$($capability.evidence_mode)'."
    }
    if ($capability.kind -eq 'decision') {
        if ($capability.status -ne 'accepted' -or
            $capability.evidence_mode -ne 'decision' -or
            @($capability.evidence_ids).Count -ne 0) {
            throw "Decision '$($capability.id)' must be accepted with decision mode and no test evidence."
        }
    }
    elseif ($capability.status -ne 'shipped' -or
        $capability.evidence_mode -eq 'decision' -or
        @($capability.evidence_ids).Count -eq 0) {
        throw "Shipped capability '$($capability.id)' must declare executable evidence."
    }
    $shippedPattern = "(?m)^\s*-\s+\[x\][^\r\n]*$([regex]::Escape($capability.prd))"
    if ($prd -notmatch $shippedPattern) {
        throw "Capability '$($capability.id)' is not declared [x] on its referenced PRD line: $($capability.prd)"
    }
}

$registeredEvidence = [Collections.Generic.HashSet[string]]::new(
    [StringComparer]::Ordinal
)
foreach ($suite in @(
    'cli_smoke.ps1'
    'fleet_smoke.ps1'
    'script_smoke.ps1'
    'theme_smoke.ps1'
    'working_context_smoke.ps1'
    'proxy_smoke.ps1'
    'workbench_smoke.ps1'
    'ux_smoke.ps1'
)) {
    $suitePath = Join-Path $PSScriptRoot $suite
    $suiteEvidence = @(& $suitePath -ListEvidence 2>&1)
    if ($LASTEXITCODE -ne 0) {
        throw "$suite -ListEvidence failed:`n$($suiteEvidence -join "`n")"
    }
    $suiteSource = Get-Content -LiteralPath $suitePath -Raw
    foreach ($evidenceId in @($suiteEvidence)) {
        $evidenceId = "$evidenceId".Trim()
        if ($evidenceId -notmatch '^[a-z0-9]+(?:[.-][a-z0-9]+)*$') {
            throw "$suite registered malformed evidence ID: $evidenceId"
        }
        if (-not $registeredEvidence.Add($evidenceId)) {
            throw "Duplicate executable evidence ID: $evidenceId"
        }
        $emission = "Write-Evidence '$evidenceId'"
        if (-not $suiteSource.Contains($emission)) {
            throw "$suite advertises '$evidenceId' but never emits it after an assertion."
        }
    }
}

$rmuxStatusSource = Get-Content -LiteralPath (
    Join-Path $root 'src\rmux_status.rs'
) -Raw
foreach ($unitName in @(
    'parses_rmux_status_windows_and_active_marker'
    'records_clickable_utf8_byte_ranges'
)) {
    if (-not $rmuxStatusSource.Contains("fn $unitName")) {
        throw "RMUX partial unit evidence is missing test: $unitName"
    }
}
[void]$registeredEvidence.Add('unit.rmux-status-parser')

$expectedEvidence = @(
    $capabilities |
        Where-Object kind -ne 'decision' |
        ForEach-Object { @($_.evidence_ids) } |
        Sort-Object
)
$actualEvidence = @($registeredEvidence | Sort-Object)
Compare-ExactList -Label 'shipped capability and executable evidence IDs' `
    -Expected $expectedEvidence -Actual $actualEvidence

$runtimeCatalog = @(& $CliExe list-commands 2>&1)
if ($LASTEXITCODE -ne 0) {
    throw "agenterm-cli list-commands failed:`n$($runtimeCatalog -join "`n")"
}
$catalogLines = @(
    $runtimeCatalog |
        ForEach-Object { "$_".Trim() } |
        Where-Object { $_ }
)

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
        throw "Public command '$name' is implemented but absent from the PRD product set."
    }
}
foreach ($rootName in @($contract.planned_command_roots)) {
    if ($publicNames.Contains($rootName)) {
        throw "Planned command root '$rootName' is already public; update its PRD state and contract."
    }
}

$protocol = Invoke-JsonCommand -Path $CliExe -CommandArgs @('protocol-info')
if ($protocol.command_catalog.schema_version -ne 1) {
    throw "Unsupported command catalog schema: $($protocol.command_catalog.schema_version)"
}
$discoveredCatalog = @(
    $protocol.command_catalog.commands |
        ForEach-Object {
            if (@($_.aliases).Count -eq 0) {
                $_.id
            }
            else {
                "$($_.id) ($(@($_.aliases) -join ', '))"
            }
        }
)
Compare-ExactList -Label 'list-commands and protocol command catalog' `
    -Expected $catalogLines -Actual $discoveredCatalog

$runtimeFeatureNames = @($protocol.features.PSObject.Properties.Name | Sort-Object)
$protocolCapabilities = @(
    $capabilities |
        Where-Object { -not [string]::IsNullOrWhiteSpace($_.protocol_feature) }
)
$contractFeatureNames = @($protocolCapabilities.protocol_feature | Sort-Object)
Compare-ExactList -Label 'protocol features and PRD evidence contract' `
    -Expected $contractFeatureNames -Actual $runtimeFeatureNames

foreach ($feature in $protocol.features.PSObject.Properties) {
    if ($feature.Value -ne $true) {
        throw "protocol-info advertises non-shipped feature '$($feature.Name)'."
    }
    $declaration = @(
        $protocolCapabilities |
            Where-Object protocol_feature -eq $feature.Name
    )
    if ($declaration.Count -ne 1) {
        throw "Protocol feature '$($feature.Name)' must map to exactly one capability ID."
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
        throw "Mux compatibility command '$name' is absent from the PRD product set."
    }
}

Write-Host (
    (
        "PASS: PRD aligns with {0} catalog entries, {1} public names, " +
        "{2} protocol features, {3} mux commands, {4} capability IDs, " +
        "and {5} executable evidence IDs"
    ) -f
    $catalogLines.Count,
    $publicNames.Count,
    $runtimeFeatureNames.Count,
    $muxLines.Count,
    $capabilityIds.Count,
    $actualEvidence.Count
)
