$ErrorActionPreference = 'Stop'
$taskReportDir = 'D:\Project\zmd\analysis'
$taskLauncher = 'D:\Project\zmd\target\release\endfield-direct-launch.exe'
$taskStartedAt = [DateTimeOffset]::Now.ToString('o')
try {
    & $taskLauncher --launch --wait 2>&1 | Out-File -LiteralPath "$taskReportDir\launch-2-output.txt" -Encoding utf8
    $taskExitCode = $LASTEXITCODE
    [pscustomobject]@{
        StartedAt = $taskStartedAt
        CompletedAt = [DateTimeOffset]::Now.ToString('o')
        Elevated = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
        LauncherExitCode = $taskExitCode
    } | ConvertTo-Json | Set-Content -LiteralPath "$taskReportDir\launch-2-result.json" -Encoding utf8
} catch {
    $_.Exception.Message | Set-Content -LiteralPath "$taskReportDir\launch-2-error.txt" -Encoding utf8
    exit 1
}
