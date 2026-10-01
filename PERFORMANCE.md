# Performance — WinCommander

This document owns repository-wide responsiveness behavior, measurement scope,
and public performance evidence. It does not define product capabilities or
architecture; see [FEATURES.md](FEATURES.md) and [ARCHITECTURE.md](ARCHITECTURE.md).

## Responsiveness design

- A small startup entry mounts the actual animated splash before loading the
  application modules. The application updates that same animation instance;
  there is no separate text loading screen. A bounded saved-theme read selects
  one appearance for splash and dashboard. The splash waits for startup readiness and does not replay
  after calculator lock/unlock.
- Splash appearance is fixed for each launch and isolated from dashboard CSS.
  Settings hydration cannot restart the rain, title scramble, or CSS animation
  clocks. Cached branding is used on the next launch. Error/retry pauses and
  resumes the same animation; it does not initialize a second canvas.
- Packaged startup uses a bundled stylesheet accepted by the release CSP.
  The native window is revealed after the splash DOM is committed; artwork,
  fonts and animation frames do not gate visibility. Animation clocks wait for
  the reveal acknowledgement or its soft time budget. A late acknowledgement
  keeps the animation running; explicitly suppressed launches stay hidden.
- Settings hydrate from the local cache before background system probes.
  A slow initial read stays shared after the soft recovery budget expires.
  Only a confirmed read failure offers Retry startup; elapsed time alone does
  not fail the launch. The dashboard remains gated until settings are known.
  Routine cold reads do not persist launch metadata before publishing their
  cache; first-run identity and legacy migration retain their initial save.
  Best-effort user-directory ACL helpers and service disconnect cleanup have
  five-second limits, so those helpers cannot wait indefinitely during startup.
  Settings observers run after the settings write lock is released; settings
  IPC and slow diagnostics/network checks run outside the window event thread.
- One startup coordinator shares duplicate work and permits one expensive launch
  probe or scan at a time. Panel and Disk Cleanup warming are intent/idle-led.
  Queued jobs have a bounded admission wait. A frontend timeout does not stop a
  native operation or release its occupied scan slot.
- Startup hardware refresh shares the expensive probe queue. Returning to the
  dashboard does not repeat that scan. Automatic drive-health probing waits for
  30 seconds on the dashboard; recent app inventory is reused, and automatic
  inventory refresh is deferred for two minutes before idle scheduling.
- Dashboard cards share one live-metrics request. Automatic panel polling waits
  for startup completion, pauses while the document is hidden, and refreshes on
  return. CPU/RAM, temperature, disk, process and SMART collection use separate
  single-flight workers; a stalled disk query cannot hold up CPU/RAM or the
  window event loop. Disk readings refresh at most once every five seconds.
  Failed or pending probes retain their last reading with freshness status.
  Metrics child processes have bounded execution/output and are stopped on
  timeout. A blocked OS call retains its worker slot rather than spawning more
  workers. Identity, licence, portable state, settings and mutation inputs are
  not cached in those snapshots.
- The panel poller subscribes only to the stable metrics refresh action, not
  each metric sample. Clipboard categories and phrase registration synchronize
  on content changes rather than new object/array identities on every render.
  Diagnostic read/write/retention locks are scoped to the canonical profile log
  directory, so another user's private log does not share the same lock.
- Settings readers use a committed snapshot while storage transactions run
  behind a separate gate. Native full-object writes reject stale snapshots;
  observers run outside both locks. Initial security settings load before window
  setup with a bounded wait; failure leaves the interface closed without resetting
  settings. Service settings work runs off pipe-I/O threads, with bounded queues
  and independent per-account serialization using authenticated ownership.
- Startup module loading has a bounded window-reveal acknowledgement wait.
  Focus requests do not attach to another application's input queue or inject
  keyboard input. Changing foreground applications must not gate initialization.
- Sidecar admission, request writes and shutdown have bounded waits. Only requests
  known not to have been sent may retry automatically; an interrupted write or
  missing response is an unknown outcome, not permission to replay a mutation.
- Serialized Vault service operations mark both lock acquisition and execution
  as blocking work while preserving the caller's Windows impersonation thread.
  Queued status reads must not starve the broker's pipe I/O or timeout timers.
  Mount/dismount response waits are bounded to three minutes and are never
  replayed automatically after an ambiguous transport failure. A timeout is
  an unknown result, not proof that no mount occurred.
- Available drive letters are preloaded at app startup into the shared in-memory
  query cache and refreshed every thirty seconds while the app is visible.
  Secure Storage and Quick Mount open from that cache, not a new letter scan.
  Manual refresh and mount submission recheck availability; the service remains
  authoritative when allocating a drive. Fleet policy-specific exclusions are
  not stored in this common cache.
- Secure Storage refreshes its authorized mount inventory every five seconds
  while the panel is visible, with one initial check when no observation exists.
  Clicking, focusing, or returning to the panel does not start an extra check.
  Hidden panels do not poll it. Manual Refresh and post-operation checks start
  fresh reads, even during a background check; older replies cannot replace
  newer observations. Manual Refresh is busy until its foreground reads finish.
  Failed checks retain the last confirmed rows with a stale warning and disabled
  actions; a confirmed empty result removes them.
- The native trace uses one monotonic clock for allowlisted native phases and
  frontend job milestones. It stores no paths, settings values, command
  arguments, licence material, or error text.

## File-icon reuse

Native icon lookup keeps the existing eight-request concurrency limit and
visible-row priority. Reusable results use a least-recently-used cache capped at
1,024 entries and 8 MiB of conservatively counted UTF-16 key/data strings; the
entry cap also bounds per-entry overhead, which is not included in that byte
figure. Oversized icons still reach waiting rows but are not retained.

Successful entries expire after five minutes and unavailable/failed lookups
after ten seconds. Expiry is lazy: a later lookup reloads the icon; no recurring
timer or automatic retry loop runs while a row is idle. This bounds retention
and permits recovery; it does not claim a measured rendering-speed improvement
or immediate notification of every Windows icon-association change.

## Startup sample reports

`bun run startup:benchmark samples.json report.json --require-complete`
validates and summarizes measurements supplied by a separate capture run. It
does not launch the application. Omit `report.json` to write JSON to stdout.
An output file must be new: existing reports and raw inputs are never replaced,
including when another filename points to the same file.

The versioned input is `{ "schemaVersion": 1, "metadata": { ... }, "samples": [...] }`.
Use a separate envelope whenever the build, machine, fixture, or protection
configuration changes. Each sample has a `scenario` and an `elapsedMs` object.
Scenarios are `warm`, `cold`, `first-install`, `offline`, and `downloads-50k`.
Timings are finite, non-negative milliseconds from the same monotonic process
clock. The required milestones, in order, are:

1. `process_start` (zero)
2. `native_setup_entered`
3. `main_window_show_requested`
4. `webview_dom_ready`
5. `settings_cache_hydrated`
6. `dashboard_first_visible`
7. `dashboard_interactive`
8. `fresh_system_probe_complete`
9. `background_idle`

Also record exactly one protection outcome from the same clock:
`protection_required_ready`, `protection_not_required`, or `protection_failed`.
It must occur by `background_idle`. A failed outcome cannot qualify successful
startup data; the other outcomes must agree with `protectionRequired`.

| Metadata field | Required value |
| --- | --- |
| `freeRevision`, `proRevision` | Full 40-character source commit hashes |
| `freeArtifactHash`, `proArtifactHash` | SHA-256 of the measured artifacts, 64 hexadecimal characters |
| `machineId` | Stable non-sensitive machine label, not a user name or serial number |
| `windowsVersion`, `webviewVersion` | Versions actually used during capture |
| `capturedAt` | UTC ISO timestamp including milliseconds, such as `2026-09-22T00:00:00.000Z` |
| `downloadsEntries` | Non-negative integer; exactly `50000` for the `downloads-50k` scenario |
| `protectionRequired` | Boolean reflecting the measured launch configuration |

For a Free-only measurement, set both Pro fields to `null`. Text labels are
limited to 160 characters; unknown metadata fields are rejected. Hashes identify
inputs but the helper cannot attest that those artifacts produced the samples.

Legacy arrays and valid partial samples remain usable for diagnostics without
`--require-complete`. Empty timings, unknown scenarios/phases, malformed numbers,
conflicting outcomes, and reordered milestones are rejected and cause exit 1.
Strict mode also exits 1 for missing metadata or incomplete milestones. A valid
report still records these issues so omitted samples cannot disappear silently.
Malformed JSON or an invalid envelope fails without a report.

Reports contain per-phase sample counts, p50, nearest-rank p95, and maximum.
`dataComplete` only means every **supplied** sample has the required fields and
successful outcome. It does not require all scenarios, sufficient repetitions,
or a performance budget pass; inspect `unmeasuredScenarios` and the counts.
`reportOnly` remains true and `externalGates` lists evidence this tool cannot
establish. `generatedAt` is report time; `metadata.capturedAt` is capture time.
Synthetic test fixtures verify the helper, not startup speed or installed
Windows behavior. Preserve original captures beside generated reports.

## Public measurement status

The Windows client uses Tauri 2.12 and Tao 0.37 or newer for the upstream
keyboard-event reentrancy correction. Keyboard message inspection must not
re-enter the window callback while its keyboard-state mutex is held. This
dependency requirement does not establish installed RDP/session-transition
acceptance or a timing benchmark.

No reproducible public baseline, device-class budget, or benchmark result is
currently published. Treat responsiveness statements as design descriptions,
not measured performance claims. This document owns that gap; it is not
qualified elsewhere.

The loading logo covers document/module loading, not time before WebView2
creates the document. A recorded dashboard render milestone is not proof that
every native panel request responds; launch and panel interaction require
separate Windows measurements.

`tools/check-startup-continuity.cjs` exercises the actual animation in Chromium
against a running Vite server (URL argument, default `http://127.0.0.1:1435`).
It requires Playwright; `WINCOMMANDER_PLAYWRIGHT_MODULE` can point to an existing
installation. It checks canvas identity, animation clocks, late styles/settings,
error recovery, and one completion. This is browser evidence, not installed
Windows startup timing.

`tools/check-startup-release.cjs` checks built assets against the packaged CSP
constraints across dark, light, system and stale-cache themes. This detects
runtime inline-style rejection that an unrestricted Vite test cannot detect.
It does not replace launching the release EXE on a clean Windows machine.

`tools/check-startup-reveal-budget.cjs` uses the same Playwright configuration
against a Vite server (URL argument, default `http://127.0.0.1:1438`). It leaves
the native reveal reply unresolved and checks that application module loading
continues while another browser page accepts input. This verifies bootstrap
timeout handling, not Windows foreground activation or multi-session behavior.
