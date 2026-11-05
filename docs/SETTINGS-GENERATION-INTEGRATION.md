# Settings generation integration checkpoint

The migration implementation is experimental. Normal builds retain the current
main-branch format and guarded-key writes. A Windows build with the non-default
`experimental-settings-generation` feature AND process environment variable
`WINCOMMANDER_EXPERIMENTAL_SETTINGS_GENERATION=1` routes machine-section reads
and writes through the experimental lock and authority guard. It does not
provision the lock or initialize a generation. Current-user installs reject
this opt-in. Do not ship or enable it before the checks below.

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
short-lived shared lock: no generation and no authority is `Legacy`, a verified
stable pair is `Selected`, and a pending pair is `RecoveryRequired`. Missing
provisioned lock, invalid files or scope mismatch are errors. The explicit Windows lab opt-in uses this decision in datastore reads and
writes, not a separately wired application startup step. Before routing, a trusted external marker must also
distinguish a first legacy start from deletion or rollback of both files.
