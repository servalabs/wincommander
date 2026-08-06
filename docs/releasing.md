# Free GitHub Release workflow

The Free updater reads GitHub Releases directly:

`https://github.com/servalabs/wincommander/releases/latest/download/latest.json`

The protected public release workflow builds and Tauri-signs the MSI, attests
its provenance, then publishes the MSI, signature, `latest.json`, checksum,
and SBOM to the versioned GitHub Release. Free does not use R2.

## Publish an exact version

1. Run **prepare release** in GitHub Actions with the new version. It opens a
   `release/vX.Y.Z` pull request containing the aligned Free version files.
2. Merge that PR. The tag workflow creates the protected `vX.Y.Z` tag and
   invokes the release workflow.
3. Approve the protected `release` environment. The workflow builds from the
   tag and publishes the GitHub Release.

The Tauri updater manifest always points to the MSI in the same versioned
GitHub Release, so the manifest and installer cannot drift.

## Existing Free installations on the legacy R2 endpoint

Installations released before this GitHub endpoint still query
`/free/latest.json` on R2. After the new GitHub Release is live, run the
one-time bridge from the private checkout:

```powershell
.\tools\migrate-free-updater-to-github.ps1 -Version X.Y.Z -Publish
```

It copies only the verified GitHub `latest.json` to the legacy R2 pointer. The
manifest sends old clients to the signed MSI on GitHub; it never uploads a Free
installer or signature to R2. Keep the bridge pointer while those versions are
still supported.
