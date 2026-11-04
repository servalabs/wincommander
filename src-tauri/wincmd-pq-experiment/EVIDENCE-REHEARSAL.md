# Experimental evidence re-attestation

This opt-in prototype signs a digest and size of an existing archive with both
Ed25519 and ML-DSA-65 under a dedicated `evidence-attestation` purpose. The
new detached statement is separate from the archive and does not change it.
It does not establish the original evidence provenance, identity, creation time,
or that the archive was protected against quantum attacks in the past.

`evidence_attestation::sign` requires independently managed signing callbacks.
There is no production key custody, issuer, user interface or release process.
Callers must verify the original bundle independently and prevent changes while
reading it. The experimental offline checker can be built with the
`experimental-offline-verifier` feature:

```text
wincmd-pq-evidence-check <attestation.json> <archive> <independent-trust.json>
```

The independently obtained trust file has `ed_key_id`, `ed_public`, `pq_key_id`,
`pq_public` and a positive `minimum_epoch`. Public keys are unpadded base64url;
the caller must authenticate this file outside the archive. The Windows checker
holds the archive without write/delete sharing while reading; other platforms
need separate concurrency controls. Verification has a 4 GiB input limit. It
checks both signatures and the exact bytes, but not original case validity,
signer key revocation, trusted time, rollback or production key governance.
