# Free R2 release workflow

The Free updater reads the R2-backed public update domain directly:

`https://winupdates.servalabs.com/free/latest.json`

The protected public release workflow builds and Tauri-signs the NSIS setup,
attests its provenance, then uploads the setup, minisign signature, SHA-256 checksum,
and SBOM to R2 under `/free/vX.Y.Z/`. Once those immutable artifacts are
available, it updates `/free/latest.json` as the signed updater pointer. The
same artifacts remain attached to the versioned GitHub Release as the public
release archive, but Free does not use GitHub Releases for update checks or
downloads. The Free release bucket is `windows`.

## Publish an exact version

The usual path is to tag the commits you want to ship. Pushing that tag starts
the release workflow: it updates the Free version files if they are still on
the previous version, then builds the signed NSIS setup and publishes to R2.

```powershell
git checkout main
git pull
git tag -a v3.2.17 -m "release: v3.2.17"
git push origin v3.2.17
```

1. The tag must look like `vX.Y.Z` or `vX.Y.Z-rc.1` and must point at
   `origin/main` if Cargo/`package.json` still need a bump. A tag whose
   version files already match can rebuild that exact commit.
2. When the version files do not match, the workflow commits
   `release: vX.Y.Z` on `main` (`package.json`, Free `tauri.conf.json`,
   Free `Cargo.toml`, and the Free `Cargo.lock` package version) and builds
   that commit.
3. If you (owner/admin) or a username in `RELEASE_AUTO_APPROVERS` pushed
   the tag, the workflow approves the `release` environment itself. Anyone
   else still uses **Review deployments**. The setup job then signs the
   installer, copies the artifacts to R2, and publishes the GitHub Release.

The reviewable alternative is unchanged: run **prepare release** with the
exact version, merge the `release/vX.Y.Z` pull request, and let the tag
workflow create `vX.Y.Z`. That tag push starts the same NSIS setup + R2 job.

The updater manifest points to the versioned R2 NSIS setup, so the manifest and
installer cannot drift. The release environment must provide `CF_UPDATE_DOMAIN`,
`CF_R2_API`, `R2_KEY`, and `R2_SECRET`.

## Manual Free publication

When the hosted workflow is unavailable, use the checked-in manual publisher
from a Windows release workstation. It is a release tool, not a replacement for
the workflow's GitHub OIDC provenance attestation.

```powershell
pwsh -File .\tools\release-free.ps1 -Version 3.4.6 -StageOnly
pwsh -File .\tools\release-free.ps1 -Version 3.4.6
```

It refuses a dirty checkout, a version mismatch, a tag not pointing at the
current `origin/main`, or a missing updater signature. The requested `v3.4.6`
tag and matching source version files must already exist; the script does not
change versions or move tags. It imports the private local `.env` without
printing it, builds the signed NSIS setup (unless `-SkipBuild` is explicitly
given), creates a CycloneDX SBOM with a SHA-256-pinned Syft archive when needed,
uploads and re-downloads all immutable R2 artifacts, then promotes `latest.exe`
and finally `latest.json`. It also creates or refreshes the GitHub Release
archive using the authenticated GitHub CLI.

`-StageOnly` is the safe rehearsal: it builds and validates the artifacts but
does not contact R2 or GitHub. A manual publication truthfully notes that no
GitHub Actions provenance attestation was generated; do not present it as an
attested workflow release.

## Who can release

A tag push is **not** owner-only by GitHub default. Anyone with Write on the
repo can push `v3.2.17` and start the workflow. The workflow itself now refuses to run unless `github.actor` is a
repository **admin** (org owners and people with the Admin role) **or** a
username listed in the repository variable `RELEASE_AUTO_APPROVERS`
(Settings → Secrets and variables → Actions → Variables). A Write
collaborator who is not on that list is rejected before any Cargo commit.

That check is the in-repo gate. Keep these GitHub settings so a workflow
edit cannot weaken it alone:

1. **Tag ruleset** — Settings → Rules → Rulesets → new tag ruleset.
   Target `refs/tags/v*`. Restrict create, update, and delete to
   **Repository admins** (and any named releaser if you add them to the
   ruleset bypass). Block force pushes.
2. **`release` environment** — Settings → Environments → `release`.
   Required reviewers = the owners. Deployment branches/tags = `v*`.
   Signing keys and R2 secrets live here.
3. **Turn off Prevent self-review** on that environment. If it is on,
   GitHub blocks the owner from approving their own tag (including the
   automation PAT). The PAT in `RELEASE_AUTOMATION_TOKEN` must belong to
   a required reviewer.
4. **`RELEASE_AUTO_APPROVERS`** — optional comma-separated GitHub
   logins, for example `alice,bob`. Those people can start a release and
   skip the Review deployments click without being admins.

When the actor is an owner/admin or is allowlisted, **prepare release**
also merges its version PR with `--admin` so CODEOWNERS on
`tauri.conf.json` does not wait for a second human. The tag push then
starts the same NSIS setup job.

## Free bundle media boundary

The shared `assets` submodule also supports website and other-product media.
The Free desktop build imports only its explicit runtime asset allowlist from
`src/assets.ts`; adding a file to the submodule does not put it in the setup.
The release workflow rejects bundled installers, archives, Theron media, and
media payloads larger than 64 MiB. Add a genuinely required desktop asset to
the allowlist and release verification patterns in the same change.

## Service payload

`bun run build:tauri:release` builds `commander-svc` before it creates a
temporary release-only Tauri configuration that bundles `wincommander-svc.exe`.
The installer copies it to `%ProgramFiles%\WinCommander\wincommander-svc.exe`,
registers the automatic LocalSystem `WinCommanderSvc`, and starts it. Updates
wait for the old service to stop before replacement; uninstall stops, deletes,
and removes it. The desktop manifest is `highestAvailable`, so a standard user
keeps their own Windows token while an administrator may elevate. The app and
service installation do not require a separate Authenticode certificate; this
does not change the release workflow's existing installer/updater signing.

The source contract tests cover the quoted service ImagePath and rollback of a
failed replacement to the previous executable/configuration. The release
workflow additionally runs the exact NSIS setup on a clean hosted runner and
requires it to install `wincommander-svc.exe`, create/configure/start
`WinCommanderSvc`, then uninstall and remove both. A failed lifecycle gate
blocks publishing; clean-machine endpoint acceptance remains a separate check.
