$ErrorActionPreference = 'Stop'
function Hide-UrlQueries($taskValue) {
    if ($null -eq $taskValue) { return $null }
    if ($taskValue -is [string]) {
        if ($taskValue -match '^https?://') {
            $taskUri = [Uri]$taskValue
            if ($taskUri.Query) {
                return [ordered]@{ url_without_query = $taskUri.GetLeftPart([UriPartial]::Path); query_keys = @($taskUri.Query.TrimStart('?').Split('&') | ForEach-Object { $_.Split('=')[0] }) }
            }
        }
        return $taskValue
    }
    if ($taskValue -is [System.Collections.IDictionary]) {
        $taskResult = [ordered]@{}
        foreach ($taskKey in $taskValue.Keys) { $taskResult[$taskKey] = Hide-UrlQueries $taskValue[$taskKey] }
        return $taskResult
    }
    if ($taskValue -is [System.Collections.IEnumerable]) {
        return ,@($taskValue | ForEach-Object { Hide-UrlQueries $_ })
    }
    return $taskValue
}
$taskRecords = @()
foreach ($taskVersion in @('1.6.3', '0.0.0', '1.6.2', '1.6.0')) {
    $taskUrl = 'https://launcher.hypergryph.com/api/game/get_latest?appcode=6LL0KJuqHBVz33WK&channel=1&sub_channel=1&version=' + $taskVersion + '&launcher_appcode=abYeZZ16BPluCFyT&launcher_sub_channel=1&disk_type=1&patch_encrypt=true'
    try {
        $taskResponse = Invoke-WebRequest -Uri $taskUrl -Method Get -TimeoutSec 30 -MaximumRedirection 0
        if ($taskResponse.RawContentLength -gt 2097152) { throw 'Metadata exceeded 2 MiB bound' }
        $taskJson = $taskResponse.Content | ConvertFrom-Json -AsHashtable
        $taskRecords += [ordered]@{ observed_at = [DateTimeOffset]::Now.ToString('o'); method = 'GET'; request_url = $taskUrl; status = [int]$taskResponse.StatusCode; body = (Hide-UrlQueries $taskJson) }
    } catch {
        $taskRecords += [ordered]@{ observed_at = [DateTimeOffset]::Now.ToString('o'); method = 'GET'; request_url = $taskUrl; error_type = $_.Exception.GetType().FullName; error = 'Request or JSON parsing failed; raw response and signed URLs not retained' }
    }
}
$taskOut = [ordered]@{ kind = 'read-only-official-game-metadata-probe'; downloaded_game_packages = $false; account_credentials_used = $false; responses = $taskRecords }
$taskOut | ConvertTo-Json -Depth 40 | Set-Content -LiteralPath 'D:\Project\zmd\analysis\game-update-live-metadata.json' -Encoding utf8
$taskRecords | ForEach-Object {
    $taskSum = [long]0
    foreach ($taskPack in $_.body.pkg.packs) { $taskSum += [long]$taskPack.package_size }
    [ordered]@{ request_version = $_.body.request_version; version = $_.body.version; action = $_.body.action; pack_count = $_.body.pkg.packs.Count; package_sum = $taskSum; total_size = $_.body.pkg.total_size; patch = $_.body.patch; pre_patch = $_.body.pre_patch }
} | ConvertTo-Json -Depth 25
