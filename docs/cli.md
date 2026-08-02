# WinCommander Free command line

WinCommander Free uses one executable for both the desktop app and headless automation. Launching `wincommander-free.exe` normally opens the GUI. The explicit verbs `commands`, `audit`, `run`, and `help` switch the same executable into CLI mode.

Run it from an elevated terminal because the shipped executable retains WinCommander's administrator manifest. Output is one JSON document on stdout and failures use a nonzero process exit code.

```powershell
wincommander-free.exe commands list
wincommander-free.exe commands describe backend:Get-SystemInfo
wincommander-free.exe commands describe tauri:get_log_records
wincommander-free.exe audit catalog
wincommander-free.exe run backend:Get-SystemInfo --params '{}'
wincommander-free.exe run tauri:get_log_records --params '{"limit":10,"levels":["error"]}'
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
- `--timeout-ms` is available only for read-only native Tauri commands and sets a wait deadline from 100 ms to 60 minutes. It is not transactional cancellation.
- Mutating and destructive CLI commands run to completion and are serialized across CLI processes in the current Windows session.
- Parameters must be a JSON object supplied inline, as `@path`, or as `-` for stdin. They are parsed as data and are never evaluated as shell text.
- Existing licence, module, administrator, investigator-mode, and Pro-sidecar checks are preserved because CLI commands use the same backend and Tauri dispatchers as the GUI.

Examples:

```powershell
# Safe preview only; this does not clear the DNS cache.
wincommander-free.exe run backend:Clear-DnsCache --dry-run

# A real destructive operation requires the exact token returned by describe.
wincommander-free.exe run backend:Clear-DnsCache `
  --confirm DESTROY:backend:Clear-DnsCache
```

All 763 backend-script commands and all 416 release Tauri handlers are executable from the shipped Free binary. Debug builds additionally expose the four debug-only handlers, for 1,183 executable commands in total. Release builds keep those four entries cataloged for drift auditing but refuse to execute them.

Native commands run through a minimal hidden Tauri runtime. CLI mode does not mount the React dashboard, create a tray icon, show a taskbar window, register hotkeys, start ambient monitors, write autostart state, or begin updater polling. The process exits after one response. A read-only wait timeout returns exit code 10; mutating commands cannot be cut off with this option while external work may still be running. Terminal Lockdown commands may return a detached acknowledgement because the production handler requests application exit after launching its cleanup worker.

## Developer checks

The catalog is generated from the actual Tauri handler registry, backend dispatcher/tier gate, and production frontend call sites.

```powershell
bun run gen:cli-catalog
bun run gen:cli-catalog:check
bun test tools/cli-catalog.test.ts
cargo test -p commander-free --lib cli::tests
```

The generated JSON escapes command separators before it is embedded, preventing contiguous paid-command names from reappearing in the Free binary. JSON decoding restores the real identifiers at runtime.
