# Release workflow

## 1. Normal changes

Merge reviewed pull requests into `main` with a conventional-commit title:

- `fix: ...` — next patch release
- `feat: ...` — next minor release
- `feat!: ...` — next major release

`chore:`, `ci:`, `docs:`, and `refactor:` do not create a release by themselves.

## 2. Release pull request

The `prepare release` workflow creates or updates one Release Please pull
request. It updates the three Free version sources together:

- `package.json`
- `src-tauri/commander-free/tauri.conf.json`
- `src-tauri/commander-free/Cargo.toml`

Review its CI and diff like any other pull request. Merging it creates the
protected `vX.Y.Z` tag.

## 3. Signed MSI publication

The tag starts `release.yml`. It pauses at the protected `release` environment.
An approved reviewer must select **Review deployments** then **Approve and
deploy**. The workflow builds, Tauri-signs, attests, and uploads the Free MSI,
signature, manifest, checksum, and SBOM to that existing GitHub Release.

## 4. Required one-time configuration

Create the `RELEASE_AUTOMATION_TOKEN` repository Actions secret. Use a
fine-grained personal access token owned by the approved release account,
restricted to this repository with only these permissions:

- Contents — Read and write
- Pull requests — Read and write
- Issues — Read and write
- Metadata — Read-only

The release-tag ruleset must include that token's account in its always-allow
bypass list. Keep `v*` creation, update, and deletion restricted for everyone
else. This token creates release PRs and tags only; it is not a signing key.

Tauri signing keys stay in the protected `release` environment. Do not put them
in ordinary repository secrets.
