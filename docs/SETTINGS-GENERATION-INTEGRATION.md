# Settings generation integration checkpoint

The migration implementation is experimental. `experimental-settings-generation`
only compiles the isolated seams; it does not provision a file, select a
generation, change live datastore reads or writes, or turn on migration. The
normal datastore retains the current main-branch format and guarded-key writes.

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
