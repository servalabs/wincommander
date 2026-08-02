# WinCommander Free command line

WinCommander Free uses one executable for both the desktop app and headless automation. Launching `wincommander-free.exe` normally opens the GUI. The explicit verbs `commands`, `audit`, `run`, and `help` switch the same executable into CLI mode.

Run it from an elevated terminal because the shipped executable retains WinCommander's administrator manifest. Output is one JSON document on stdout and failures use a nonzero process exit code.

```powershell
wincommander-free.exe commands list
wincommander-free.exe commands describe backend:Get-SystemInfo
wincommander-free.exe audit catalog
wincommander-free.exe run backend:Get-SystemInfo --params '{}'
```

The executable remains a Windows-subsystem application so Explorer and context-menu launches never flash a console. For deterministic PowerShell automation, wait for the process and redirect its streams:

```powershell
$process = Start-Process .\wincommander-free.exe `
  -ArgumentList @('audit', 'catalog') `
  -Wait -PassThru `
  -RedirectStandardOutput .\audit.json `
  -RedirectStandardError .\audit.stderr
exit $process.ExitCode
```

## Safety model

`commands describe` reports the command transport, tier, risk, frontend references, headless support, and exact confirmation token.

- Read-only commands run without a confirmation token.
- Mutating commands require `--confirm RUN:<command-id>`.
- Destructive commands require `--confirm DESTROY:<command-id>`.
- `--dry-run` never invokes the backend and reports what would execute.
- Parameters must be a JSON object supplied inline, as `@path`, or as `-` for stdin. They are parsed as data and are never evaluated as shell text.
- Existing licence, module, administrator, investigator-mode, and Pro-sidecar checks are preserved because CLI backend commands use the same dispatcher as the GUI.

Examples:

```powershell
# Safe preview only; this does not clear the DNS cache.
wincommander-free.exe run backend:Clear-DnsCache --dry-run

# A real destructive operation requires the exact token returned by describe.
wincommander-free.exe run backend:Clear-DnsCache `
  --confirm DESTROY:backend:Clear-DnsCache
```

Backend-script commands are executable headlessly. Native Tauri commands are included in the catalog; commands without a shared headless adapter report `cataloged`, while window-only operations report `ui-only`. Attempting either returns `headless_not_enabled` instead of silently opening the GUI.

## Developer checks

The catalog is generated from the actual Tauri handler registry, backend dispatcher/tier gate, and production frontend call sites.

```powershell
bun run gen:cli-catalog
bun run gen:cli-catalog:check
bun test tools/cli-catalog.test.ts
cargo test -p commander-free --lib cli::tests
```

The generated JSON escapes command separators before it is embedded, preventing contiguous paid-command names from reappearing in the Free binary. JSON decoding restores the real identifiers at runtime.
