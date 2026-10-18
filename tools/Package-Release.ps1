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
    exe_md5 = (Get-FileHash -LiteralPath $taskReleaseExe -Algorithm MD5).Hash.ToLowerInvariant()
}
$taskBuildInfoPath = Join-Path $taskOutput 'build-info.json'
[IO.File]::WriteAllText($taskBuildInfoPath, ($taskBuildInfo | ConvertTo-Json) + "`n", $taskEncoding)
Copy-Item -LiteralPath $taskBuildInfoPath -Destination (Join-Path $taskStaging 'build-info.json')
$taskZipName = "endfield-direct-launch-v$taskVersion-windows-x86_64.zip"
Compress-Archive -Path (Join-Path $taskStaging '*') -DestinationPath (Join-Path $taskOutput $taskZipName) -CompressionLevel Optimal
$taskPayloadNames = @($taskExeName, $taskZipName, 'build-info.json')
foreach ($taskAlgorithm in @('SHA256', 'MD5')) {
    $taskChecksums = foreach ($taskName in $taskPayloadNames) {
        $taskHash = (Get-FileHash -LiteralPath (Join-Path $taskOutput $taskName) -Algorithm $taskAlgorithm).Hash.ToLowerInvariant()
        "$taskHash  $taskName"
    }
    [IO.File]::WriteAllText((Join-Path $taskOutput "${taskAlgorithm}SUMS.txt"), ($taskChecksums -join "`n") + "`n", $taskEncoding)
}
$taskTag = "pre-v$taskVersion-$BuildId"
$taskDescriptions = [ordered]@{
    $taskExeName = '独立启动器程序（Windows x64），需要管理员权限；适合直接下载运行。'
    $taskZipName = '便携压缩包，包含同一启动器 EXE、使用说明和构建信息；解压后运行 EXE。'
    'build-info.json' = '构建信息：源码提交、编译器、目标平台、管理员权限声明，以及 EXE 的 SHA-256 和 MD5。'
    'SHA256SUMS.txt' = 'EXE、便携 ZIP 和构建信息的 SHA-256 校验清单。'
    'MD5SUMS.txt' = 'EXE、便携 ZIP 和构建信息的 MD5 校验清单。'
}
$taskRepoUrl = if ($env:GITHUB_SERVER_URL -and $env:GITHUB_REPOSITORY) {
    "$env:GITHUB_SERVER_URL/$env:GITHUB_REPOSITORY"
} else {
    'https://github.com/NatsumiXD/zmd-rust-launcher'
}
$taskRunId = ($BuildId -split '\.')[0]
$taskNotes = @(
    "## 终末地 Rust 启动器 $taskVersion 预发布版"
    ''
    'Windows x64 自动构建，需要管理员权限。可直接下载 EXE，或下载便携 ZIP 并解压后运行。'
    '不包含官方 Games.exe 或游戏文件；启动器会按需检查、复用或获取官方 Games.exe。'
    '使用 --game 指定终末地主程序位置，命令终端输入 help 查看 start、restart、lud、cn 等指令。每次启动游戏后自动检查 Rust 更新，lud 可手动检查；先测速官方与镜像，再下载、校验并暂存，关闭启动器后才应用，游戏继续运行。'
    ''
    ('源码提交：[`{0}`]({1}/commit/{0})' -f $CommitSha, $taskRepoUrl)
    ('构建记录：[GitHub Actions]({0}/actions/runs/{1})' -f $taskRepoUrl, $taskRunId)
    ''
    '### 附件中文说明'
    ''
    '| 文件 | 说明 |'
    '| --- | --- |'
)
foreach ($taskEntry in $taskDescriptions.GetEnumerator()) {
    $taskAssetUrl = "$taskRepoUrl/releases/download/$taskTag/$([Uri]::EscapeDataString($taskEntry.Key))"
    $taskNotes += ('| [{0}]({1}) | {2} |' -f $taskEntry.Key, $taskAssetUrl, $taskEntry.Value)
}
foreach ($taskAlgorithm in @('SHA256', 'MD5')) {
    $taskHeading = if ($taskAlgorithm -eq 'SHA256') { 'SHA-256' } else { 'MD5' }
    $taskNotes += @('', "### $taskHeading", '', '```text')
    foreach ($taskName in $taskDescriptions.Keys) {
        $taskHash = (Get-FileHash -LiteralPath (Join-Path $taskOutput $taskName) -Algorithm $taskAlgorithm).Hash.ToLowerInvariant()
        $taskNotes += "$taskHash  $taskName"
    }
    $taskNotes += '```'
}
[IO.File]::WriteAllText((Join-Path $taskOutput 'release-notes.md'), ($taskNotes -join "`n") + "`n", $taskEncoding)
if ($env:GITHUB_OUTPUT) {
    Add-Content -LiteralPath $env:GITHUB_OUTPUT -Value "version=$taskVersion`ntag=$taskTag" -Encoding utf8
}
Write-Host "Packaged $taskTag in $taskOutput (administrator manifest verified)."
