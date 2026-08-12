# ============================================================================
# COMMAND ROUTER
# Dispatches commands to appropriate modules
# ============================================================================

function Invoke-BackendCommand {
    param(
        [string]$CommandName,
        [hashtable]$Parameters
    )

    # When called with no explicit args (the Rust dispatch path), read the
    # command name and parameters from the environment. Params arrive as a JSON
    # object in $env:WINCMD_PARAMS_JSON; keys become hashtable keys (DATA), never
    # interpolated into script text, so a compromised WebView cannot inject
    # PowerShell via a crafted param key (audit finding C1).
    if (-not $PSBoundParameters.ContainsKey('CommandName') -or [string]::IsNullOrEmpty($CommandName)) {
        $CommandName = $env:WINCMD_COMMAND
    }
    if (-not $PSBoundParameters.ContainsKey('Parameters')) {
        $Parameters = @{}
        $json = $env:WINCMD_PARAMS_JSON
        if (-not [string]::IsNullOrEmpty($json)) {
            $obj = $json | ConvertFrom-Json
            foreach ($prop in $obj.PSObject.Properties) {
                $Parameters[$prop.Name] = $prop.Value
            }
        }
    }

    try {
        # Command function is already loaded by Rust
        # Just execute it with parameters
        $result = & $CommandName @Parameters
        ConvertTo-JsonOutput $result
    }
    catch {
        $details = $_ | Out-String
        ConvertTo-JsonOutput @{
            error   = $true
            message = $_.Exception.Message
            details = $details.Trim()
            command = $CommandName
        }
        exit 1
    }
}
