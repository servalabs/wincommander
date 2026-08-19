# Free R2 release workflow

The Free updater reads the R2-backed public update domain directly:

`https://winupdates.servalabs.com/free/latest.json`

The protected public release workflow builds and Tauri-signs the MSI, attests
its provenance, then uploads the MSI, minisign signature, SHA-256 checksum,
and SBOM to R2 under `/free/vX.Y.Z/`. Once those immutable artifacts are
available, it updates `/free/latest.json` as the signed updater pointer. The
same artifacts remain attached to the versioned GitHub Release as the public
release archive, but Free does not use GitHub Releases for update checks or
downloads. The Free release bucket is `windows`.

## Publish an exact version

The usual path is to tag the commits you want to ship. Pushing that tag starts
the release workflow: it updates the Free version files if they are still on
the previous version, then builds the signed MSI and publishes to R2.

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
   else still uses **Review deployments**. The MSI job then signs the
   installer, copies the artifacts to R2, and publishes the GitHub Release.

The reviewable alternative is unchanged: run **prepare release** with the
exact version, merge the `release/vX.Y.Z` pull request, and let the tag
workflow create `vX.Y.Z`. That tag push starts the same MSI + R2 job.

The updater manifest points to the versioned R2 MSI, so the manifest and
installer cannot drift. The release environment must provide `CF_UPDATE_DOMAIN`,
`CF_R2_API`, `R2_KEY`, and `R2_SECRET`.

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
starts the same MSI job.

## Free bundle media boundary

The shared `assets` submodule also supports website and other-product media.
The Free desktop build imports only its explicit runtime asset allowlist from
`src/assets.ts`; adding a file to the submodule does not put it in the MSI.
The release workflow rejects bundled installers, archives, Theron media, and
media payloads larger than 64 MiB. Add a genuinely required desktop asset to
the allowlist and release verification patterns in the same change.
