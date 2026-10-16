$ErrorActionPreference = 'Stop'
$taskResults = @()
foreach ($taskVersion in @('1.6.0','0.0.0')) {
  $taskQuery = 'appcode=abYeZZ16BPluCFyT&channel=1&sub_channel=1&version=' + [uri]::EscapeDataString($taskVersion) + '&target_app=EndField'
  $taskUri = 'https://launcher.hypergryph.com/api/launcher/get_latest?' + $taskQuery
  try {
    $taskResponse = Invoke-WebRequest -Uri $taskUri -Method Get -TimeoutSec 30 -MaximumRedirection 3
    $taskJSON = $taskResponse.Content | ConvertFrom-Json
    $taskData = $taskJSON
    $taskDownloadUri = if ($taskData.zip_package_url) { [uri]$taskData.zip_package_url } else { $null }
    $taskResults += [pscustomobject]@{
      RequestedAt=(Get-Date).ToString('o'); Method='GET'; Endpoint='https://launcher.hypergryph.com/api/launcher/get_latest'; RequestVersion=$taskVersion
      StatusCode=[int]$taskResponse.StatusCode; ResponseShape='root launcher object'; RootKeys=@($taskJSON.PSObject.Properties.Name)
      DataKeys=@($taskData.PSObject.Properties.Name); Action=$taskData.action; Version=$taskData.version; RequestVersionEcho=$taskData.request_version
      MD5=$taskData.md5; PackageSize=$taskData.package_size; TotalSize=$taskData.total_size
      PackageURLWithoutQuery=$(if($taskDownloadUri){$taskDownloadUri.GetLeftPart([UriPartial]::Path)}else{$null})
      HasSignedQuery=$(if($taskDownloadUri){$taskDownloadUri.Query.Length -gt 0}else{$false})
    }
  } catch {
    $taskResults += [pscustomobject]@{RequestedAt=(Get-Date).ToString('o'); Method='GET'; Endpoint='https://launcher.hypergryph.com/api/launcher/get_latest'; RequestVersion=$taskVersion; ErrorType=$_.Exception.GetType().FullName; StatusCode=$(if($_.Exception.Response){[int]$_.Exception.Response.StatusCode}else{$null})}
  }
}
$taskResults | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $PSScriptRoot 'acq-live-probe.json') -Encoding UTF8
$taskResults | ConvertTo-Json -Depth 5
