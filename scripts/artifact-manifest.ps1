function Get-AgenTermArtifactManifest {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Path
    )

    $manifestPath = (Resolve-Path -LiteralPath $Path).Path
    $manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
    if ($manifest.schema_version -ne 2) {
        throw "Unsupported artifact manifest schema: $($manifest.schema_version)"
    }

    $entries = @($manifest.executables)
    if ($entries.Count -eq 0) {
        throw 'Artifact manifest contains no executables.'
    }

    $names = [Collections.Generic.HashSet[string]]::new(
        [StringComparer]::Ordinal
    )
    foreach ($entry in $entries) {
        if ($entry.name -notmatch '^agenterm(?:-[a-z]+)?\.exe$') {
            throw "Invalid artifact name: $($entry.name)"
        }
        if (-not $names.Add([string]$entry.name)) {
            throw "Duplicate artifact name: $($entry.name)"
        }
        if ([string]::IsNullOrWhiteSpace([string]$entry.role)) {
            throw "Artifact '$($entry.name)' has no role."
        }
        if ([int]$entry.pe_subsystem -notin @(2, 3)) {
            throw "Artifact '$($entry.name)' has an invalid PE subsystem."
        }
        if ([string]::IsNullOrWhiteSpace([string]$entry.documentation_role)) {
            throw "Artifact '$($entry.name)' has no documentation role."
        }
        $probe = @($entry.offline_probe)
        if ([int]$entry.pe_subsystem -eq 2 -and $probe.Count -ne 0) {
            throw "GUI artifact '$($entry.name)' must not have an offline launch probe."
        }
        if ([int]$entry.pe_subsystem -eq 3 -and $probe.Count -eq 0) {
            throw "Console artifact '$($entry.name)' must have an offline probe."
        }
        if (@($probe | Where-Object {
                    [string]::IsNullOrWhiteSpace([string]$_)
                }).Count -gt 0) {
            throw "Artifact '$($entry.name)' has an empty offline probe argument."
        }
        if ([uint64]$entry.release_budget_bytes -eq 0) {
            throw "Artifact '$($entry.name)' has no release budget."
        }
    }

    return $manifest
}
