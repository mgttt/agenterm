param(
    [Parameter(Mandatory = $true)][string]$ReadyPath,
    [Parameter(Mandatory = $true)][string]$LogPath,
    [Parameter(Mandatory = $true)][string]$StopPath,
    [int]$MaxRequests = 9,
    [int]$IdleTimeoutMs = 10000
)

$ErrorActionPreference = 'Stop'

function Find-HeaderEnd {
    param([Parameter(Mandatory = $true)][byte[]]$Bytes)
    for ($index = 0; $index + 3 -lt $Bytes.Length; $index++) {
        if ($Bytes[$index] -eq 13 -and
            $Bytes[$index + 1] -eq 10 -and
            $Bytes[$index + 2] -eq 13 -and
            $Bytes[$index + 3] -eq 10) {
            return $index
        }
    }
    return -1
}

function Write-Response {
    param(
        [Parameter(Mandatory = $true)][IO.Stream]$Stream,
        [int]$Status = 200,
        [string]$Reason = 'OK',
        [byte[]]$Body = [byte[]]@(),
        [string[]]$Headers = @()
    )
    $head = @(
        "HTTP/1.1 $Status $Reason"
        $Headers
        "Content-Length: $($Body.Length)"
        'Connection: close'
        ''
        ''
    ) -join "`r`n"
    $headBytes = [Text.Encoding]::ASCII.GetBytes($head)
    $Stream.Write($headBytes, 0, $headBytes.Length)
    if ($Body.Length -gt 0) {
        $Stream.Write($Body, 0, $Body.Length)
    }
    $Stream.Flush()
}

function Add-RequestLog {
    param([hashtable]$Record)
    Add-Content -LiteralPath $LogPath -Value (
        $Record | ConvertTo-Json -Compress
    ) -Encoding UTF8
}

$readyFullPath = [IO.Path]::GetFullPath($ReadyPath)
$logFullPath = [IO.Path]::GetFullPath($LogPath)
$stopFullPath = [IO.Path]::GetFullPath($StopPath)
$readyParent = [IO.Path]::GetDirectoryName($readyFullPath)
$logParent = [IO.Path]::GetDirectoryName($logFullPath)
foreach ($directory in @($readyParent, $logParent) | Select-Object -Unique) {
    [IO.Directory]::CreateDirectory($directory) | Out-Null
}

$listener = [Net.Sockets.TcpListener]::new(
    [Net.IPAddress]::Loopback,
    0
)
$listener.Start()
$port = ([Net.IPEndPoint]$listener.LocalEndpoint).Port
$ready = [ordered]@{
    schema_version = 1
    url = "http://127.0.0.1:$port"
    tls_url = "https://127.0.0.1:$port"
    pid = $PID
}
$temporaryReady = "$readyFullPath.tmp-$PID"
[IO.File]::WriteAllText(
    $temporaryReady,
    ($ready | ConvertTo-Json -Compress),
    [Text.UTF8Encoding]::new($false)
)
[IO.File]::Move($temporaryReady, $readyFullPath)

$handled = 0
$deadline = [DateTime]::UtcNow.AddMilliseconds($IdleTimeoutMs)
try {
    while ($handled -lt $MaxRequests) {
        if (-not $listener.Pending()) {
            if (Test-Path -LiteralPath $stopFullPath) {
                break
            }
            if ([DateTime]::UtcNow -ge $deadline) {
                throw "HTTP fixture timed out after $handled requests"
            }
            Start-Sleep -Milliseconds 5
            continue
        }

        $client = $listener.AcceptTcpClient()
        $handled++
        $deadline = [DateTime]::UtcNow.AddMilliseconds($IdleTimeoutMs)
        try {
            $client.ReceiveTimeout = 2000
            $client.SendTimeout = 2000
            $stream = $client.GetStream()
            $buffer = [byte[]]::new(2048)
            $request = [IO.MemoryStream]::new()
            $headerEnd = -1
            $contentLength = 0

            while ($request.Length -lt 65536) {
                $read = $stream.Read($buffer, 0, $buffer.Length)
                if ($read -eq 0) {
                    break
                }
                $request.Write($buffer, 0, $read)
                $bytes = $request.ToArray()
                if ($bytes[0] -eq 0x16) {
                    Add-RequestLog @{ tls = $true; bytes = $bytes.Length }
                    $invalidTls = [Text.Encoding]::ASCII.GetBytes(
                        "HTTP/1.1 400 Bad Request`r`nConnection: close`r`nContent-Length: 0`r`n`r`n"
                    )
                    $stream.Write($invalidTls, 0, $invalidTls.Length)
                    $stream.Flush()
                    break
                }
                $headerEnd = Find-HeaderEnd -Bytes $bytes
                if ($headerEnd -lt 0) {
                    continue
                }
                $headerText = [Text.Encoding]::ASCII.GetString(
                    $bytes,
                    0,
                    $headerEnd
                )
                $contentLengthMatch = [regex]::Match(
                    $headerText,
                    '(?im)^Content-Length:\s*(\d+)\s*$'
                )
                if ($contentLengthMatch.Success) {
                    $contentLength = [int]$contentLengthMatch.Groups[1].Value
                }
                if ($bytes.Length -ge $headerEnd + 4 + $contentLength) {
                    break
                }
            }

            $bytes = $request.ToArray()
            if ($bytes.Length -eq 0 -or $bytes[0] -eq 0x16) {
                continue
            }
            if ($headerEnd -lt 0) {
                Add-RequestLog @{ malformed_request = $true; bytes = $bytes.Length }
                continue
            }

            $headerText = [Text.Encoding]::ASCII.GetString($bytes, 0, $headerEnd)
            $requestLine = $headerText.Split("`r`n")[0]
            $parts = $requestLine.Split(' ')
            $method = if ($parts.Length -ge 1) { $parts[0] } else { '' }
            $path = if ($parts.Length -ge 2) { $parts[1] } else { '' }
            $bodyOffset = $headerEnd + 4
            $body = [byte[]]::new($contentLength)
            if ($contentLength -gt 0) {
                [Array]::Copy($bytes, $bodyOffset, $body, 0, $contentLength)
            }
            Add-RequestLog @{
                method = $method
                path = $path
                body_bytes = $contentLength
            }

            switch ($path) {
                '/status' {
                    Write-Response -Stream $stream -Status 201 -Reason 'Created' `
                        -Headers @('X-Test: one', 'X-Test: two') `
                        -Body ([Text.Encoding]::UTF8.GetBytes('hello'))
                }
                '/echo' {
                    Write-Response -Stream $stream -Headers @("X-Method: $method") `
                        -Body $body
                }
                '/large' {
                    Write-Response -Stream $stream `
                        -Body ([Text.Encoding]::UTF8.GetBytes('abcdefgh'))
                }
                '/async' {
                    Write-Response -Stream $stream `
                        -Body ([Text.Encoding]::UTF8.GetBytes('async-ok'))
                }
                '/slow' {
                    Start-Sleep -Milliseconds 300
                    Write-Response -Stream $stream `
                        -Body ([Text.Encoding]::UTF8.GetBytes('too-late'))
                }
                '/cancel' {
                    Start-Sleep -Milliseconds 500
                    Write-Response -Stream $stream `
                        -Body ([Text.Encoding]::UTF8.GetBytes('cancel-late'))
                }
                '/malformed' {
                    $malformed = [Text.Encoding]::ASCII.GetBytes(
                        "NOT-HTTP`r`nContent-Length: 0`r`n`r`n"
                    )
                    $stream.Write($malformed, 0, $malformed.Length)
                    $stream.Flush()
                }
                '/disconnect' {
                    # Deliberately close without a response.
                }
                default {
                    Write-Response -Stream $stream -Status 404 -Reason 'Not Found'
                }
            }
        }
        catch [IO.IOException] {
            # Timeout and cancellation paths are expected to close their socket.
        }
        finally {
            $client.Dispose()
        }
    }
}
finally {
    $listener.Stop()
}

if ($handled -gt $MaxRequests) {
    throw "HTTP fixture exceeded its $MaxRequests request ceiling"
}
