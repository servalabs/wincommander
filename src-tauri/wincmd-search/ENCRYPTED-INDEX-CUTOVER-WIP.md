# Encrypted search index cutover: experimental design

Status: test-only research. Do not enable the prototype in production. The index
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
- Stop old writers during cutover. Remove the old index only after the new
  generation is verified and all older processes are excluded; report failures
  honestly. Deleting files does not guarantee physical erasure on SSDs, backups,
  snapshots, or filesystem journals. Avoid plaintext temporary index files.
- Before activation, verify large Tantivy reads against the prototype 8 MiB
  range limit, file identity and rename confinement, crash boundaries, disk-full
  recovery, stale processes, cross-user isolation, and installed Windows service
  behavior. Fixed test keys and the test-only Directory are not key custody.

The portable chunk tests do not establish any of these production properties.
