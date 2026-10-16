param([Parameter(Mandatory=$true)][string]$Path, [Parameter(Mandatory=$true)][string[]]$Texts)
$taskBytes = [IO.File]::ReadAllBytes($Path)
$taskPE = [BitConverter]::ToInt32($taskBytes, 0x3c)
$taskSectionCount = [BitConverter]::ToUInt16($taskBytes, $taskPE + 6)
$taskOptionalSize = [BitConverter]::ToUInt16($taskBytes, $taskPE + 20)
$taskOptional = $taskPE + 24
if ([BitConverter]::ToUInt16($taskBytes, $taskOptional) -ne 0x20b) { throw 'Expected PE32+' }
$taskBase = [BitConverter]::ToUInt64($taskBytes, $taskOptional + 24)
$taskSections = for ($taskIndex=0; $taskIndex -lt $taskSectionCount; $taskIndex++) {
  $taskOffset = $taskOptional + $taskOptionalSize + 40*$taskIndex
  [pscustomobject]@{RVA=[BitConverter]::ToUInt32($taskBytes,$taskOffset+12); RawSize=[BitConverter]::ToUInt32($taskBytes,$taskOffset+16); Raw=[BitConverter]::ToUInt32($taskBytes,$taskOffset+20)}
}
$taskASCII = [Text.Encoding]::ASCII.GetString($taskBytes)
$taskResults = foreach ($taskText in $Texts) {
  $taskFrom = 0
  while (($taskFound=$taskASCII.IndexOf($taskText,$taskFrom,[StringComparison]::Ordinal)) -ge 0) {
    $taskSection = $taskSections | Where-Object { $taskFound -ge $_.Raw -and $taskFound -lt $_.Raw+$_.RawSize } | Select-Object -First 1
    $taskVA = if ($null -ne $taskSection) { '0x{0:x}' -f ($taskBase+$taskSection.RVA+$taskFound-$taskSection.Raw) } else { $null }
    [pscustomobject]@{Text=$taskText; FileOffset=('0x{0:x}' -f $taskFound); Address=$taskVA}
    $taskFrom=$taskFound+$taskText.Length
  }
}
ConvertTo-Json -InputObject @($taskResults) -Depth 3
