# Package managers

Packages & Apps checks each available package manager independently. A missing
optional manager never blocks WinGet, npm, or another independent provider.

## Scoop

Scoop is optional. If it is unavailable or cannot run, WinCommander leaves its
Scoop-only updates unavailable and continues to show the other providers.

WinCommander never downloads or runs Scoop's remote PowerShell bootstrap
script—during startup, refreshes, inventory scans, repairs, or a background
task. The upstream bootstrap has no package identity or pinned artifact hash
that WinCommander can verify before execution. Install Scoop only through your
organization-approved process, then use **Check updates** to verify that the
same execution path can run `scoop --version`.
