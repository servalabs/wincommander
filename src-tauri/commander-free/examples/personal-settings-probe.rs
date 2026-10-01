// SPDX-License-Identifier: AGPL-3.0-or-later
//! Opt-in installed-service verification; never prints preference values or secrets.
#[allow(dead_code)]
#[path = "../src/svc_client.rs"]
mod svc_client;

use serde_json::json;
use wincmd_shared::personal_settings::{
    PersonalSettingsRecord, WritePersonalSettingsRequest, READ_PERSONAL_SETTINGS_VERB,
    WRITE_PERSONAL_SETTINGS_VERB,
};

async fn read() -> Result<PersonalSettingsRecord, String> {
    serde_json::from_value(svc_client::call(READ_PERSONAL_SETTINGS_VERB, json!({})).await?)
        .map_err(|_| "Invalid personal-settings response".into())
}

async fn verify(write: bool) -> Result<(), String> {
    let before = read().await?;
    println!(
        "PASS authenticated own-account read; saved record present: {}",
        before.value.is_some()
    );
    if !write {
        return Ok(());
    }
    let value = before
        .value
        .clone()
        .ok_or("No existing record; probe refuses to create one")?;
    let request = WritePersonalSettingsRequest {
        expected_revision: before.revision,
        value,
        legacy_recovery_required: before.legacy_recovery_required,
    };
    let committed: PersonalSettingsRecord = serde_json::from_value(
        svc_client::call(
            WRITE_PERSONAL_SETTINGS_VERB,
            serde_json::to_value(request).unwrap(),
        )
        .await?,
    )
    .map_err(|_| "Invalid save response")?;
    let after = read().await?;
    if committed.revision != before.revision + 1
        || after.revision != committed.revision
        || after.value != before.value
        || after.legacy_recovery_required != before.legacy_recovery_required
    {
        return Err("Read-back changed during verification; no rollback was attempted".into());
    }
    println!("PASS unchanged-value CAS save and fresh-connection read-back; preferences and encrypted secrets preserved");
    Ok(())
}

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let write = match args.as_slice() {
        [] => false,
        [arg] if arg == "--verify-unchanged-write" => true,
        _ => {
            eprintln!("Usage: personal-settings-probe [--verify-unchanged-write]");
            std::process::exit(2);
        }
    };
    let result = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("probe runtime")
        .block_on(verify(write));
    if let Err(error) = result {
        eprintln!("FAIL: {error}");
        std::process::exit(1);
    }
}
