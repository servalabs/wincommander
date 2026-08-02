# WinCommander Free command line

WinCommander Free uses one executable for both the desktop app and headless automation. Launching `wincommander-free.exe` normally opens the GUI. The explicit verbs `commands`, `audit`, `run`, and `help` switch the same executable into CLI mode.

Run it from an elevated terminal because the shipped executable retains WinCommander's administrator manifest. Every CLI request writes one structured JSON document to stdout; the process exit code carries the outcome.

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

## Catalog and runtime

The generated catalog contains 1,183 entries: 763 backend-script commands and 420 Tauri handlers. Four Tauri handlers are debug-only, so the shipped release binary executes 1,179 commands; it retains the four debug-only entries for catalog-drift auditing and refuses them at runtime.

Backend commands run in a windowless Tauri context. Native commands run through a real, invisible `cli-runtime.html` Tauri WebView that invokes the same production handler as the GUI. CLI mode never mounts the React dashboard or creates a tray icon, taskbar window, hotkeys, ambient monitors, autostart state, or updater polling.

Native Tauri commands require Microsoft Edge WebView2. If it is unavailable, CLI mode returns one JSON error with `error: "runtime_prerequisite"` and exit code `9`; it does not display the GUI prerequisite dialog.

## Safety model

`commands describe` reports the command transport, tier, risk, frontend references, headless support, and exact confirmation token.

- Read-only commands run without a confirmation token.
- Mutating commands require `--confirm RUN:<command-id>`.
- Destructive commands require `--confirm DESTROY:<command-id>`.
- `--dry-run` never invokes a dispatcher and reports what would execute.
- `--timeout-ms` is enforced only for read-only native Tauri commands and sets a wait deadline from 100 ms to 60 minutes; the default native deadline is 300,000 ms (5 minutes). It is a wait limit, not transactional cancellation.
- The parser accepts `--timeout-ms` on a read-only backend-script command, but that existing backend dispatcher has no deadline and ignores it. Automation must not rely on a backend-script timeout.
- Mutating and destructive CLI commands do not accept `--timeout-ms`; they run to completion and are serialized across CLI processes in the current Windows session.
- Parameters must be a JSON object supplied inline, as `@path`, or as `-` for stdin. Native Tauri dispatch preserves JSON values; backend-script dispatch serializes object values to their string/JSON representations for its existing dispatcher. Neither path evaluates parameter text as shell code.
- Existing licence, module, administrator, investigator-mode, and Pro-sidecar checks are preserved because CLI commands use the same backend and Tauri dispatchers as the GUI.

Examples:

```powershell
# Safe preview only; this does not clear the DNS cache.
wincommander-free.exe run backend:Clear-DnsCache --dry-run

# A real destructive operation requires the exact token returned by describe.
wincommander-free.exe run backend:Clear-DnsCache `
  --confirm DESTROY:backend:Clear-DnsCache
```

The process exits after one response. A read-only native wait timeout returns exit code `10`; it cannot cancel already-started external work. Terminal `tauri:lockdown` and `tauri:full_lockdown` may instead return `{ "detached": true, "processExitRequested": true }` with exit code `0`, because the production handler requests application exit after it launches its cleanup worker. That payload acknowledges only that the worker was started and process exit was requested; it is not a wipe-completion result.

## Exit codes

| Code | Meaning |
| :-- | :-- |
| `0` | Success, dry-run, or a detached terminal-lockdown acknowledgement |
| `2` | Invalid CLI arguments, including a timeout on a mutating or destructive command |
| `3` | Required exact confirmation token missing or incorrect |
| `4` | Unknown catalog command |
| `5` | Embedded catalog error |
| `6` | Catalog audit found a missing dispatcher or adapter |
| `7` | Command is cataloged but unavailable for this build/runtime |
| `8` | Dispatcher/handler execution failed |
| `9` | Native runtime, WebView2 prerequisite, or invoke-bridge failure |
| `10` | Read-only native command exceeded its requested/default wait deadline |
| `11` | Another mutating or destructive CLI command holds the cross-process execution lock |

## Developer checks

The catalog is generated from the actual Tauri handler registry, backend dispatcher/tier gate, and production frontend call sites.

```powershell
bun run gen:cli-catalog
bun run gen:cli-catalog:check
bun test tools/cli-catalog.test.ts
cargo test -p commander-free --lib cli::tests
```

The generated JSON escapes command separators before it is embedded, preventing contiguous paid-command names from reappearing in the Free binary. JSON decoding restores the real identifiers at runtime.
