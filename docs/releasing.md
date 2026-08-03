# Release workflow

## 1. Prepare an exact version

Open **Actions** → **prepare release** → **Run workflow** and enter the exact
version. Use `3.2.5-rc.1` for a safe pre-release test; it does not become the
updater's `latest` release.

The workflow creates one normal release pull request and updates these Free
version sources together:

- `package.json`
- `src-tauri/commander-free/tauri.conf.json`
- `src-tauri/commander-free/Cargo.toml`

Review its CI and diff like any other pull request. Merging it automatically
creates the protected `vX.Y.Z` tag and starts the signed release workflow.

## 2. Signed MSI publication

The tag starts `release.yml`. It pauses at the protected `release` environment.
An approved reviewer must select **Review deployments** then **Approve and
deploy**. The workflow builds, Tauri-signs, attests, and uploads the Free MSI,
signature, manifest, checksum, and SBOM to that GitHub Release.

## 3. Required one-time configuration

Create the `RELEASE_AUTOMATION_TOKEN` repository Actions secret. Use a
fine-grained personal access token owned by the approved release account,
restricted to this repository with only these permissions:

- Contents — Read and write
- Pull requests — Read and write
- Metadata — Read-only

The release-tag ruleset must allow the token account to create `v*` tags. This
token creates release PRs and tags only; it is not a signing key.

Tauri signing keys stay in the protected `release` environment. Do not put them
in ordinary repository secrets.
