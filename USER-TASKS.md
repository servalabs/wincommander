# User tasks — WinCommander Free

External, manual or owner-side work that code changes cannot finish: live
acceptance runs and decisions. Each item says what outcome closes it. Open
engineering work is in [ROADMAP.md](ROADMAP.md); the limits these tasks address
are in [WEAKNESSES.md](WEAKNESSES.md).

## Acceptance runs (need real Windows)

Record results in the release or pull-request notes, not as new files here.
Remove an item when it has passed on the intended build.

1. **Secure erase on live Explorer selections.** Files and folders, with a
   junction or hard link swapped in after selection, an open handle, and a
   read-only file. The code and Windows unit tests exist
   (`src-tauri/commander-context-shred`); a real Explorer run does not.
2. **Installed-service acceptance.** Standard user versus administrator,
   Windows password reset, simultaneous standard and administrator users,
   named-pipe peer metadata grant, personal settings and Vault flows
   ([SECURITY.md](SECURITY.md)).
3. **Windows Server 2019/2022/2025 with Desktop Experience and RDS sessions**
   (idle-timeout dismount, user switching, tray behaviour).
4. **Physical USB** attach/detach timeline and Pro approval flows on real
   devices.
5. **Signed installer, installed update path and clean-machine install**,
   including the matched Pro pairing for Secure Storage copy-check/restore.
6. **Selective VeraCrypt header destruction**: re-mount the container after the
   overwrite to confirm it can no longer open.
7. **Flows and Privacy Shield**: confirm gaze triggers fire from real camera
   events end to end before enabling CameraTrigger.
8. **Performance baseline**: capture the scenarios in
   [PERFORMANCE.md](PERFORMANCE.md) on named Windows machines and run
   `bun run startup:benchmark`; publish the report there.

## Decisions and owner actions

1. **Standard-user evidence storage and oversized files.** `main` was
   fast-forwarded to `dev` on 2026-11-16; the clippy and unit-test failures were
   fixed the same day, but the standard-user evidence-storage regression and the
   oversized files remain ([STATUS.md](STATUS.md#the-2026-11-16-merge-of-dev),
   [WEAKNESSES.md](WEAKNESSES.md#code-health)). Decide the order. The
   standard-user fix needs the service-mediated design in [RESEARCH.md](RESEARCH.md#conditional-future-opportunities) to be implemented first.
2. **Post-quantum path.** Decide key custody, a signing ceremony and an
   independent reviewer before any production hybrid signing or public claim
   ([SECURITY.md](SECURITY.md#cryptography-and-post-quantum-status)).
3. **CODEOWNERS.** Replace the `@servalabs-owner` placeholder in
   `.github/CODEOWNERS` with a real handle or team so code-owner review can
   apply to `fleet-proto`, `fleet-agent-core`, `wincmd-shared` and
   `deny.toml`.
4. **Branch protection.** Confirm in GitHub settings that `main` is protected
   and which checks are required; [CONTRIBUTING.md](CONTRIBUTING.md) describes
   a pull-request and auto-merge flow that cannot be verified from the
   repository.
5. **Root `latest.*.json` manifests.** Decide whether they are still used by
   the release pipeline. They record `3.2.8` while the app is `3.6.7`; refresh
   them or delete them.
6. **Docs check on documentation-only changes.** Decide whether the cost of
   running the invariants workflow on `*.md` changes is acceptable, or add a
   separate lightweight docs-only workflow.
7. **Third-party download hardening.** Decide whether to pin or sign-check the
   ImDisk installer, Privacy Shield models and Firefox extensions
   ([WEAKNESSES.md](WEAKNESSES.md#optional-third-party-components)).
8. **Internal references.** Decide where the audit reports cited by finding ID
   in source comments live (they are not in this public repository).
9. **Third-party licence notices.** `LICENSES/optimizerDuck-GPL-3.0.txt` is not
   referenced by any attribution file, and there is no `THIRD_PARTY_NOTICES`.
   Decide what it covers and add the notice ([OPEN_CORE.md](OPEN_CORE.md)).
