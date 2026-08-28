# Performance — WinCommander

This document owns repository-wide responsiveness behavior, measurement scope,
and public performance evidence. It does not define product capabilities or
architecture; see [FEATURES.md](FEATURES.md) and [ARCHITECTURE.md](ARCHITECTURE.md).

## Responsiveness design

- Settings hydrate from the local cache before background system probes; normal
  cached launches do not wait for the decorative splash.
- One startup coordinator shares duplicate work and permits one expensive launch
  probe or scan at a time. Panel and Disk Cleanup warming are intent/idle-led.
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
