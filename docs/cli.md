# WinCommander Free command line

WinCommander Free uses one executable for both the desktop app and headless automation. Launching `wincommander-free.exe` normally opens the GUI. The explicit verbs `commands`, `audit`, `run`, and `help` switch the same executable into CLI mode.

The production CLI is read-only. Mutating and destructive commands remain
desktop-only until they share the desktop's native confirmation and
cross-process locking controls.

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

The generated catalog contains 1,278 entries: 806 backend-script commands and 472 Tauri handlers. Four Tauri handlers are debug-only, so the shipped release binary executes 1,274 commands; it retains the four debug-only entries for catalog-drift auditing and refuses them at runtime.

`audit catalog` reports a failure only for a genuinely missing dispatcher or a
registered command left without a declared execution boundary. A command marked
`uiOnly` is intentionally restricted to the desktop's confirmation and locking
path; it is not a missing headless adapter.

Backend commands run in a windowless Tauri context. Native commands run through a real, invisible `cli-runtime.html` Tauri WebView that invokes the same production handler as the GUI. CLI mode never mounts the React dashboard or creates a tray icon, taskbar window, hotkeys, ambient monitors, autostart state, or updater polling.

Native Tauri commands require Microsoft Edge WebView2. If it is unavailable, CLI mode returns one JSON error with `error: "runtime_prerequisite"` and exit code `9`; it does not display the GUI prerequisite dialog.

## Safety model

`commands describe` reports the command transport, tier, risk, frontend references, headless support, and exact confirmation token.

- Read-only commands run without a confirmation token.
- Mutating commands require `--confirm RUN:<command-id>`.
- Destructive commands require `--confirm DESTROY:<command-id>`.
- `--dry-run` never invokes a dispatcher and reports what would execute.
- `--timeout-ms` is enforced for every read-only command, native or backend-script, and sets a wait deadline from 100 ms to 60 minutes; the default deadline is 300,000 ms (5 minutes). The backend timer starts before Tauri runtime initialization, so a wedged startup is bounded too. It is a wait limit, not transactional cancellation: the CLI stops waiting, reports `error: "timeout"`, and exits `10`, but already-started work continues in whatever it started.
- Mutating and destructive CLI commands do not accept `--timeout-ms`; they run to completion and are serialized across CLI processes in the current Windows session.

Risk is assigned in a fail-closed priority order rather than by name alone:

1. `authz::DESTRUCTIVE_COMMANDS` — the same registry the CI authorization gate enforces — always wins. Adding a catastrophic command there is enough to make the CLI demand `DESTROY:`; it does not also need a frightening name. This is why `tauri:fleet_connect` and `tauri:internet_kill_switch_set` are destructive despite reading as ordinary mutations.
2. A small table of handlers whose name misreports their effect, each verified against the handler body. `tauri:search_rename_file` calls `std::fs::rename`, and the `tauri:export_*` commands write a new artefact, so none of them may hold the no-confirmation read-only tier their prefix implies — a read-only wait deadline must never be able to kill an export part-written.
3. The name prefix/token rules.
4. Any name containing `erase`, `shred`, `wipe`, or `destroy`, matched anywhere rather than only as a prefix, so the whole `Invoke-*Erase` family is destructive.

Step 4 runs after the read-only allowlist, so `Get-AutoEraseSchedules` remains a read while `Invoke-7Erase` is destructive. It deliberately over-classifies a few configuration commands that schedule erasure rather than performing it — `Set-AutoEraseSchedule`, `Set-MultiUserAutoEraseSchedule`, `Set-ShredPolicy` — which require `DESTROY:`. Classification remains a safeguard, not authentication; `commands describe <id>` always reports the exact token a command needs.
- Parameters must be a JSON object supplied inline, as `@path`, or as `-` for stdin. Native Tauri dispatch preserves JSON values; backend-script dispatch serializes object values to their string/JSON representations for its existing dispatcher. Neither path evaluates parameter text as shell code.
- Existing licence, module, administrator, investigator-mode, and Pro-sidecar checks are preserved because CLI commands use the same backend and Tauri dispatchers as the GUI.

The encrypted emergency-backup commands have fixed empty payloads. `backend:Get-EncryptedBackupTargetStatus` is read-only and may report only `bound` plus a schema version; it never returns the target path or identity hash. `backend:Provision-EncryptedBackupTarget` is mutating and `backend:Clear-EncryptedBackupTarget` is conservatively destructive, so both remain desktop-only under the current production CLI policy. The desktop backend derives the sole mounted file-container identity and rejects every supplied field.

Examples:

```powershell
# Safe preview only; this does not clear the DNS cache.
wincommander-free.exe run backend:Clear-DnsCache --dry-run

# A real destructive operation requires the exact token returned by describe.
wincommander-free.exe run backend:Clear-DnsCache `
  --confirm DESTROY:backend:Clear-DnsCache
```

The process exits after one response. A read-only wait timeout returns exit code `10`; it cannot cancel already-started external work. Terminal `tauri:lockdown` and `tauri:full_lockdown` may instead return `{ "detached": true, "processExitRequested": true }` with exit code `0`, because the production handler requests application exit after it launches its cleanup worker. That payload acknowledges only that the worker was started and process exit was requested; it is not a wipe-completion result.

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
| `10` | Read-only command exceeded its requested/default wait deadline |
| `11` | Another mutating or destructive CLI command holds the cross-process execution lock |

## Current limitations

This document owns the CLI's behavior and safety contract, including these
limits.

- Settings writes are last-writer-wins. The CLI serializes its own mutations,
  but the desktop app does not take that lock, so a concurrent GUI settings
  write can discard one side.
- Search-index readers and reindex/replacement share a bounded session-local
  lock and fail closed with `busy`. Signed-build GUI/CLI contention acceptance
  is still pending.
- Confirmation tokens prevent mistakes and mistargeting. They do not prevent
  use by someone already able to run the executable.
- A destructive command's exit code reports dispatch, not completion.
  `lockdown` and `full_lockdown` acknowledge only that their worker launched.
- Risk grading deliberately over-classifies some erase-scheduling commands
  (`Set-AutoEraseSchedule`, `Set-MultiUserAutoEraseSchedule`, `Set-ShredPolicy`),
  so they require `DESTROY:` confirmation.

Limits that qualify the product's security claims are in
[SECURITY.md](../SECURITY.md).

## Developer checks

The catalog is generated from the actual Tauri handler registry, backend dispatcher/tier gate, and production frontend call sites. It records each handler's `#[cfg(debug_assertions)]` gate as `debugOnly`, so the four debug-only handlers the release binary refuses are derived from the gate itself rather than from a hardcoded name list that could drift past the drift check.

```powershell
bun run gen:cli-catalog
bun run gen:cli-catalog:check
bun test tools/cli-catalog.test.ts
cargo test -p commander-free --lib cli::tests
```

The generated JSON escapes command separators before it is embedded, preventing contiguous paid-command names from reappearing in the Free binary. JSON decoding restores the real identifiers at runtime.
