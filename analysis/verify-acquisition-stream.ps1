$ErrorActionPreference = 'Stop'
$taskTokens = $null
$taskErrors = $null
$taskAST = [System.Management.Automation.Language.Parser]::ParseFile('D:\Project\zmd\tools\Get-LatestGames.ps1',[ref]$taskTokens,[ref]$taskErrors)
if ($taskErrors.Count) { throw 'Acquisition script syntax error.' }
$taskFunction = $taskAST.Find({param($taskNode) $taskNode -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $taskNode.Name -eq 'Copy-BoundedStream'}, $true)
. ([scriptblock]::Create($taskFunction.Extent.Text))
$taskResults = foreach ($taskCase in @(
    @{Name='Exact declared length'; Actual=8; Expected=8; MustFail=$false},
    @{Name='Truncated transfer'; Actual=7; Expected=8; MustFail=$true},
    @{Name='Excess transfer'; Actual=9; Expected=8; MustFail=$true}
)) {
    $taskInput = [IO.MemoryStream]::new([byte[]](1..$taskCase.Actual))
    $taskOutput = [IO.MemoryStream]::new()
    $taskFailed = $false
    try { Copy-BoundedStream $taskInput $taskOutput $taskCase.Expected }
    catch { $taskFailed = $true }
    if ($taskFailed -ne $taskCase.MustFail) { throw "Unexpected result: $($taskCase.Name)" }
    if ($taskOutput.Length -gt $taskCase.Expected) { throw 'Output exceeded declared size.' }
    if (-not $taskCase.MustFail -and [Convert]::ToBase64String($taskOutput.ToArray()) -ne [Convert]::ToBase64String($taskInput.ToArray())) { throw 'Bytes changed in transfer.' }
    [pscustomobject]@{Case=$taskCase.Name; Passed=$true; Rejected=$taskFailed}
    $taskInput.Dispose()
    $taskOutput.Dispose()
}
$taskResults | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $PSScriptRoot 'acq-stream-verification.json') -Encoding UTF8
$taskResults | ConvertTo-Json
