# Packages only the built Rust launcher and documentation; never runs the EXE.
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$ExecutablePath,
    [Parameter(Mandatory)][string]$OutputDirectory,
    [Parameter(Mandatory)][ValidatePattern('^[0-9]+\.[0-9]+$')][string]$BuildId,
    [Parameter(Mandatory)][ValidatePattern('^[a-fA-F0-9]{40}$')][string]$CommitSha
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$taskRoot = Split-Path -Parent $PSScriptRoot
$taskExecutable = (Resolve-Path -LiteralPath $ExecutablePath).Path
$taskOutput = [IO.Path]::GetFullPath($OutputDirectory)
if ((Test-Path -LiteralPath $taskOutput) -and
    (Get-ChildItem -LiteralPath $taskOutput -Force | Select-Object -First 1)) {
    throw 'Release output directory must be empty; choose a new output directory.'
}

$taskMetadata = cargo +stable metadata --locked --no-deps --format-version 1 --manifest-path (Join-Path $taskRoot 'Cargo.toml') | ConvertFrom-Json
if ($LASTEXITCODE -ne 0) { throw 'Cannot read Cargo metadata' }
$taskPackage = @($taskMetadata.packages | Where-Object { $_.name -eq 'endfield-direct-launch' })
if ($taskPackage.Count -ne 1) { throw 'Launcher package not found' }
$taskVersion = $taskPackage[0].version
if ($taskVersion -notmatch '^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$') {
    throw 'Unexpected package version'
}

# Inspect the PE header and embedded manifest; do not execute an elevated binary.
$taskReader = [IO.BinaryReader]::new([IO.File]::OpenRead($taskExecutable))
try {
    if ($taskReader.ReadUInt16() -ne 0x5a4d) { throw 'Missing DOS executable header' }
    $taskReader.BaseStream.Position = 0x3c
    $taskPeOffset = $taskReader.ReadUInt32()
    $taskReader.BaseStream.Position = $taskPeOffset
    if ($taskReader.ReadUInt32() -ne 0x00004550 -or $taskReader.ReadUInt16() -ne 0x8664) {
        throw 'Release executable must be a Windows x64 PE image'
    }
} finally {
    $taskReader.Dispose()
}

$taskSdkBin = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits/10/bin'
$taskMt = Get-ChildItem -Path (Join-Path $taskSdkBin '*/x64/mt.exe') -File |
    Sort-Object { [version]$_.Directory.Parent.Name } -Descending | Select-Object -First 1
if (!$taskMt) { throw 'Windows SDK mt.exe is required to verify the administrator manifest' }
New-Item -ItemType Directory -Path $taskOutput -Force | Out-Null
$taskStaging = Join-Path $taskOutput 'package'
New-Item -ItemType Directory -Path $taskStaging | Out-Null
$taskManifestPath = Join-Path $taskOutput 'administrator.manifest.xml'
& $taskMt.FullName '-nologo' ("-inputresource:{0};#1" -f $taskExecutable) ("-out:{0}" -f $taskManifestPath)
if ($LASTEXITCODE -ne 0) { throw 'Cannot extract embedded administrator manifest' }
[xml]$taskManifest = Get-Content -LiteralPath $taskManifestPath -Raw
$taskLevel = $taskManifest.SelectSingleNode("//*[local-name()='requestedExecutionLevel']")
if (!$taskLevel -or $taskLevel.level -ne 'requireAdministrator' -or $taskLevel.uiAccess -ne 'false') {
    throw 'Release must retain requireAdministrator and uiAccess=false'
}

$taskExeName = 'endfield-direct-launch.exe'
$taskReleaseExe = Join-Path $taskOutput $taskExeName
Copy-Item -LiteralPath $taskExecutable -Destination $taskReleaseExe
Copy-Item -LiteralPath $taskReleaseExe -Destination (Join-Path $taskStaging $taskExeName)
Copy-Item -LiteralPath (Join-Path $taskRoot 'docs/RELEASE-README.md') -Destination (Join-Path $taskStaging 'README.md')
$taskRustVersion = rustc +stable --version
if ($LASTEXITCODE -ne 0) { throw 'Cannot read Rust compiler version' }
$taskEncoding = [Text.UTF8Encoding]::new($false)
$taskBuildInfo = [ordered]@{
    name = 'endfield-direct-launch'
    version = $taskVersion
    target = 'x86_64-pc-windows-msvc'
    commit = $CommitSha
    build_id = $BuildId
    built_at_utc = [DateTimeOffset]::UtcNow.ToString('o')
    rustc = $taskRustVersion
    execution_level = 'requireAdministrator'
    exe_sha256 = (Get-FileHash -LiteralPath $taskReleaseExe -Algorithm SHA256).Hash.ToLowerInvariant()
}
$taskBuildInfoPath = Join-Path $taskOutput 'build-info.json'
[IO.File]::WriteAllText($taskBuildInfoPath, ($taskBuildInfo | ConvertTo-Json) + "`n", $taskEncoding)
Copy-Item -LiteralPath $taskBuildInfoPath -Destination (Join-Path $taskStaging 'build-info.json')
$taskZipName = "endfield-direct-launch-v$taskVersion-windows-x86_64.zip"
Compress-Archive -Path (Join-Path $taskStaging '*') -DestinationPath (Join-Path $taskOutput $taskZipName) -CompressionLevel Optimal
$taskChecksums = foreach ($taskName in @($taskExeName, $taskZipName, 'build-info.json')) {
    $taskHash = (Get-FileHash -LiteralPath (Join-Path $taskOutput $taskName) -Algorithm SHA256).Hash.ToLowerInvariant()
    "$taskHash  $taskName"
}
[IO.File]::WriteAllText((Join-Path $taskOutput 'SHA256SUMS.txt'), ($taskChecksums -join "`n") + "`n", $taskEncoding)
$taskTag = "pre-v$taskVersion-$BuildId"
if ($env:GITHUB_OUTPUT) {
    Add-Content -LiteralPath $env:GITHUB_OUTPUT -Value "version=$taskVersion`ntag=$taskTag" -Encoding utf8
}
Write-Host "Packaged $taskTag in $taskOutput (administrator manifest verified)."
