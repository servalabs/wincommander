// SPDX-License-Identifier: AGPL-3.0-or-later
//! Closed scenario runner for debug-only autonomous Fleet test artifacts.

mod actions;
mod catalog;
mod parser;
mod result;

use std::sync::{Arc, Mutex};

use parser::{parse_request, validate_capability, Request};
use result::{fail, print_json};

pub fn is_invocation(args: &[String]) -> bool {
    matches!(args.first().map(String::as_str), Some("agent-test"))
}

pub fn main(args: Vec<String>) -> i32 {
    let request = match parse_request(&args).and_then(validate_capability) {
        Ok(request) => request,
        Err(error) => return fail("invalid_request", &error),
    };
    run_in_tauri(request)
}

fn run_in_tauri(request: Request) -> i32 {
    let result = Arc::new(Mutex::new(None));
    let result_for_setup = result.clone();
    let mut context = tauri::generate_context!();
    context.config_mut().app.windows.clear();
    context.config_mut().build.dev_url = None;
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(move |app| {
            let app_handle = app.handle().clone();
            let result = result_for_setup.clone();
            tauri::async_runtime::spawn(async move {
                let envelope = actions::execute(app_handle.clone(), &request).await;
                if let Ok(mut slot) = result.lock() {
                    *slot = Some(envelope);
                }
                app_handle.exit(0);
            });
            Ok(())
        });
    if let Err(error) = app.run(context) {
        return fail("runtime_error", &format!("test runtime failed: {error}"));
    }
    match result.lock().ok().and_then(|mut slot| slot.take()) {
        Some(result) => {
            print_json(&result);
            if result.outcome == "passed" {
                0
            } else {
                8
            }
        }
        None => fail("runtime_error", "test runtime exited without a result"),
    }
}

#[cfg(test)]
mod tests {
    use super::catalog::Scenario;
    use super::*;

    #[test]
    fn parser_refuses_file_indirection_and_unknown_scenario() {
        assert!(parse_request(&[
            "agent-test".into(),
            "run".into(),
            "--request".into(),
            "@request.json".into(),
        ])
        .is_err());
        let raw = r#"{"runId":"00000000-0000-4000-8000-000000000001","scenario":"shell","fixtureId":"wc-test-a","deadlineMs":1000,"capability":{}}"#;
        assert!(parse_request(&[
            "agent-test".into(),
            "run".into(),
            "--request".into(),
            raw.into(),
        ])
        .is_err());
    }

    #[test]
    fn fixture_namespace_rejects_paths_and_uppercase() {
        assert!(!parser::valid_fixture_id("C:/temp"));
        assert!(!parser::valid_fixture_id("wc-test-Upper"));
        assert!(parser::valid_fixture_id("wc-test-abc-123"));
    }

    #[test]
    fn scenario_catalog_is_closed_and_safe() {
        for scenario in [
            Scenario::FleetPreflight,
            Scenario::FleetCheckinReadback,
            Scenario::ClipboardGuardSyntheticMarker,
            Scenario::PrivacyShieldStatus,
            Scenario::PrivacyShieldStartStop,
        ] {
            assert_eq!(scenario.command_binding().1, fleet_proto::ActionClass::Safe);
        }
    }
}
