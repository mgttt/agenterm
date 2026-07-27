function Get-AgenTermArtifactManifest {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Path
    )

    $manifestPath = (Resolve-Path -LiteralPath $Path).Path
    $manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
    if ($manifest.schema_version -ne 1) {
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
        if ([uint64]$entry.release_budget_bytes -eq 0) {
            throw "Artifact '$($entry.name)' has no release budget."
        }
    }

    return $manifest
}
