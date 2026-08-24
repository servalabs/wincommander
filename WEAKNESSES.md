# Weaknesses — WinCommander

This is the authoritative public record of current open limitations and
validation gaps. Deliberate product boundaries are in [NON-GOALS.md](NON-GOALS.md),
security threat assumptions are in [SECURITY.md](SECURITY.md), and shipped
changes belong in [CHANGELOG.md](CHANGELOG.md).

## Environment and availability

- Server Core is not a complete GUI target: it has no Explorer or taskbar. CLI
  use also depends on WebView2 and the selected command's prerequisites.
- Windows Server/RDS, physical USB, hardware-backed keys, signed installers,
  and clean-machine behavior still require environment-specific validation.
- A UI feature can be unavailable when the Windows edition, hardware, policy,
  privilege, entitlement, or private component does not support it.

## Detection, containment, and erase assurance

- Ransomware alerts and response can reduce time to detection or containment;
  they cannot guarantee that no file changes before Windows observes activity.
- Unknown-keyboard approval is reactive. It cannot guarantee first-keystroke,
  pre-boot, firmware-level, or fast-replug prevention, and a UI confirmation is
  not proof of trusted physical input.
- Secure deletion and crypto-erasure depend on media behavior, firmware,
  encryption state, escrow, and verification. A successful request or removed
  local access does not prove every recovery copy is gone.
- Fleet reachability is not an air gap. Offline devices cannot receive a new
  command until they reconnect.
- Planned or source-tested functionality is not a shipped or independently
  certified guarantee.

## CLI automation

- Settings writes are last-writer-wins. The CLI serializes its own mutations,
  but the desktop app does not take that lock; a concurrent GUI settings write
  can discard one side.
- Search-index readers and reindex/replacement share a bounded session-local
  lock and fail closed with `busy`; signed-build GUI/CLI contention acceptance
  remains pending.
- Confirmation tokens prevent mistakes and mistargeting, not use by someone
  already able to run the executable.
- A destructive command's exit code reports dispatch, not completion.
  `lockdown` and `full_lockdown` acknowledge only that their worker launched.
- Risk grading deliberately over-classifies some erase-scheduling commands
  (`Set-AutoEraseSchedule`, `Set-MultiUserAutoEraseSchedule`, and
  `Set-ShredPolicy`), so they require `DESTROY:` confirmation.

## Automation flows

- The v2 manual-test bridge only re-syncs rules and reports an acknowledgement;
  it does not dispatch an individual rule. Its execution log is a capped,
  frontend-only event stream that is lost when the panel closes or the app
  restarts.
- The legacy `CameraTrigger` is permanently disabled; use v2 `GazeTrigger`.
- Legacy execution history is an in-memory 50-run ring and is lost on restart.
  Destructive legacy flows have no debounce, actions stop at the first failure,
  and there is no rollback or continue-on-failure behavior.
- Legacy `ShellAction` runs unrestricted PowerShell and `HTTPAction` can reach
  an arbitrary URL. They still execute only in the legacy engine for
  backward-compatibility; v2 treats them as deserialize-only variants and
  disables them during migration rather than executing them.
- Legacy `SignalReceivedTrigger` polls the Taildrop inbox. It can break if the
  VPN relocates that directory; migration to the shared filesystem watcher is
  pending.

## Performance evidence

The public documentation has no reproducible benchmark baseline or device-class
budget. [PERFORMANCE.md](PERFORMANCE.md) records the current responsiveness
design and measurement scope.
