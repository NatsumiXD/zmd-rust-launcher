$ErrorActionPreference = 'Stop'
$taskReportDir = 'D:\Project\zmd\analysis'
$taskLauncher = 'D:\Project\zmd\portable\endfield-direct-launch.exe'
$taskParent = 'D:\Project\zmd\portable\Games.exe'
$taskStartedAt = [DateTimeOffset]::Now.ToString('o')
try {
    & $taskLauncher --parent $taskParent --launch 2>&1 | Out-File -LiteralPath "$taskReportDir\portable-launch-output.txt" -Encoding utf8
    $taskExitCode = $LASTEXITCODE
    [pscustomobject]@{
        StartedAt = $taskStartedAt
        CompletedAt = [DateTimeOffset]::Now.ToString('o')
        Elevated = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
        Parent = $taskParent
        ToolExitCode = $taskExitCode
    } | ConvertTo-Json | Set-Content -LiteralPath "$taskReportDir\portable-launch-result.json" -Encoding utf8
} catch {
    $_.Exception.Message | Set-Content -LiteralPath "$taskReportDir\portable-launch-error.txt" -Encoding utf8
    exit 1
}
