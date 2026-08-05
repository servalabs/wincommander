# R2 release workflow

The Free updater reads `https://winupdates.servalabs.com/free/latest.json`.
Release the signed MSI and its Tauri manifest from the private
`wincommander-pro` checkout, which uploads the versioned artifact and promotes
the R2 `free/latest.json` pointer.

## Publish an exact version

First commit and push the exact Free and Pro source to their `main` branches.
The release tool refuses dirty worktrees so the built installer always matches
the tagged source.

From `wincommander-pro`, run:

```powershell
.\tools\release.ps1 -Version 3.2.7 -Variant free -SkipFleet
```

Use `-Variant both` instead when the matching Pro, Investigator, and Fleet
artifacts must also be published. The tool updates aligned version metadata,
builds and Tauri-signs the Free MSI, uploads the MSI/signature/versioned
manifest to R2, promotes `free/latest.json`, then commits, tags, and pushes the
release source.

## Required local secrets

The private `wincommander-pro/.env` must provide the R2 credentials and update
domain, and the release shell must have the Tauri signing key/password. The
tool validates the signed MSI before it publishes the update pointer.

The public GitHub release workflow is manual-only, so pushing the legacy
release tag does not publish a second GitHub release.
