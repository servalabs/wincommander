# Package managers

Packages & Apps checks each available package manager independently. A missing
optional manager never blocks WinGet, npm, or another independent provider.

## Chocolatey and Scoop

Chocolatey and Scoop are optional. If either is unavailable or cannot run,
WinCommander leaves that provider's updates unavailable and continues to show
the other providers. Neither counts as an engine, appears in engine readiness,
or runs as part of **Install all**.

WinCommander never installs Chocolatey or Scoop—during startup, refreshes,
inventory scans, repairs, or a background task. Install either only through
your organization-approved process, then use **Check updates** to verify that
the same execution path can run its version command.
