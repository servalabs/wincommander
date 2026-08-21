# Beta update channel plan

## Purpose

WinCommander will offer two update channels:

- **Stable** is the default for everyone and receives versions approved for general use.
- **Beta** is an opt-in test channel that receives a new version before Stable.

The existing Stable update experience must continue to work without a reinstall,
setting change, or change to the update trust model.

## Version rule

Each version number identifies one exact signed installer and is never reused.

Example:

```text
Stable: 3.2.18

Publish 3.2.19 to Beta
  - If it is approved, promote the same 3.2.19 build to Stable.
  - If it has an issue, fix it and publish 3.2.20 to Beta.
```

Do not rebuild or overwrite a published `3.2.19`. Promotion changes only which
channel points to the already tested, signed installer.

## Update locations

```text
/free/latest.json        Stable update manifest; existing installations use this.
/free/latest-beta.json   Beta update manifest.
/free/vX.Y.Z/...         Immutable signed installer, signature, checksum, and SBOM.
```

`/free/latest.json` remains the Stable location for backward compatibility.
Both manifests use the existing Tauri updater format and the existing pinned
update signing key.

## User setting

Add an **Update channel** choice in Settings > Versions & Updates:

```text
Stable (recommended)
Beta (receive new versions before general release)
```

This is independent of the existing **Automatically update WinCommander**
choice:

- Update channel chooses *which* release stream the app checks.
- Automatic updates chooses whether an available release downloads and stages
  automatically or waits for the user to review it.

The default is `stable` for new installations and upgraded installations.
Enabling Beta shows a short confirmation that beta builds may contain issues.

## Application implementation

1. Add `app.updateChannel` to the Rust and TypeScript settings models.
   - Valid values are `stable` and `beta`.
   - Missing, invalid, or damaged values safely resolve to `stable`.
   - Migrate existing settings explicitly to `stable`.

2. Keep the allowed update URLs inside the trusted Rust updater code.
   - The frontend only requests `stable` or `beta`; it never supplies a URL,
     hostname, signing key, or hash.
   - Reuse the current cache busting, DNS-over-HTTPS, timeout, and signature
     verification behavior for both channels.

3. Route every update path through the same channel resolver.
   - Background updater checks.
   - Manual "Check for updates".
   - Manual install.
   - Installation of a downloaded and staged update.

4. Include the selected channel in updater state and logs, so the interface can
   accurately say whether an available update is Stable or Beta.

5. On a channel change, discard a staged installer from the old channel and
   immediately check the newly selected channel.
   - This prevents an already downloaded Beta installer being installed after a
     user has changed back to Stable.
   - A beta user already on a version newer than Stable is never downgraded.
     Stable updates resume once Stable reaches that version or a newer one.

6. Respect existing update controls.
   - Beta does not bypass disabled updates, the automatic-update preference,
     install locks, or the current signature verification process.
   - A managed or locked setting cannot be changed through the interface.

## Release implementation

1. A version release builds and signs the installer once.
2. Upload and verify all immutable versioned files first.
3. Publish `/free/latest-beta.json` last. Stable remains unchanged.
4. Add a protected **Promote Beta to Stable** release action.
   - It accepts a tested version, for example `3.2.19`.
   - It verifies the existing manifest, signature, versioned installer, and
     checksum before making any Stable change.
   - It updates `/free/latest.json` last, pointing to the same tested build.
   - It does not rebuild or resign the installer.
5. An urgent security fix follows the same process, but is promoted immediately
   after its validation instead of waiting for the normal beta test period.

## Safety cases

| Situation | Required behavior |
| --- | --- |
| A Beta build has an issue | Fix it and publish the next version; do not overwrite the old version. |
| Beta is approved | Promote that exact version to Stable. |
| User turns Beta off | Check Stable; do not automatically downgrade the installed app. |
| User changes channel during download | Discard the staged old-channel installer. |
| Manifest, signature, or installer is missing | Do not publish the channel pointer. |
| Manifest points outside the official host | Reject the release. |
| A release tries to overwrite an existing version | Fail the release unless every file is byte-for-byte identical. |
| A Stable promotion is older than current Stable | Reject it; Stable never moves backward. |
| Internet, DNS, or server issue | Preserve current app; show the current retry/error behavior. |
| Critical security issue | Publish a new fixed version and promote it to Stable immediately. |

## Initial scope

The first implementation covers the WinCommander Free self-updater only.
Pro, Fleet, and Investigator retain their current Stable update contracts.
Therefore a Beta Free release must remain compatible with the current Stable
Pro component. Beta channels for those separate products can be designed later
as a separate, compatible expansion.

## Required validation

- Existing installations default to Stable and continue checking
  `/free/latest.json`.
- Beta users check `/free/latest-beta.json` immediately after opting in.
- Stable, Beta, manual, automatic, and staged update paths select the same
  expected channel.
- Invalid settings and failed network or signature checks safely leave the
  installed version unchanged.
- Switching from Beta to Stable cannot install stale Beta bytes or downgrade.
- Release automation rejects missing artifacts, invalid signatures, pointer
  mistakes, version reuse, and backward Stable promotion.
- Validate the real desktop update path on a clean install, an existing Stable
  installation, and an already-updated Beta installation.
