# Documentation

This directory is the map for detailed, area-specific WinCommander references.
Start with the repository-wide documents at the root: [README](../README.md),
[features](../FEATURES.md), [architecture](../ARCHITECTURE.md),
[security](../SECURITY.md), [open-core model](../OPEN_CORE.md),
[positioning](../POSITIONING.md), [non-goals](../NON-GOALS.md), and
[changelog](../CHANGELOG.md), and [performance](../PERFORMANCE.md). Current
operational limits are in [security](../SECURITY.md).

## Detailed references

- [cli.md](cli.md) — Free command-line automation and its safety contract. It
  remains at this path because the generated CLI catalog validates it.
- [engineering/](engineering/) — component-specific engineering references,
  including the [IPC catalog](engineering/ipc.md).
- [frontend/](frontend/) — frontend-facing references, including the complete
  [settings and toggle reference](frontend/settings-reference.md).
- [product/](product/) — detailed product-area references, including the
  [automation flows reference](product/flows.md).

Add new detailed documentation to the narrowest relevant folder. Keep
repository-wide product truth and core architecture/security references at the
repository root; update this map and all affected Markdown links when adding
or moving a document.
