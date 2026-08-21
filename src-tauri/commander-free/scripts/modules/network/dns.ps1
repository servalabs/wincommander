# ============================================================================
# NETWORK - DNS SECURITY MODULE
# Secure DNS (DoH) and provider management
# ============================================================================
#
# KNOWLEDGE TRANSFER — Windows 11 DoH registry layout (verified June 2026
# against a real "Encrypted only (DNS over HTTPS)" config made via the Windows
# Settings UI, then dumped key-by-key with exact value kinds).
#
# There are TWO independent stores. Confusing them cost us several iterations:
#
#  1. PER-ADAPTER  — the ONLY thing the Settings "Encrypted" indicator reads,
#     and what actually makes a specific adapter use DoH:
#       HKLM\SYSTEM\CurrentControlSet\Services\Dnscache\
#         InterfaceSpecificParameters\{GUID}\DohInterfaceSettings\
#           Doh\{IPv4}     DohTemplate = <URL> (REG_SZ)   DohFlags = 2 (REG_QWORD)
#           Doh6\{IPv6}    DohTemplate = <URL> (REG_SZ)   DohFlags = 2 (REG_QWORD)
#     DohFlags (per-interface): 1 = encrypted preferred w/ UDP fallback,
#                               2 = encrypted ONLY, no fallback.
#     CRITICAL: DohFlags MUST be REG_QWORD. A DWORD is silently ignored and
#     Settings keeps showing "Unencrypted". The Doh\{IP} key is NOT an empty
#     pointer — the template string lives right here on it.
#
#  2. GLOBAL "well-known servers" — used by the DNS client + netsh, NOT read by
#     Settings for the per-adapter badge. The two can even hold different URLs.
#       HKLM\...\Dnscache\Parameters\DohWellKnownServers\{IP}
#           Template = <URL> (REG_SZ)   Flags = 8 (REG_QWORD)  (autoupgrade,no-udp)
#     `netsh dns add encryption` writes this AND signals the running Dnscache
#     service to reload without a restart (Dnscache is protected / unstoppable),
#     so we use it as our live-reload trigger.
#
# Bottom line: to make Settings show "Encrypted" you MUST write store #1.
# Store #2 alone is not enough.
# ============================================================================

function Get-DNSStatus {
    # Only read DNS from physical adapters (InterfaceType 6=Ethernet, 71=Wi-Fi).
    # Tailscale (type 53) and other tunnels manage their own resolvers and
    # would return misleading data (e.g. MagicDNS IPs instead of the user's
    # configured provider).
    $physicalInterfaceTypes = @(6, 71)
    $allUp = Get-NetAdapter | Where-Object { $_.Status -eq 'Up' -and $physicalInterfaceTypes -contains $_.InterfaceType }

    # Within physical adapters, prefer Ethernet then Wi-Fi.
    $adapter = $allUp | Where-Object { $_.InterfaceType -eq 6 } | Select-Object -First 1
    if (-not $adapter) {
        $adapter = $allUp | Select-Object -First 1
    }

    if (-not $adapter) {
        return @{
            servers     = @()
            adapter     = $null
            provider    = $null
            resolverIp  = $null
            resolverOrg = $null
            dohTemplate = $null
        }
    }

    $dns = Get-DnsClientServerAddress -InterfaceIndex $adapter.ifIndex -AddressFamily IPv4
    $servers = $dns.ServerAddresses

    # Probe the actual resolver in use (works through Tailscale/VPN DNS overrides).
    # whoami.cloudflare.com returns multiple TXT records; the "remote_ip:" entry is
    # the IP of the recursive resolver that queried Cloudflare's authoritative server.
    $resolverIp  = $null
    $resolverOrg = $null
    try {
        $txt = Resolve-DnsName -Name "whoami.cloudflare.com" -Type TXT -ErrorAction SilentlyContinue
        $remoteEntry = $txt | Where-Object { $_.Strings -like "remote_ip:*" } | Select-Object -First 1
        if ($remoteEntry) {
            $resolverIp = ($remoteEntry.Strings[0] -replace '^remote_ip:\s*', '').Trim()
        }
    } catch {}

    # PTR lookup with a hard 2-second timeout so a slow reverse-DNS zone never
    # blocks the whole status call. Uses .NET async instead of Resolve-DnsName.
    if ($resolverIp) {
        try {
            $task = [System.Net.Dns]::GetHostEntryAsync($resolverIp)
            if ($task.Wait(2000) -and $task.Result.HostName -ne $resolverIp) {
                $parts = $task.Result.HostName.Split('.')
                if ($parts.Length -ge 2) {
                    $resolverOrg = (Get-Culture).TextInfo.ToTitleCase($parts[$parts.Length - 2].ToLower())
                }
            }
        } catch {}
    }

    # AdGuard public DNS IPs (Default, Family, Non-filtering variants)
    $adguardIPs = @(
        "94.140.14.14", "94.140.15.15",
        "94.140.14.15", "94.140.15.16",
        "94.140.14.140", "94.140.15.140",
        "94.140.14.149", "94.140.15.59"
    )
    $cloudflareFamilyIPs = @("1.1.1.3", "1.0.0.3")
    $cloudflareIPs       = @("1.1.1.1", "1.0.0.1", "1.1.1.2", "1.0.0.2")
    # ControlD FreeDNS — same IPs regardless of category filter slug.
    $controldIPs         = @("76.76.2.11", "76.76.10.11")

    $provider = "Custom"

    # Primary: classify by resolved upstream IP (sees through Tailscale/VPN)
    if ($resolverIp) {
        if ($adguardIPs -contains $resolverIp -or $resolverIp -match '^94\.140\.(14|15)\.') {
            $provider = "AdGuard_Ads_Trackers"
        } elseif ($cloudflareFamilyIPs -contains $resolverIp) {
            $provider = "Cloudflare_Malware_Adult"
        } elseif ($cloudflareIPs -contains $resolverIp) {
            $provider = "Cloudflare"
        } elseif ($controldIPs -contains $resolverIp -or $resolverIp -match '^76\.76\.(2|10)\.') {
            $provider = "ControlD"
        }
    }

    # Fallback: classify by adapter-configured IP (catches direct configs the probe may miss)
    if ($provider -eq "Custom") {
        foreach ($ip in $servers) {
            if ($adguardIPs -contains $ip) { $provider = "AdGuard_Ads_Trackers"; break }
            if ($cloudflareFamilyIPs -contains $ip) { $provider = "Cloudflare_Malware_Adult"; break }
            if ($controldIPs -contains $ip) { $provider = "ControlD"; break }
        }
    }

    @{
        servers     = $servers
        adapter     = $adapter.Name
        provider    = $provider
        resolverIp  = $resolverIp
        resolverOrg = $resolverOrg
        dohTemplate = $null
    }
}

function Set-SecureDNS {
    param(
        [string]$Provider = "cloudflare",
        [string]$DohId,
        [string]$DeviceName,
        [string]$Primary,
        [string]$Secondary,
        [string]$Primary6,
        [string]$Secondary6,
        [string]$FilterSlug,
        [switch]$Silent
    )

    Assert-IsAdmin

    $config = Get-ProviderParams -Provider $Provider -DohId $DohId -DeviceName $DeviceName -Primary $Primary -Secondary $Secondary -Primary6 $Primary6 -Secondary6 $Secondary6 -FilterSlug $FilterSlug

    if ($config.Servers4.Count -eq 0) {
        throw "No IPv4 DNS servers configured for provider '$Provider'."
    }

    # Only physical adapters (Ethernet=6, Wi-Fi=71). Excludes Tailscale
    # (InterfaceType 53), Hyper-V, WireGuard, and other tunnel adapters
    # so we never stomp on MagicDNS or VPN-managed resolvers.
    $physicalTypes = @(6, 71)
    $adapters = Get-NetAdapter | Where-Object { $_.Status -eq 'Up' -and $physicalTypes -contains $_.InterfaceType }
    if ($adapters.Count -eq 0) {
        throw "No active physical network adapters found (Ethernet or Wi-Fi)."
    }

    $dncBase = "HKLM:\SYSTEM\CurrentControlSet\Services\Dnscache\InterfaceSpecificParameters"

    foreach ($adapter in $adapters) {
        # Set DNS server IPs on adapter
        Set-DnsClientServerAddress -InterfaceIndex $adapter.ifIndex -ServerAddresses $config.Servers4 -ErrorAction Stop

        if ($config.Servers6.Count -gt 0) {
            $adapterName = $adapter.Name
            netsh interface ipv6 set dns "$adapterName" static ::1 validate=no 2>$null | Out-Null
            netsh interface ipv6 delete dns "$adapterName" ::1 2>$null | Out-Null
            $idx = 0
            foreach ($s6 in $config.Servers6) {
                if ($idx -eq 0) { netsh interface ipv6 set dns "$adapterName" static $s6 validate=no 2>$null | Out-Null }
                else             { netsh interface ipv6 add dns "$adapterName" $s6 index=2 validate=no 2>$null | Out-Null }
                $idx++
            }
        }

        # ----------------------------------------------------------------------
        # PER-ADAPTER DoH (store #1 — see module header). This is what makes
        # Windows Settings show "Encrypted" and what binds DoH to THIS adapter.
        # Replicates exactly what the Settings UI writes for "Encrypted only":
        #   Doh\{IPv4} / Doh6\{IPv6}:
        #       DohTemplate = <URL> (REG_SZ)
        #       DohFlags    = 2     (REG_QWORD)   <-- QWORD is mandatory
        # ----------------------------------------------------------------------
        if ($config.DohTemplate) {
            $guid = $adapter.InterfaceGuid
            $doh4 = "$dncBase\$guid\DohInterfaceSettings\Doh"
            $doh6 = "$dncBase\$guid\DohInterfaceSettings\Doh6"

            # Erase stale entries first so a changed provider can't leave old IPs active.
            foreach ($p in @($doh4, $doh6)) {
                if (Test-Path $p) { Get-ChildItem $p -ErrorAction SilentlyContinue | Remove-Item -Recurse -Force }
            }

            foreach ($ip in $config.Servers4) {
                $k = "$doh4\$ip"
                New-Item -Path $k -Force | Out-Null
                Set-ItemProperty -Path $k -Name DohTemplate -Value $config.DohTemplate -Type String
                # DohFlags MUST be QWORD (a DWORD here is ignored -> "Unencrypted").
                New-ItemProperty -Path $k -Name DohFlags -Value 2 -PropertyType QWord -Force | Out-Null
            }
            foreach ($ip in $config.Servers6) {
                $k = "$doh6\$ip"
                New-Item -Path $k -Force | Out-Null
                Set-ItemProperty -Path $k -Name DohTemplate -Value $config.DohTemplate -Type String
                New-ItemProperty -Path $k -Name DohFlags -Value 2 -PropertyType QWord -Force | Out-Null
            }
        }
    }

    # --------------------------------------------------------------------------
    # GLOBAL well-known servers (store #2 — see module header). NOT read by the
    # Settings indicator; this is for the DNS client itself. `netsh dns add
    # encryption` writes Template+Flags=8 here AND triggers a live Dnscache
    # reload without a service restart (Dnscache is protected). We let netsh own
    # this key entirely (it writes Template/Flags) — we just clear stale entries
    # first and use the call as the reload trigger so the per-adapter config
    # above takes effect immediately.
    # --------------------------------------------------------------------------
    if ($config.DohTemplate) {
        $wksBase = "HKLM:\SYSTEM\CurrentControlSet\Services\Dnscache\Parameters\DohWellKnownServers"
        foreach ($ip in ($config.Servers4 + $config.Servers6)) {
            $k = "$wksBase\$ip"
            if (Test-Path $k) { Remove-Item $k -Recurse -Force -ErrorAction SilentlyContinue }
            netsh dns add encryption server=$ip dohtemplate=$($config.DohTemplate) autoupgrade=yes udpfallback=no 2>$null | Out-Null
        }
    }

    Clear-DnsClientCache -ErrorAction SilentlyContinue

    @{
        status    = 'set'
        encrypted = ($null -ne $config.DohTemplate)
        provider  = $config.ProviderId
        servers   = $config.Servers4
        doh       = $config.DohTemplate
        message   = "DNS encrypted and applied automatically."
    }
}

function Clear-SecureDNS {
    Assert-IsAdmin
    $physicalTypes = @(6, 71)
    $adapters = Get-NetAdapter | Where-Object { $_.Status -eq 'Up' -and $physicalTypes -contains $_.InterfaceType }
    $dncBase  = "HKLM:\SYSTEM\CurrentControlSet\Services\Dnscache\InterfaceSpecificParameters"

    foreach ($adapter in $adapters) {
        Set-DnsClientServerAddress -InterfaceIndex $adapter.ifIndex -ResetServerAddresses
        # Erase per-adapter DoH registry so Windows stops enforcing encryption
        # on this adapter. Without this the DNS client keeps trying DoH even
        # after the server IPs are reset to DHCP.
        # Remove the per-adapter Doh\{IP} / Doh6\{IP} subkeys (store #1 in the
        # module header). Deleting each {IP} subkey also drops its DohTemplate +
        # DohFlags values, so Settings flips back to "Unencrypted".
        $guid = $adapter.InterfaceGuid
        foreach ($sub in @("DohInterfaceSettings\Doh", "DohInterfaceSettings\Doh6")) {
            $p = "$dncBase\$guid\$sub"
            if (Test-Path $p) { Get-ChildItem $p -ErrorAction SilentlyContinue | Remove-Item -Recurse -Force }
        }
    }

    # Erase DohWellKnownServers entries for all known provider IPs so Windows
    # Settings stops showing "Encrypted" and the DNS client stops using DoH.
    $wksBase = "HKLM:\SYSTEM\CurrentControlSet\Services\Dnscache\Parameters\DohWellKnownServers"
    $knownProviderIPs = @(
        "94.140.14.14","94.140.15.15",
        "1.1.1.3","1.0.0.3",
        "76.76.2.11","76.76.10.11",
        "2a10:50c0::ad1:ff","2a10:50c0::ad2:ff",
        "2606:4700:4700::1113","2606:4700:4700::1003",
        "2606:1a40::11"
    )
    foreach ($ip in $knownProviderIPs) {
        $k = "$wksBase\$ip"
        if (Test-Path $k) { Remove-Item $k -Recurse -Force -ErrorAction SilentlyContinue }
        netsh dns delete encryption server=$ip 2>$null | Out-Null
    }

    Clear-DnsClientCache -ErrorAction SilentlyContinue
    @{ status = 'cleared' }
}

function Get-ProviderParams {
    param($Provider, $DohId, $DeviceName, $Primary, $Secondary, $Primary6, $Secondary6, $FilterSlug)

    $type = "custom"
    if ($Provider -match '(?i)adguard') { $type = "adguard" }
    elseif ($Provider -match '(?i)cloudflare') { $type = "cloudflare" }
    elseif ($Provider -match '(?i)swiss') { $type = "nextdns" }
    elseif ($Provider -match '(?i)controld') { $type = "controld" }

    $providerId = switch ($type) {
        "adguard"    { "AdGuard_Ads_Trackers" }
        "cloudflare" { "Cloudflare_Malware_Adult" }
        "nextdns"    { "Swiss_Firewall" }
        "controld"   { "ControlD" }
        default      { "Custom" }
    }

    $config = @{ Servers4 = @(); Servers6 = @(); DohTemplate = $null; ProviderName = $type; ProviderId = $providerId }

    switch ($type) {
        "adguard" {
            $config.Servers4 = @("94.140.14.14", "94.140.15.15")
            $config.Servers6 = @("2a10:50c0::ad1:ff", "2a10:50c0::ad2:ff")
            $config.DohTemplate = "https://dns.adguard-dns.com/dns-query"
        }
        "cloudflare" {
            $config.Servers4 = @("1.1.1.3", "1.0.0.3")
            $config.Servers6 = @("2606:4700:4700::1113", "2606:4700:4700::1003")
            $config.DohTemplate = "https://family.cloudflare-dns.com/dns-query"
        }
        "nextdns" {
            $config.Servers4 = @($Primary, $Secondary) | Where-Object { $_ }
            $config.Servers6 = @($Primary6, $Secondary6) | Where-Object { $_ }
            if ($DohId -and $DeviceName) {
                $config.DohTemplate = "https://dns.nextdns.io/$DohId/$([uri]::EscapeDataString($DeviceName))"
            }
        }
        "controld" {
            # ControlD FreeDNS — IPs are constant across all category combinations.
            # The category slug (e.g. "no-ads-porn-dating") becomes the DoH hostname prefix:
            #   https://<slug>.freedns.controld.com/dns-query
            # An empty slug falls back to the default unfiltered ControlD resolver.
            $config.Servers4 = @("76.76.2.11")
            $config.Servers6 = @("2606:1a40::11")
            if ($FilterSlug) {
                # Lower-case + strip anything outside [a-z0-9-] to avoid an attacker-shaped
                # slug producing a malformed URL.
                $safeSlug = ($FilterSlug -as [string]).ToLower() -replace '[^a-z0-9-]', ''
                if ($safeSlug) {
                    $config.DohTemplate = "https://$safeSlug.freedns.controld.com/dns-query"
                }
            }
            if (-not $config.DohTemplate) {
                $config.DohTemplate = "https://freedns.controld.com/dns-query"
            }
        }
        Default {
            $config.Servers4 = @($Primary, $Secondary) | Where-Object { $_ }
            $config.Servers6 = @($Primary6, $Secondary6) | Where-Object { $_ }
        }
    }
    return $config
}

function Enable-IPv4Preference {
    Assert-IsAdmin
    # 0x20 decimal is 32. It tells Windows to prefer IPv4 over IPv6.
    $path = "HKLM:\SYSTEM\CurrentControlSet\Services\Tcpip6\Parameters"
    if (!(Test-Path $path)) { New-Item -Path $path -Force | Out-Null }
    Set-ItemProperty -Path $path -Name "DisabledComponents" -Value 32 -Type DWord -Force
    @{ status = 'enabled' }
}

function Disable-IPv4Preference {
    Assert-IsAdmin
    $path = "HKLM:\SYSTEM\CurrentControlSet\Services\Tcpip6\Parameters"
    if (Test-Path $path) {
        Invoke-7Erase -Path $path -Type RegistryProperty -Name "DisabledComponents"
    }
    @{ status = 'disabled' }
}

# ============================================================================
# DNS CENSORSHIP PROTECTION
# ============================================================================
# Forces ALL name resolution through the encrypted DoH resolver by blocking
# outbound plaintext DNS (UDP + TCP port 53) at the Windows firewall. With
# Encrypted DNS on (DohFlags=2, encrypted-only), the system resolver talks DoH
# over 443, so blocking 53 stops the ISP/network from transparently
# intercepting, redirecting, or censoring lookups — and stops apps that do
# their own plaintext DNS from leaking around DoH.
#
# Precondition (enforced by the UI, which only enables this toggle when
# Encrypted DNS is on): without an encrypted resolver, blocking 53 would break
# all DNS. Both directions Assert-IsAdmin and are fully reversible. The rules
# carry the "WinCommander-" prefix so the self-destruct cleanup removes them.
# ============================================================================

function Enable-DNSCensorshipProtection {
    Assert-IsAdmin
    foreach ($proto in @('UDP', 'TCP')) {
        $name = "WinCommander-DNSCensorshipProtection-$proto"
        Get-NetFirewallRule -DisplayName $name -ErrorAction SilentlyContinue | Remove-NetFirewallRule -ErrorAction SilentlyContinue
        New-NetFirewallRule -DisplayName $name -Direction Outbound -Action Block `
            -Protocol $proto -RemotePort 53 -Profile Any -ErrorAction Stop | Out-Null
    }
    @{ status = 'enabled' }
}

function Disable-DNSCensorshipProtection {
    Assert-IsAdmin
    foreach ($proto in @('UDP', 'TCP')) {
        $name = "WinCommander-DNSCensorshipProtection-$proto"
        Get-NetFirewallRule -DisplayName $name -ErrorAction SilentlyContinue | Remove-NetFirewallRule -ErrorAction SilentlyContinue
    }
    @{ status = 'disabled' }
}
