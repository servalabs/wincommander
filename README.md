<div align="center">

# WinCommander

### Take back control of your Windows.

One command console to switch off Microsoft's tracking, block trackers on your network, erase your digital leftovers, and stand guard against ransomware — local-first, zero telemetry.

[![Latest release](https://img.shields.io/github/v/release/servalabs/wincommander)](https://github.com/servalabs/wincommander/releases)
[![License: AGPL-3.0](https://img.shields.io/badge/license-AGPL--3.0-blue)](LICENSE)
[![Platform: Windows 11 + Server](https://img.shields.io/badge/platform-Windows%2011%20%2B%20Server-0078D4)](https://github.com/servalabs/wincommander/releases)
[![Telemetry: zero](https://img.shields.io/badge/telemetry-zero-brightgreen)](SECURITY.md)

[**Download**](https://github.com/servalabs/wincommander/releases) · [Features](FEATURES.md) · [Security](SECURITY.md) · [Why it exists](POSITIONING.md)

</div>

## Why WinCommander

Windows scatters its privacy controls across ~150 settings panels, quietly keeps histories and caches about everything you do, and phones home by default. The usual fixes are either opaque "optimizer" apps you can't trust or loose scripts you can't reason about.

WinCommander is the third option: a single, open-source command console that shows you exactly how locked-down your PC is — and lets you fix it in clicks.

## Top features

- **Live privacy score** — one 0–100 number for how locked-down your PC is, with exactly what to fix next. Drift from your ideal state is detected continuously.
- **Kill Windows tracking** — the telemetry, ads, and data-collection settings Microsoft spreads across ~150 panels, all in one place.
- **Block trackers network-wide** — one-click blocklists for ads, telemetry, and known bad hosts.
- **Find any file instantly** — press `Ctrl+Space` (default hotkey) anywhere for fast search across file names *and* file contents. The index never leaves your machine.
- **Erase your trail** — clear the histories, caches, and junk Windows quietly keeps about what you do.
- **Stand guard 24/7** — Free includes ransomware/USB attach alerts, clipboard watch, and a VPN kill-switch; Pro adds attack attribution, device intelligence, reactive containment, and a human-presence approval challenge for newly detected keyboards.
- **Emergency lockdown** — an opt-in panic sequence you can fire with a single hotkey (default `Ctrl+Shift+Q`) when it matters most.

**No accounts. No cloud. No product telemetry or analytics.** Network use is limited to the update check, licence and trial activation (device hash only), downloads of optional components when you enable the feature that needs them (for example the Everything search CLI, the Pro installer, and detector models for Privacy Shield), and Fleet traffic only after you explicitly enrol a device in an organization. This list comes from reviewing the source and is not an exhaustive network audit; see [SECURITY.md](SECURITY.md).

Settings backups created in Backup & Restore are password-protected. Keep the password safe: a backup cannot be restored without it. Existing version 2 plaintext settings backups can still be imported; version 1 and versionless backups cannot be imported safely. Older plaintext backups remain readable to anyone who can access them.

Other tools that have access to WinCommander's settings interface may still read settings in plaintext; the backup password protects the exported backup file.

## Install

1. Download the latest installer from [Releases](https://github.com/servalabs/wincommander/releases).
2. For desktop operation, use Windows 11 (the supported target). Windows Server 2019, 2022 and 2025 with Desktop Experience and WebView2 are targeted, but live Server/RDS validation is still pending ([ROADMAP.md](ROADMAP.md)). Most features need Administrator. Server Core is not a full GUI target: it has no Explorer/taskbar, and CLI commands work only when WebView2 and that command's prerequisites are installed. See [SECURITY.md](SECURITY.md) for current operational limits.
3. Follow the guided first-run tour. You can switch between the **Casual** and **Secure** profiles later from the sidebar.

Or run from source:

```powershell
git clone https://github.com/servalabs/wincommander.git
cd wincommander
git submodule update --init --recursive -- assets
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\dev.ps1
```

`tools/dev.ps1` installs the pinned Bun and Rust toolchain, then starts Tauri; the
Visual Studio C++ Build Tools and Windows SDK remain manual prerequisites. See
[CONTRIBUTING.md](CONTRIBUTING.md#local-setup) for development modes,
administrator testing and the Free-only workflow.

## Verify your download

Every release is built in public CI and verifiable end to end. The signed NSIS setup,
updater manifest, signature, SBOM, and SHA-256 checksum are published together
on the GitHub Release:

- **Build provenance (SLSA)** — each installer carries a Sigstore-backed attestation tying it to the exact public commit and CI run that built it:

  ```powershell
  gh attestation verify ".\WinCommander_<version>_x64-setup.exe" -R servalabs/wincommander
  ```

- **Dependency inventory** — every release includes a CycloneDX SBOM for offline vulnerability scanning.

## Learn more

| Doc                                | What's inside                                                 |
| :--------------------------------- | :------------------------------------------------------------ |
| [FEATURES.md](FEATURES.md)         | Public capability and Free/Pro/Fleet/Investigator boundary    |
| [ARCHITECTURE.md](ARCHITECTURE.md) | Public architecture, trust boundaries, and source map         |
| [SECURITY.md](SECURITY.md)         | Threat model, security posture, how to report a vulnerability |
| [PERFORMANCE.md](PERFORMANCE.md)   | Responsiveness design and public measurement status           |
| [POSITIONING.md](POSITIONING.md)   | Who it's for and how it compares to alternatives              |
| [NON-GOALS.md](NON-GOALS.md)       | What WinCommander deliberately is not                         |
| [ROADMAP.md](ROADMAP.md)           | Deferred, in-progress, partial and pending work               |
| [OPEN_CORE.md](OPEN_CORE.md)       | Free/Pro licence and source boundary                          |
| [CHANGELOG.md](CHANGELOG.md)       | Release-facing change history                                 |
| [CONTRIBUTING.md](CONTRIBUTING.md) | How to contribute                                             |

## Licensing

- **WinCommander Free** (this repo) — [AGPL-3.0](LICENSE), free and open source.
- **WinCommander Pro** — proprietary, governed by the [WinCommander EULA](https://servalabs.com/eula).
- [Privacy Policy](https://servalabs.com/privacy)

## Security & source review

ServaLabs welcomes review of the Pro source code — press, technology reviewers, and security researchers can contact [licensing@servalabs.com](mailto:licensing@servalabs.com) for access. To report a vulnerability, see [SECURITY.md](SECURITY.md).
