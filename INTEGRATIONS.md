# Integrations — WinCommander Free

External services, tools and components that the public Free client talks to or
installs, with their trust and data boundaries. Derived from a source review of
`src-tauri/commander-free`, `src-tauri/commander-svc` and the PowerShell
modules; it is not an exhaustive network audit. Private Pro/Fleet service
internals are out of scope ([OPEN_CORE.md](OPEN_CORE.md)). Security limits are
owned by [SECURITY.md](SECURITY.md); there is no product telemetry or analytics
endpoint in the Free client.

## Network services

| Integration | Used for | Data sent | Boundary |
| :-- | :-- | :-- | :-- |
| Update host (`winupdates.servalabs.com`) | Update check and signed artifact download | Update request only | The host is pinned in Rust (`ALLOWED_UPDATE_HOST` in `pro_install.rs`); artifacts are minisign-verified; the frontend cannot choose host or hash. Debug builds never check for updates. |
| Licence service (`wincommander-licensing.servalabs.com`) | Licence refresh, one 16-day trial per device | Device hash for trial start; licence identifiers for refresh | Signed Ed25519 token verified in the client; trial and Pro need it, Free does not. Portable builds have no trial. |
| DNS-over-HTTPS fallback (Cloudflare `1.1.1.1`, hostname-pinned, 5 s timeout) | Resolve the update and licence hosts when system DNS fails | Host names of those two services | Does not weaken artifact verification (`net.rs`). |
| Fleet (organization-run service) | Managed policy, commands, reporting | Only after explicit enrollment; contents are bounded by the organization's configuration | See [FEATURES.md](FEATURES.md#fleet) and [SECURITY.md](SECURITY.md#privacy-and-managed-monitoring). Wire contracts are in `src-tauri/fleet-proto`. |

## Optional components downloaded or installed on demand

| Component | When | Verification noted in source |
| :-- | :-- | :-- |
| Everything + `es.exe` CLI (voidtools) | Instant Search install | `es.exe` Authenticode signer is checked (`scripts/core/utils.ps1`); Everything via winget. |
| ImDisk (ltr-data.se) | RAM-disk engine install | Download URLs are in `dependencies.ps1`; installer verification is an open limit ([WEAKNESSES.md](WEAKNESSES.md#optional-third-party-components)). |
| Pro sidecar (`wincommander-pro.exe`) | Entitled Pro install | Pinned host, SHA-256 check, atomic install (`pro_install.rs`). |
| Investigator application | Licence with the Investigator entitlement | Minisign-verified manifest and host-pinned hashes (`investigator_install.rs`). |
| Privacy Shield detector models (Google MediaPipe model storage) | When Privacy Shield prepares its models | Size sanity check only; see [WEAKNESSES.md](WEAKNESSES.md#optional-third-party-components). |
| Firefox extensions (addons.mozilla.org) | Browser hardening option | Installed through Firefox policy by add-on slug; see [WEAKNESSES.md](WEAKNESSES.md#optional-third-party-components). |
| winget packages | Packages & Apps and dependency installs | Delegated to winget; Windows Server 2019/2022 lack winget. |

## Local-only integrations

| Integration | Boundary |
| :-- | :-- |
| ActivityWatch | Reads the user's local ActivityWatch over fixed loopback HTTP when installed and enabled; productivity view only. Supports classic server/watchers and the experimental Tauri app's managed watcher lifecycle. API-key protected servers report authorization required; this integration does not read stored keys or disable authentication. |
| Tailscale / ProtonVPN | The VPN kill-switch polls `tailscale.exe ip` and `netsh interface`; no account access. |
| Syncthing, VeraCrypt (Pro Vault) | Driven locally by the paid component and service; Syncthing device pairing stays the user's action ([FEATURES.md](FEATURES.md#pro)). |
| BleachBit | Used by the self-destruct cleanup step (`run_bleachbit_clean`); browsers are excluded there. |
| Windows Task Scheduler, WMI/CIM, Windows policy providers | The mechanism for scheduled cleanup, USB history, Bluetooth policy and similar controls. |

## Conditional or not integrated

- Cloud storage, accounts and telemetry sinks: none in Free.
- Provider-native Wazuh/Velociraptor administration, a plugin marketplace and
  third-party executable SDKs are out of scope ([NON-GOALS.md](NON-GOALS.md)).
