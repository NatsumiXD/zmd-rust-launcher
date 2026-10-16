[CmdletBinding()]
param([switch]$MetadataOnly)

$ErrorActionPreference = 'Stop'
$taskProjectRoot = [IO.Path]::GetFullPath((Split-Path -Parent $PSScriptRoot))
$taskEndpoint = 'https://launcher.hypergryph.com/api/launcher/get_latest'
$taskQuery = 'appcode=abYeZZ16BPluCFyT&channel=1&sub_channel=1&version=0.0.0&target_app=EndField'
$taskExpectedPublisher = 'Shanghai Hypergryph Network Technology Co., Ltd.'
$taskStage = $null
$taskPackage = $null
$taskGames = $null
$taskComplete = $false
$taskHTTP = $null

function Copy-BoundedStream {
    param([IO.Stream]$InputStream, [IO.Stream]$OutputStream, [long]$ExpectedBytes)
    $taskBuffer = New-Object byte[] 131072
    [long]$taskCopied = 0
    while (($taskRead = $InputStream.Read($taskBuffer, 0, $taskBuffer.Length)) -gt 0) {
        $taskCopied += $taskRead
        if ($taskCopied -gt $ExpectedBytes) { throw 'Received more bytes than declared.' }
        $OutputStream.Write($taskBuffer, 0, $taskRead)
    }
    if ($taskCopied -ne $ExpectedBytes) { throw 'Received length differs from declared length.' }
}

try {
    # Public release metadata only; do not copy user/device headers or account state.
    $taskResponse = Invoke-WebRequest -Uri ($taskEndpoint + '?' + $taskQuery) -Method Get -TimeoutSec 30 -MaximumRedirection 0
    if ([int]$taskResponse.StatusCode -ne 200) { throw 'Release metadata HTTP status is not 200.' }
    $taskMetadata = $taskResponse.Content | ConvertFrom-Json
    foreach ($taskField in @('action','version','request_version','zip_package_url','md5','package_size','total_size','description')) {
        if ($null -eq $taskMetadata.PSObject.Properties[$taskField]) { throw "Missing release field: $taskField" }
    }
    if ([int]$taskMetadata.action -ne 1 -or [string]$taskMetadata.request_version -ne '0.0.0') { throw 'Release does not provide a full update for version 0.0.0.' }
    $taskVersion = [string]$taskMetadata.version
    if ($taskVersion -notmatch '^\d+(?:\.\d+){1,3}$') { throw 'Unsupported release version format.' }
    $taskMD5 = [string]$taskMetadata.md5
    if ($taskMD5 -notmatch '^[0-9a-fA-F]{32}$') { throw 'Invalid package MD5 format.' }
    [long]$taskPackageSize = 0
    if (-not [long]::TryParse([string]$taskMetadata.package_size, [ref]$taskPackageSize) -or $taskPackageSize -le 0 -or $taskPackageSize -gt 536870912) { throw 'Package size is invalid or exceeds 512 MiB.' }
    $taskURL = [uri][string]$taskMetadata.zip_package_url
    if ($taskURL.Scheme -ne 'https' -or $taskURL.Host -ne 'launcher-sign.hycdn.cn' -or $taskURL.Port -ne 443 -or $taskURL.UserInfo -ne '' -or $taskURL.AbsolutePath -notlike '/abYeZZ16BPluCFyT/launcher/1/1/endfield/*') { throw 'Unexpected package origin or release path.' }

    $taskRecord = [ordered]@{
        QueriedAt=(Get-Date).ToString('o'); Endpoint=$taskEndpoint; Method='GET'
        AppCode='abYeZZ16BPluCFyT'; Channel='1'; SubChannel='1'; TargetApp='EndField'
        RequestVersion='0.0.0'; Version=$taskVersion; Action=[int]$taskMetadata.action
        PackageSize=$taskPackageSize; TotalSize=[string]$taskMetadata.total_size; PackageMD5=$taskMD5.ToLowerInvariant()
        PackageURLWithoutQuery=$taskURL.GetLeftPart([UriPartial]::Path); HasSignedQuery=($taskURL.Query.Length -gt 0)
    }
    if ($MetadataOnly) {
        [pscustomobject]$taskRecord
        return
    }

    # Every output is under this source project's artifacts tree; never update an installation.
    $taskOutputRoot = [IO.Path]::GetFullPath((Join-Path $taskProjectRoot 'artifacts\official-games'))
    $taskInstallRoot = [IO.Path]::GetFullPath('D:\yj\Hypergryph Launcher').TrimEnd('\')
    if ($taskOutputRoot.Equals($taskInstallRoot, [StringComparison]::OrdinalIgnoreCase) -or $taskOutputRoot.StartsWith($taskInstallRoot+'\', [StringComparison]::OrdinalIgnoreCase)) { throw 'Output must be outside the installation.' }
    # Reject existing junctions/symlinks on the output chain.
    $taskWalk = $taskOutputRoot
    while ($taskWalk) {
        if (Test-Path -LiteralPath $taskWalk) {
            if (((Get-Item -LiteralPath $taskWalk -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) { throw 'Output chain contains a reparse point.' }
        }
        $taskWalk = [IO.Path]::GetDirectoryName($taskWalk)
    }
    $taskStage = Join-Path $taskOutputRoot ($taskVersion + '-' + [guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $taskStage | Out-Null
    $taskPackage = Join-Path $taskStage 'launcher.zip'
    $taskGames = Join-Path $taskStage 'Games.exe'
    Write-Host "Downloading launcher $taskVersion ($taskPackageSize bytes) to the project artifacts directory."

    # Keep temporary CDN signatures in memory; do not log them or follow redirects to new origins.
    Add-Type -AssemblyName System.Net.Http
    $taskHandler = New-Object Net.Http.HttpClientHandler
    $taskHandler.AllowAutoRedirect = $false
    $taskHTTP = New-Object Net.Http.HttpClient($taskHandler)
    $taskHTTP.Timeout = [TimeSpan]::FromMinutes(3)
    $taskDownload = $taskHTTP.GetAsync($taskURL, [Net.Http.HttpCompletionOption]::ResponseHeadersRead).GetAwaiter().GetResult()
    try {
        if ([int]$taskDownload.StatusCode -ne 200) { throw 'Package HTTP status is not 200.' }
        if ($null -ne $taskDownload.Content.Headers.ContentLength -and $taskDownload.Content.Headers.ContentLength -ne $taskPackageSize) { throw 'Package Content-Length differs from release metadata.' }
        $taskInput = $taskDownload.Content.ReadAsStreamAsync().GetAwaiter().GetResult()
        $taskOutput = [IO.File]::Open($taskPackage, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
        try { Copy-BoundedStream $taskInput $taskOutput $taskPackageSize }
        finally { $taskOutput.Dispose(); $taskInput.Dispose() }
    } finally { $taskDownload.Dispose() }
    $taskActualMD5 = (Get-FileHash -LiteralPath $taskPackage -Algorithm MD5).Hash
    if (-not $taskActualMD5.Equals($taskMD5, [StringComparison]::OrdinalIgnoreCase)) { throw 'Package MD5 differs from official metadata.' }

    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $taskArchive = [IO.Compression.ZipFile]::OpenRead($taskPackage)
    try {
        $taskEntries = @($taskArchive.Entries | Where-Object { $_.Name -ceq 'Games.exe' })
        if ($taskEntries.Count -ne 1) { throw 'Expected exactly one Games.exe in the release archive.' }
        $taskEntry = $taskEntries[0]
        if ($taskEntry.Length -le 0 -or $taskEntry.Length -gt 134217728) { throw 'Games.exe archive entry size is invalid or exceeds 128 MiB.' }
        $taskEntryName = $taskEntry.FullName
        # Write one exact file name; ZIP member paths are never used as filesystem destinations.
        $taskInput = $taskEntry.Open()
        $taskOutput = [IO.File]::Open($taskGames, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
        try { Copy-BoundedStream $taskInput $taskOutput $taskEntry.Length }
        finally { $taskOutput.Dispose(); $taskInput.Dispose() }
        $taskRecord.ArchiveEntries = $taskArchive.Entries.Count
        $taskRecord.SelectedEntry = $taskEntryName
        $taskRecord.GamesSize = $taskEntry.Length
    } finally { $taskArchive.Dispose() }

    $taskSignature = Get-AuthenticodeSignature -LiteralPath $taskGames
    if ($taskSignature.Status -ne 'Valid' -or $null -eq $taskSignature.SignerCertificate) { throw 'Games.exe Authenticode verification failed.' }
    $taskPublisher = $taskSignature.SignerCertificate.GetNameInfo([Security.Cryptography.X509Certificates.X509NameType]::SimpleName, $false)
    if (-not $taskPublisher.Equals($taskExpectedPublisher, [StringComparison]::Ordinal)) { throw 'Games.exe publisher differs from the expected official publisher.' }
    $taskRecord.GamesPath = $taskGames
    $taskRecord.GamesSHA256 = (Get-FileHash -LiteralPath $taskGames -Algorithm SHA256).Hash
    $taskRecord.PackageSHA256 = (Get-FileHash -LiteralPath $taskPackage -Algorithm SHA256).Hash
    $taskRecord.SignatureStatus = $taskSignature.Status.ToString()
    $taskRecord.Publisher = $taskPublisher
    $taskRecord.SignerThumbprint = $taskSignature.SignerCertificate.Thumbprint
    $taskRecord.CompletedAt = (Get-Date).ToString('o')
    $taskRecord.ExecutedDownloadedCode = $false
    $taskRecord.ReceiptPath = Join-Path $taskStage 'receipt.json'
    $taskRecord | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $taskRecord.ReceiptPath -Encoding UTF8
    $taskComplete = $true
    [pscustomobject]$taskRecord
} catch {
    # Exception URLs can contain temporary auth_key values. Expose a bounded diagnostic only.
    $taskDiagnostic = $_.Exception.Message
    if ($taskDiagnostic -match 'https?://|auth_key|\?') { $taskDiagnostic = 'Network or parsing operation failed; temporary URLs omitted.' }
    throw "Official Games acquisition failed: $taskDiagnostic"
} finally {
    if ($null -ne $taskHTTP) { $taskHTTP.Dispose() }
    if (-not $taskComplete -and $taskGames -and (Test-Path -LiteralPath $taskGames)) {
        Remove-Item -LiteralPath $taskGames -Force
    }
}
