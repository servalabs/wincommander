# Encrypted search index cutover: experimental design

Status: experimental, disabled-by-default research. Do not enable it in production. The index
contains potentially sensitive extracted content and is currently plaintext.

- Create a random 256-bit data key for each Windows user and index identity.
  Protect it with user-scoped DPAPI and bind its protected record to the index
  identity. Do not reuse machine-scoped install material or put keys in .env.
  Authentication or unprotect failure must be an explicit error, never a cue
  to replace the key or open a plaintext index.
- Under an exclusive, cross-process per-index lock, create a separate secured
  destination. Rebuild from original documents into the authenticated directory;
  do not migrate plaintext segments in place. Validate a real reader, search,
  restart, and identity/key binding before publication. Preserve the old index
  untouched while the new build is incomplete or fails.
- Publish the selected generation with an authenticated, durable atomic commit
  and an identity-confined handle-based Windows operation. On restart, a durable
  marker must distinguish an uncommitted candidate from a committed index;
  fail closed on missing keys, ambiguous state, and unexpected replacement.
  Reader and writer selection must agree across processes and users.
- The held Tantivy writer lock does not fence legacy schema cleanup or other
  direct file mutations. A fresh install with no old index also cannot acquire
  that lock. Both cases need explicit handling before activation.
- Rebuilds currently remain unselected caches. Triple source hashing was
  removed: it was expensive and could not prove that extraction saw the same
  bytes, timestamps, or paths. Proof publication fails closed until a reviewed
  source snapshot or equivalent guarantee and durable selection are available.
- Stop old writers during cutover. Remove the old index only after the new
  generation is verified and all older processes are excluded; report failures
  honestly. Deleting files does not guarantee physical erasure on SSDs, backups,
  snapshots, or filesystem journals. Avoid plaintext temporary index files.
- Experimental format v2 derives a separate key for each randomly generated
  256-bit file ID, uses distinct counter nonces for chunks and the footer, and
  caps each file at 65,536 chunks. This depends on fresh OS randomness across
  crashes and needs native streaming validation and cryptographic review before
  release. The format changed from v1; existing experimental files cannot be
  reopened and must be rebuilt in a fresh unselected generation.
- Before activation, verify large Tantivy reads against the prototype 8 MiB
  range limit, file identity and rename confinement, crash boundaries, disk-full
  recovery, stale processes, cross-user isolation, and installed Windows service
  behavior. Fixed test keys and the experimental Directory are not production key custody.

The portable chunk tests do not establish any of these production properties.
