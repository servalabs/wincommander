function Test-BlocklistConnection {
    $checks = @()
    try {
        $content = Read-HostsContent
        $managed = Get-HostsRecords (Get-HostsManagedSection $content)
        $all = Get-HostsRecords $content
        $owned = @(Get-HostsEssentialOverrides $managed)
        $external = @(Get-HostsEssentialOverrides $all | Where-Object { -not $managed.ContainsKey($_) })
        $detail = 'No listed update, security or connectivity destinations are overridden by WinCom.'
        if ($owned.Count) { $detail = "$($owned.Count) WinCom entries may affect Windows updates, security or connectivity. Update protection or undo the last change." }
        elseif ($external.Count) { $detail = "$($external.Count) other hosts entries override Windows destinations. WinCom will preserve those entries." }
        $checks += @{ name = 'Blocking rules'; state = $(if ($owned.Count -or $external.Count) { 'warning' } else { 'ok' }); detail = $detail; affectedDomains = @($owned + $external) }
    } catch { $checks += @{ name = 'Blocking rules'; state = 'unknown'; detail = $_.Exception.Message } }
    try {
        $lookup = [Net.Dns]::BeginGetHostAddresses('www.microsoft.com', $null, $null)
        try {
            if (-not $lookup.AsyncWaitHandle.WaitOne(3000)) { throw 'DNS lookup timed out.' }
            $addresses = @([Net.Dns]::EndGetHostAddresses($lookup))
            if (-not $addresses.Count -or @($addresses | Where-Object { [Net.IPAddress]::IsLoopback($_) -or $_.ToString() -in @('0.0.0.0', '::') }).Count) {
                throw 'The test destination resolves to a local blocking address.'
            }
        } finally { $lookup.AsyncWaitHandle.Close() }
        $checks += @{ name = 'DNS'; state = 'ok'; detail = 'The Microsoft test destination resolved.' }
    } catch { $checks += @{ name = 'DNS'; state = 'warning'; detail = 'The test name could not be resolved normally. DNS settings, filtering or the network may be involved.' } }
    $response = $null
    try {
        $request = [Net.HttpWebRequest]::Create('https://www.microsoft.com/')
        $request.Method = 'HEAD'; $request.Timeout = 4000; $request.ReadWriteTimeout = 4000
        $request.AllowAutoRedirect = $false; $request.UseDefaultCredentials = $false
        $response = $request.GetResponse()
        $checks += @{ name = 'HTTPS'; state = 'ok'; detail = 'The Microsoft test destination answered over HTTPS.' }
    } catch {
        $failure = $_.Exception
        while ($failure.InnerException) { $failure = $failure.InnerException }
        $response = $failure.Response
        if ($response) {
            $checks += @{ name = 'HTTPS'; state = 'ok'; detail = 'The test server answered over HTTPS, although it declined the test request.' }
        } else {
            $checks += @{ name = 'HTTPS'; state = 'warning'; detail = 'The HTTPS check did not complete. This alone does not identify WinCom as the cause.' }
        }
    } finally {
        if ($response) { $response.Close() }
        if ($request) { $request.Abort() }
    }
    try {
        $profiles = @(Get-NetConnectionProfile -ErrorAction Stop)
        $online = @($profiles | Where-Object { $_.IPv4Connectivity -eq 'Internet' -or $_.IPv6Connectivity -eq 'Internet' }).Count -gt 0
        $checks += @{ name = 'Windows connectivity'; state = $(if ($online) { 'ok' } else { 'warning' }); detail = $(if ($online) { 'Windows reports internet access.' } else { 'Windows does not currently report internet access. A captive portal, VPN or connectivity probe may be involved.' }) }
    } catch { $checks += @{ name = 'Windows connectivity'; state = 'unknown'; detail = 'Windows connectivity status could not be read.' } }
    return @{ checkedAt = [DateTime]::UtcNow.ToString('o'); checks = @($checks) }
}
