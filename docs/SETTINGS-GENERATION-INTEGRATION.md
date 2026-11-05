# Settings generation integration checkpoint

The migration implementation is experimental. Normal builds retain the current
main-branch format and guarded-key writes. A Windows build with the non-default
`experimental-settings-generation` feature AND process environment variable
`WINCOMMANDER_EXPERIMENTAL_SETTINGS_GENERATION=1` routes machine-section reads
and writes through the experimental lock and authority guard. It does not
provision the lock or initialize a generation. Current-user installations use
user-scoped DPAPI for the new generation and fail with an account/profile
recovery message if Windows cannot unlock it; they never retry the new generation
with machine-scope protection. Do not ship or enable it before the checks below.

The machine installer has a possible privileged provisioning point in
`NSIS_HOOK_POSTINSTALL`, after `WC_REPAIR_SHARED_MACHINE_DATA_ACL_OR_ABORT`.
It must not invoke the experimental provisioner until an independently
verified known-folder root and its parent ACL are held against substitution,
the lock file's exact ACL is checked on the held handle, and elevated
Administrator, LocalSystem and standard-user tests pass. The existing helper
uses environment-derived paths and is not itself proof of trusted root identity.
The installer currently stops the exact installed desktop image and the named
service; that is not proof that every old executable or writer has stopped.

Desktop settings are preloaded in `run` before Tauri `setup`, and some
headless command paths run even earlier. A future selection gate must run
before every settings read or write across those paths, not only in `setup`.
Stable selected data should use short-lived shared read ownership; pending
state needs authorized exclusive recovery. Initial recovery must verify a
fresh held source snapshot before selecting the new file. The old store and
signing key must remain recoverable, with explicit failure reporting and no
fallback from a corrupt selected generation.

Current-user installations need a separate LocalAppData identity/ACL check,
user-scope protection and interactive, noninteractive, restart and cross-user
acceptance. Machine storage needs standard-user read access and an installer-
provisioned, tested lock ACL. All participating writers must be coordinated,
and running older versions must be fenced. The current protected marker does
not prevent rollback of both marker and generation; an external trusted anchor
or an explicitly accepted recovery policy is still needed. Do not enable a
runtime or installer route until these conditions and interruption tests pass.

The Windows machine-lock fixture tests are marked ignored in ordinary test
runs because they require an elevated token. Acceptance must explicitly run
`cargo test -p commander-free elevated_provision_is_idempotent -- --ignored`
and `cargo test -p commander-free settings_rotation_trial::tests -- --ignored`
in both elevated Administrator and LocalSystem contexts, followed by separate
standard-user versus privileged-process cross-account tests. An ignored test
is not acceptance evidence.

The gated `inspect_startup_once` seam now makes a read-only decision under a
short-lived shared lock: an explicitly provisioned legacy anchor and no selected
generation is `Legacy`, a verified active pair is `Selected`, and a pending
pair is `RecoveryRequired`. A missing anchor or lock, invalid files or scope
mismatch are errors. The explicit Windows lab opt-in uses this decision in
datastore reads and writes, not a separately wired startup step. The current
file-backed DPAPI-protected anchor is a draft, not an independent anti-rollback
anchor; restoring an older matching anchor and generation remains undetectable.

For a current-user installation, the gated route requires a pinned LocalAppData
root and a pre-provisioned lock whose ACL matches the current Windows account.
The legacy user key keeps its existing compatibility handling; newly selected
generations use the current user's DPAPI scope. SSH/noninteractive access may
therefore require the same account's profile to be loaded and has not been
accepted until tested. Experimental F6 state distinguishes a verified source
with no signing key from an existing key; an existing empty or malformed seed
fails closed. Restoring both old generation and authority remains an unsolved
rollback case without a trusted anchor.

The gated experiment now uses one protected cutover record for both the runtime
route and explicit migration draft. An explicit migration can stage and verify
an encrypted candidate, record the pending transition, and select it without
removing legacy files. Interrupted initial selection requires the exact staged
candidate and a fresh source check. No installer or startup command invokes the
migration. The current record is file-backed under the same data root and is
not an independent anti-rollback anchor; it cannot detect restoration of an old
matching record and generation. It must not be represented as rollback proof.

A gated read encountering an interrupted update now releases its shared lock,
acquires exclusive ownership, and reconciles only a verified old or new
selected generation. If exclusive access is unavailable it reports that an
authorized account or administrator must recover it. An interrupted initial
selection still requires a fresh guarded source snapshot. A separate explicit
fresh-install provisioning seam refuses existing data; it is not called by
normal startup and is not proof that an installation is truly fresh. Existing
stores with a missing cutover anchor fail closed and are never automatically
reclassified as legacy.
