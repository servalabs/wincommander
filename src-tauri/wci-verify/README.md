# Legacy case-bundle checker

This checker handles only the legacy layout with `hashes.txt`, `signature.sig`,
`verify-pubkey.hex` and a signed manifest hash in conventional sha256sum form.
It requires an Ed25519 public key obtained independently from the bundle:

```text
wci-verify <bundle.case.zip> --trusted-key <64-hex-character-public-key>
```

The key inside an archive is not proof of identity. Unknown hash-list formats,
missing audit chains, unlisted files and mismatched keys or hashes are rejected.
This changes the old one-argument command: it previously could report success
without authenticating the manifest. Evidence files are not modified.

Current Investigator `wci-case-bundle-v1` archives use `manifest.sig` and an
identity certificate, and are not this legacy layout. Verify those using the
Investigator verifier and an independently obtained root fingerprint. This
legacy format uses classical Ed25519 only; it does not provide post-quantum
signatures. It has not been qualified for untrusted oversized archives or
concurrent changes to the bundle file.
