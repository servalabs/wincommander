// SPDX-License-Identifier: AGPL-3.0-or-later
//! Explicit feature-gated maintenance commands; normal builds omit this module.
//! The offline acknowledgement does not fence old executables; independently
//! enforced maintenance mode remains required before deployment.
#[derive(Debug, PartialEq, Eq)]
enum Action { Status, Fresh, Migrate, Install, Stage, NewIdentity, StageUser(String) }
fn parse(args: &[String], enabled: bool) -> Option<Result<Action, &'static str>> {
    if args.first().map(String::as_str) != Some("--settings-generation-lab") { return None; }
    if !enabled { return Some(Err("Settings migration lab is disabled; no data changed.")); }
    Some(match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["--settings-generation-lab", "status"] => Ok(Action::Status),
        ["--settings-generation-lab", "fresh", "--offline-confirmed"] => Ok(Action::Fresh),
        ["--settings-generation-lab", "migrate", "--offline-confirmed"] => Ok(Action::Migrate),
        ["--settings-generation-lab", "install", "--offline-confirmed"] => Ok(Action::Install),
        ["--settings-generation-lab", "stage", "--offline-confirmed"] => Ok(Action::Stage),
        ["--settings-generation-lab", "new-machine-identity", "--offline-confirmed", "--confirm-new-identity"] => Ok(Action::NewIdentity),
        ["--settings-generation-lab", "stage-user", "--offline-confirmed", "--expected-user-sid", sid] if !sid.is_empty() => Ok(Action::StageUser((*sid).to_owned())),
        _ => Err("Use status, or fresh/migrate/install/stage with --offline-confirmed. Existing data was preserved."),
    })
}
pub(crate) fn user_sid_matches(expected: &str, actual: &str) -> bool {
    !expected.is_empty() && expected.eq_ignore_ascii_case(actual)
}
#[cfg(windows)]
pub fn run(args: &[String]) -> Option<Result<&'static str, String>> {
    let enabled = std::env::var_os("WINCOMMANDER_EXPERIMENTAL_SETTINGS_GENERATION").as_deref() == Some(std::ffi::OsStr::new("1"));
    let action = match parse(args, enabled)? { Ok(action) => action, Err(error) => return Some(Err(error.into())) };
    Some(match action {
        Action::Status => crate::datastore::settings_generation_lab_status(),
        Action::Fresh | Action::Migrate | Action::Install | Action::Stage => Err("Use the installer source-proof flow; existing settings were preserved.".into()),
        Action::NewIdentity => crate::datastore::settings_generation_lab_create_new_identity().map(|_| "A new machine identity was created; previous identities were not recovered."),
        Action::StageUser(sid) => crate::datastore::settings_generation_lab_stage_user(&sid).map(|_| "User settings preflight complete; original store remains active."),
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    fn args(values: &[&str]) -> Vec<String> { values.iter().map(|value| value.to_string()).collect() }
    #[test]
    fn user_stage_requires_explicit_nonempty_original_user_sid() {
        let a = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert!(parse(&a(&["--settings-generation-lab", "stage-user", "--offline-confirmed"]), true).unwrap().is_err());
        assert!(parse(&a(&["--settings-generation-lab", "stage-user", "--offline-confirmed", "--expected-user-sid", ""]), true).unwrap().is_err());
        assert_eq!(parse(&a(&["--settings-generation-lab", "stage-user", "--offline-confirmed", "--expected-user-sid", "S-1-5-21-1"]), true).unwrap(), Ok(Action::StageUser("S-1-5-21-1".into())));
    }
    #[test]
    #[cfg(windows)]
    fn another_user_is_rejected_before_any_store_access() {
        assert!(crate::datastore::settings_generation_lab_stage_user("S-1-0-0").is_err());
    }
    #[test]
    fn original_user_sid_must_match_exactly() {
        assert!(user_sid_matches("S-1-5-21-123", "s-1-5-21-123"));
        assert!(!user_sid_matches("S-1-5-21-123", "S-1-5-21-124"));
        assert!(!user_sid_matches("", ""));
    }
    #[test]
    fn lab_commands_require_explicit_opt_in_and_offline_acknowledgement() {
        assert_eq!(parse(&args(&["--normal"]), true), None);
        assert!(parse(&args(&["--settings-generation-lab", "status"]), false).unwrap().is_err());
        assert_eq!(parse(&args(&["--settings-generation-lab", "status"]), true).unwrap(), Ok(Action::Status));
        assert!(parse(&args(&["--settings-generation-lab", "fresh"]), true).unwrap().is_err());
        assert!(parse(&args(&["--settings-generation-lab", "migrate", "--offline-confirmed", "extra"]), true).unwrap().is_err());
        assert_eq!(parse(&args(&["--settings-generation-lab", "fresh", "--offline-confirmed"]), true).unwrap(), Ok(Action::Fresh));
        assert_eq!(parse(&args(&["--settings-generation-lab", "migrate", "--offline-confirmed"]), true).unwrap(), Ok(Action::Migrate));
        assert!(parse(&args(&["--settings-generation-lab", "install"]), true).unwrap().is_err());
        assert_eq!(parse(&args(&["--settings-generation-lab", "install", "--offline-confirmed"]), true).unwrap(), Ok(Action::Install));
        assert!(parse(&args(&["--settings-generation-lab", "stage"]), true).unwrap().is_err());
        assert!(parse(&args(&["--settings-generation-lab", "new-machine-identity", "--offline-confirmed"]), true).unwrap().is_err());
        assert!(parse(&args(&["--settings-generation-lab", "new-machine-identity", "--confirm-new-identity"]), true).unwrap().is_err());
        assert_eq!(parse(&args(&["--settings-generation-lab", "new-machine-identity", "--offline-confirmed", "--confirm-new-identity"]), true).unwrap(), Ok(Action::NewIdentity));
        assert_eq!(parse(&args(&["--settings-generation-lab", "stage", "--offline-confirmed"]), true).unwrap(), Ok(Action::Stage));
    }
}

#[cfg(windows)]
pub fn hold_search_maintenance_fence(pid: u32) -> Result<(), String> {
    crate::datastore::hold_search_fence_for_installer(pid)
}

/// Strict internal installer protocol; the proof never grants identity consent.
#[cfg(windows)]
pub fn run_source_proof(args: &[String]) -> Result<(String, bool), String> {
    if std::env::var_os("WINCOMMANDER_EXPERIMENTAL_SETTINGS_GENERATION").as_deref() != Some(std::ffi::OsStr::new("1")) {
        return Err("Settings migration lab is disabled; existing data preserved.".into());
    }
    match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["--settings-source-proof-lab", "stage", "--offline-confirmed"] => crate::datastore::settings_generation_lab_stage_proof(),
        ["--settings-source-proof-lab", "install", "--offline-confirmed", proof] => {
            crate::datastore::settings_generation_lab_install_proof(proof)?;
            Ok((String::new(), false))
        },
        _ => Err("Invalid settings source-proof command; existing data preserved.".into()),
    }
}
