param(
    [string]$GamePath = 'D:\yj\Hypergryph Launcher\games\Arknights Endfield\Endfield.exe'
)

$ErrorActionPreference = 'Stop'
$taskTool = Join-Path $PSScriptRoot 'endfield-direct-launch.exe'
& $taskTool --game $GamePath --launch
exit $LASTEXITCODE
