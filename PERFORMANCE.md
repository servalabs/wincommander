# Performance — WinCommander

This document owns repository-wide responsiveness behavior, measurement scope,
and public performance evidence. It does not define product capabilities or
architecture; see [FEATURES.md](FEATURES.md) and [ARCHITECTURE.md](ARCHITECTURE.md).

## Responsiveness design

- A small startup entry mounts the actual animated splash before loading the
  application modules. The application updates that same animation instance;
  there is no separate text loading screen. Saved theme retrieval does not
  gate rendering. The splash waits for startup readiness and does not replay
  after calculator lock/unlock.
- Splash appearance is fixed for each launch and isolated from dashboard CSS.
  Settings hydration cannot restart the rain, title scramble, or CSS animation
  clocks. Cached branding is used on the next launch. Error/retry pauses and
  resumes the same animation; it does not initialize a second canvas.
- Settings hydrate from the local cache before background system probes.
  A failed initial read has one bounded recovery wait, then the splash offers
  Retry startup while the dashboard remains gated.
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
- Frequent live metrics share a bounded five-second native disk snapshot while
  CPU and RAM stay live. Identity, licence, portable state, settings, and
  mutation inputs are not cached in that snapshot.
- The native trace uses one monotonic clock for allowlisted native phases and
  frontend job milestones. It stores no paths, settings values, command
  arguments, licence material, or error text.

## Public measurement status

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
