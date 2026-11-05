// SPDX-License-Identifier: AGPL-3.0-or-later
//! Explicit debug-build lab commands. Never enabled in packaged release builds.
//! The offline acknowledgement does not fence old executables; independently
//! enforced maintenance mode remains required before deployment.
#[derive(Debug, PartialEq, Eq)]
enum Action { Status, Fresh, Migrate }
fn parse(args: &[String], enabled: bool) -> Option<Result<Action, &'static str>> {
    if args.first().map(String::as_str) != Some("--settings-generation-lab") { return None; }
    if !enabled { return Some(Err("Settings migration lab is disabled; no data changed.")); }
    Some(match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["--settings-generation-lab", "status"] => Ok(Action::Status),
        ["--settings-generation-lab", "fresh", "--offline-confirmed"] => Ok(Action::Fresh),
        ["--settings-generation-lab", "migrate", "--offline-confirmed"] => Ok(Action::Migrate),
        _ => Err("Use status, or fresh/migrate with --offline-confirmed. No data changed."),
    })
}
#[cfg(windows)]
pub fn run(args: &[String]) -> Option<Result<&'static str, String>> {
    let enabled = std::env::var_os("WINCOMMANDER_EXPERIMENTAL_SETTINGS_GENERATION").as_deref() == Some(std::ffi::OsStr::new("1"));
    let action = match parse(args, enabled)? { Ok(action) => action, Err(error) => return Some(Err(error.into())) };
    Some(match action {
        Action::Status => crate::datastore::settings_generation_lab_status(),
        Action::Fresh => crate::datastore::settings_generation_lab_fresh().map(|_| "Protected fresh settings generation prepared."),
        Action::Migrate => crate::datastore::settings_generation_lab_migrate().map(|_| "Protected settings generation prepared; original files retained."),
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    fn args(values: &[&str]) -> Vec<String> { values.iter().map(|value| value.to_string()).collect() }
    #[test]
    fn lab_commands_require_explicit_opt_in_and_offline_acknowledgement() {
        assert_eq!(parse(&args(&["--normal"]), true), None);
        assert!(parse(&args(&["--settings-generation-lab", "status"]), false).unwrap().is_err());
        assert_eq!(parse(&args(&["--settings-generation-lab", "status"]), true).unwrap(), Ok(Action::Status));
        assert!(parse(&args(&["--settings-generation-lab", "fresh"]), true).unwrap().is_err());
        assert!(parse(&args(&["--settings-generation-lab", "migrate", "--offline-confirmed", "extra"]), true).unwrap().is_err());
        assert_eq!(parse(&args(&["--settings-generation-lab", "fresh", "--offline-confirmed"]), true).unwrap(), Ok(Action::Fresh));
        assert_eq!(parse(&args(&["--settings-generation-lab", "migrate", "--offline-confirmed"]), true).unwrap(), Ok(Action::Migrate));
    }
}
