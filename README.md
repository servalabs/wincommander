<div align="center">

# WinCommander

### Take back control of your Windows.

One command console to switch off Microsoft's tracking, block trackers on your network, erase your digital leftovers, and stand guard against ransomware — local-first, zero telemetry.

[![Latest release](https://img.shields.io/github/v/release/servalabs/wincommander)](https://github.com/servalabs/wincommander/releases)
[![License: AGPL-3.0](https://img.shields.io/badge/license-AGPL--3.0-blue)](LICENSE)
[![Platform: Windows 11 + Server](https://img.shields.io/badge/platform-Windows%2011%20%2B%20Server-0078D4)](https://github.com/servalabs/wincommander/releases)
[![Telemetry: zero](https://img.shields.io/badge/telemetry-zero-brightgreen)](SECURITY.md)

[**Download**](https://github.com/servalabs/wincommander/releases) · [Features](FEATURES.md) · [Security](SECURITY.md) · [Why it exists](POSITIONING.md)

![WinCommander demo](docs/img/wc-dashboard.gif)

</div>

## Why WinCommander

Windows scatters its privacy controls across ~150 settings panels, quietly keeps histories and caches about everything you do, and phones home by default. The usual fixes are either opaque "optimizer" apps you can't trust or loose scripts you can't reason about.

WinCommander is the third option: a single, open-source command console that shows you exactly how locked-down your PC is — and lets you fix it in clicks.

## Top features

- **Live privacy score** — one 0–100 number for how locked-down your PC is, with exactly what to fix next. Drift from your ideal state is detected continuously.
- **Kill Windows tracking** — the telemetry, ads, and data-collection settings Microsoft spreads across ~150 panels, all in one place.
- **Block trackers network-wide** — one-click blocklists for ads, telemetry, and known bad hosts.
- **Find any file instantly** — press `Ctrl+Space` anywhere for fast search across file names *and* file contents. The index never leaves your machine.
- **Erase your trail** — clear the histories, caches, and junk Windows quietly keeps about what you do.
- **Stand guard 24/7** — Free includes ransomware/USB attach alerts, clipboard watch, and a VPN kill-switch; Pro adds attack attribution, device intelligence, reactive containment, and a human-presence approval challenge for newly detected keyboards.
- **Emergency lockdown** — an opt-in panic sequence you can fire with a single hotkey (`Ctrl+Shift+Q`) when it matters most.

**No accounts. No cloud. Zero telemetry.** The app's only network calls are the update check and (if you buy Pro) license activation.

## Install

1. Download the latest installer from [Releases](https://github.com/servalabs/wincommander/releases).
2. For desktop operation, use Windows 11 or Windows Server 2019, 2022, or 2025 with Desktop Experience and WebView2 (most features need Administrator). Server Core is not a full GUI target: it has no Explorer/taskbar, and CLI commands work only when WebView2 and that command's prerequisites are installed. See [WEAKNESSES.md](WEAKNESSES.md) for current validation limits.
3. Pick **Casual** or **Secure** in the first-run setup — that's it.

Or run from source:

```powershell
git clone https://github.com/servalabs/wincommander.git
cd wincommander
powershell -NoProfile -ExecutionPolicy Bypass -File .\tools\dev.ps1
```

`tools/dev.ps1` installs Bun 1.4.0 when absent, installs the exact Rust
toolchain/components pinned in `rust-toolchain.toml`, then starts Tauri. The
Visual Studio C++ Build Tools and Windows SDK remain manual prerequisites.

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
| [WEAKNESSES.md](WEAKNESSES.md)     | Current open limitations and validation gaps                  |
| [docs/README.md](docs/README.md)   | Detailed, area-specific documentation map                     |
| [docs/cli.md](docs/cli.md)         | Automate the same Free executable with JSON and safety gates   |
| [POSITIONING.md](POSITIONING.md)   | Who it's for and how it compares to alternatives              |
| [NON-GOALS.md](NON-GOALS.md)       | What WinCommander deliberately is not                         |
| [CONTRIBUTING.md](CONTRIBUTING.md) | How to contribute                                             |

## Licensing

- **WinCommander Free** (this repo) — [AGPL-3.0](LICENSE), free and open source.
- **WinCommander Pro** — proprietary, governed by the [WinCommander EULA](https://servalabs.com/eula).
- [Privacy Policy](https://servalabs.com/privacy)

## Security & source review

ServaLabs welcomes review of the Pro source code — press, technology reviewers, and security researchers can contact [licensing@servalabs.com](mailto:licensing@servalabs.com) for access. To report a vulnerability, see [SECURITY.md](SECURITY.md).
