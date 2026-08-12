# ============================================================================
# NETWORK - FIREWALL MODULE
# Manages Windows Firewall rules and lockdown policies
# ============================================================================

# Get custom backend firewall rules
# KT: Uses -DisplayName server-side filter instead of fetching ALL rules from
# ActiveStore (~2000-5000+ rules) and filtering client-side. The old approach
# could take 10-60 seconds via WFP CIM; this finishes in <1 second.
function Get-FirewallRules {
    $rules = Get-NetFirewallRule -DisplayName 'WINCOMMANDER_*' -ErrorAction SilentlyContinue |
    ForEach-Object {
        $app = Get-NetFirewallApplicationFilter -AssociatedNetFirewallRule $_ -ErrorAction SilentlyContinue
        @{
            DisplayName = $_.DisplayName
            Direction   = $_.Direction
            Action      = $_.Action
            Enabled     = $_.Enabled
            Path        = $app.Program
        }
    }
    
    return @{ rules = @($rules) }
}

# Add a block rule for a specific application
function Add-FirewallBlockRule {
    param(
        [string]$Name,
        [string]$Path
    )
    
    Assert-IsAdmin
    $ruleName = "WINCOMMANDER_$Name"
    
    New-NetFirewallRule -DisplayName $ruleName -Direction Outbound -Program $Path -Action Block -Enabled True
    New-NetFirewallRule -DisplayName "${ruleName}_In" -Direction Inbound -Program $Path -Action Block -Enabled True
    
    return @{ status = 'created'; name = $ruleName }
}

# Toggle a firewall rule
function Set-FirewallRuleEnabled {
    param(
        [string]$Name,
        [bool]$Enabled
    )

    Assert-IsAdmin
    Set-NetFirewallRule -DisplayName $Name -Enabled:($Enabled) -ErrorAction SilentlyContinue
    Set-NetFirewallRule -DisplayName "${Name}_In" -Enabled:($Enabled) -ErrorAction SilentlyContinue

    return @{ status = 'updated'; name = $Name; enabled = $Enabled }
}

# Remove a specific rule
function Remove-FirewallRule {
    param([string]$Name)
    
    Assert-IsAdmin
    Remove-NetFirewallRule -DisplayName $Name -ErrorAction SilentlyContinue
    Remove-NetFirewallRule -DisplayName "${Name}_In" -ErrorAction SilentlyContinue
    
    return @{ status = 'removed'; name = $Name }
}

# Enable restrictive lockdown mode
function Enable-LockdownMode {
    Assert-IsAdmin
    
    # Block all outbound except essential
    netsh advfirewall set allprofiles firewallpolicy blockinbound, blockoutbound
    
    # Allow DNS & HTTPS for basic connectivity
    netsh advfirewall firewall add rule name="WINCOMMANDER_LOCKDOWN_DNS" dir=out action=allow protocol=UDP remoteport=53
    netsh advfirewall firewall add rule name="WINCOMMANDER_LOCKDOWN_HTTPS" dir=out action=allow protocol=TCP remoteport=443
    
    return @{ status = 'enabled'; mode = 'lockdown' }
}

# Disable lockdown mode and return to normal
function Disable-LockdownMode {
    Assert-IsAdmin
    
    netsh advfirewall set allprofiles firewallpolicy blockinbound, allowoutbound
    
    netsh advfirewall firewall delete rule name="WINCOMMANDER_LOCKDOWN_DNS"
    netsh advfirewall firewall delete rule name="WINCOMMANDER_LOCKDOWN_HTTPS"
    
    return @{ status = 'disabled'; mode = 'normal' }
}

# Get current firewall settings and profiles
function Get-FirewallStatus {
    $profiles = Get-NetFirewallProfile | Select-Object Name, Enabled, DefaultOutboundAction
    $lockdownRules = Get-NetFirewallRule -DisplayName 'WINCOMMANDER_LOCKDOWN_*' -ErrorAction SilentlyContinue

    return @{
        lockdown = ($null -ne $lockdownRules -and $lockdownRules.Count -gt 0)
        profiles = $profiles
    }
}

# Block a specific protocol/port
function Block-Protocol {
    param(
        [string]$Name,
        [string[]]$Port,
        [string]$Protocol = "TCP",
        [ValidateSet("Both", "Inbound", "Outbound")]
        [string]$Direction = "Both"
    )
    
    Assert-IsAdmin
    $ruleName = "WINCOMMANDER_BLOCK_${Name}"

    # Deduplication Check
    $existingBlocks = (Get-ProtocolBlocks).blocks
    $isDuplicate = $existingBlocks | Where-Object { 
        ($_.Name -eq $Name) -or 
        ($_.Port -eq ($Port -join ',') -and $_.Protocol -eq $Protocol -and $_.Direction -eq $Direction)
    }

    if ($isDuplicate) {
        return @{ 
            status = 'error'; 
            error  = "A block rule with this name or these settings (Port: $($isDuplicate.Port), Protocol: $($isDuplicate.Protocol)) already exists." 
        }
    }
    
    # Parse Ports (handle comma-separated strings)
    $PortList = @()
    foreach ($p in $Port) {
        if ($p -match ',') {
            $PortList += ($p -split ',' | ForEach-Object { "$_".Trim() })
        }
        else {
            $PortList += "$p".Trim()
        }
    }
    $Port = $PortList
    
    # Remove existing rules with same name to avoid duplicates/conflicts
    Remove-NetFirewallRule -DisplayName "${ruleName}_Out" -ErrorAction SilentlyContinue
    Remove-NetFirewallRule -DisplayName "${ruleName}_In" -ErrorAction SilentlyContinue
    
    if ($Direction -eq "Outbound" -or $Direction -eq "Both") {
        # Outbound MUST filter by REMOTE port, not LocalPort. When this
        # machine connects OUT (e.g. mstsc.exe to an RDP server), the
        # local source port is some ephemeral 49xxx and the remote
        # destination port is 3389. A LocalPort filter on outbound
        # traffic would only match if the local source port equals 3389,
        # which never happens for client connections -- so the rule
        # was a silent no-op. Verified with Test-NetConnection
        # 1.1.1.1:443: LocalPort block -> connection succeeds, RemotePort
        # block -> connection refused.
        New-NetFirewallRule -DisplayName "${ruleName}_Out" -Direction Outbound -Protocol $Protocol -RemotePort $Port -Action Block -Enabled True
    }

    if ($Direction -eq "Inbound" -or $Direction -eq "Both") {
        # Inbound: filter by LOCAL port. When others connect TO this
        # machine they target our LocalPort, so this is correct.
        New-NetFirewallRule -DisplayName "${ruleName}_In" -Direction Inbound -Protocol $Protocol -LocalPort $Port -Action Block -Enabled True
    }
    
    return @{ status = 'created'; name = $ruleName }
}

# Unblock a specific protocol
function Unblock-Protocol {
    param([string]$Name)
    
    Assert-IsAdmin
    $ruleName = "WINCOMMANDER_BLOCK_${Name}"

    Remove-NetFirewallRule -DisplayName "${ruleName}_Out" -ErrorAction SilentlyContinue
    Remove-NetFirewallRule -DisplayName "${ruleName}_In" -ErrorAction SilentlyContinue
    
    return @{ status = 'removed'; name = $ruleName }
}

# Get current protocol blocks
function Get-ProtocolBlocks {
    # Fetch all WinCommander block rules more robustly
    $allRules = Get-NetFirewallRule -DisplayName "WINCOMMANDER_BLOCK_*" -ErrorAction SilentlyContinue
    
    # Debug logging
    # ("Found rules count: " + @($allRules).Count) | Out-File -FilePath "C:\Users\Public\wc_firewall_debug.txt" -Append
    
    if (-not $allRules) {


        return @{ blocks = @() }
    }


    # Debug: Check grouping logic
    $grouped = $allRules | Group-Object { $_.DisplayName -replace '_Out$|_In$', '' }
    
    $blocks = $grouped | ForEach-Object {
        $name = $_.Name -replace 'WINCOMMANDER_BLOCK_', ''

        # Determine direction based on rules present
        $hasOut = ($_.Group | Where-Object { $_.Direction -eq 'Outbound' })
        $hasIn  = ($_.Group | Where-Object { $_.Direction -eq 'Inbound' })

        $dir = "Both"
        if ($hasOut -and -not $hasIn) { $dir = "Outbound" }
        elseif (-not $hasOut -and $hasIn) { $dir = "Inbound" }

        # Inbound uses LocalPort, outbound uses RemotePort. Read the
        # right field per direction. Inbound takes precedence as the
        # "display" port when both directions are present (it'll match
        # what the user typed since outbound's RemotePort = inbound's
        # LocalPort = the port they entered).
        $rule = if ($hasIn) { $hasIn | Select-Object -First 1 } else { $hasOut | Select-Object -First 1 }
        $portFilter = $rule | Get-NetFirewallPortFilter
        $rawPort = if ($rule.Direction -eq 'Inbound') { $portFilter.LocalPort } else { $portFilter.RemotePort }
        $portStr = if ($rawPort -is [array]) { $rawPort -join ',' } else { $rawPort }

        @{
            Name      = $name
            Protocol  = $portFilter.Protocol
            Port      = $portStr
            Direction = $dir
            Enabled   = $rule.Enabled
        }
    }

    # Ensure $blocks is explicitly cast to an array of objects
    [array]$blocksArray = $blocks
    
    return @{ blocks = $blocksArray }
}
