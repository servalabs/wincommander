# Free R2 release workflow

The Free updater reads the R2-backed public update domain directly:

`https://winupdates.servalabs.com/free/latest.json`

The protected public release workflow builds and Tauri-signs the MSI, attests
its provenance, then uploads the MSI, minisign signature, SHA-256 checksum,
and SBOM to R2 under `/free/vX.Y.Z/`. Once those immutable artifacts are
available, it updates `/free/latest.json` as the signed updater pointer. The
same artifacts remain attached to the versioned GitHub Release as the public
release archive, but Free does not use GitHub Releases for update checks or
downloads.

## Publish an exact version

1. Run **prepare release** in GitHub Actions with the new version. It opens a
   `release/vX.Y.Z` pull request containing the aligned Free version files.
2. Merge that PR. The tag workflow creates the protected `vX.Y.Z` tag and
   invokes the release workflow.
3. Approve the protected `release` environment. The workflow builds from the
   tag, copies the artifacts to R2 and its updater manifest, then publishes the
   same files to the GitHub Release archive.

The updater manifest points to the versioned R2 MSI, so the manifest and
installer cannot drift. The release environment must provide `CF_UPDATE_DOMAIN`,
`CF_R2_API`, `R2_KEY`, and `R2_SECRET`.
