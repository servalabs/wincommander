// SPDX-License-Identifier: AGPL-3.0-or-later
use super::{prepare, private_jobs};
use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrivacyStatus {
    generation: String,
    private_roots: Vec<String>,
    blocked_roots: Vec<String>,
    volumes: Vec<PrivateVolumeStatus>,
    notice: Option<String>,
}

#[derive(Serialize)]
pub struct PrivateVolumeStatus {
    root: String,
    state: String,
    message: String,
}

pub(super) fn snapshot() -> Result<PrivacyStatus, String> {
    let (settings, plan) = prepare(false)?;
    private_jobs::refresh(
        &plan,
        &settings.device_id,
        &settings.app.file_search.exclusions,
        false,
    );
    start_updates();
    let mut volumes: Vec<_> = plan
        .private
        .iter()
        .map(|shard| {
            let (state, message, _) = private_jobs::state(shard);
            PrivateVolumeStatus {
                root: shard.volume.root.to_string_lossy().into_owned(),
                state: state.into(),
                message: message.into(),
            }
        })
        .collect();
    volumes.extend(plan.blocked.iter().map(|root| PrivateVolumeStatus {
        root: root.to_string_lossy().into_owned(),
        state: "unavailable".into(),
        message: "This indexed folder is unavailable or its volume identity changed.".into(),
    }));
    Ok(PrivacyStatus {
        generation: plan.generation,
        private_roots: plan.protected.iter().map(|p| p.to_string_lossy().into_owned()).collect(),
        blocked_roots: plan.blocked.iter().map(|p| p.to_string_lossy().into_owned()).collect(),
        volumes,
        notice: Some("VeraCrypt indexes stay inside their volume. Exclude private drives separately in Everything and Windows Search; WinCommander does not change those providers' databases. Read-only indexes require the same drive letter used when last updated.".into()),
    })
}

fn start_updates() {
    static STARTED: std::sync::Once = std::sync::Once::new();
    STARTED.call_once(|| {
        std::thread::spawn(|| loop {
            std::thread::sleep(std::time::Duration::from_secs(2));
            if let Ok((settings, plan)) = prepare(false) {
                private_jobs::refresh(
                    &plan,
                    &settings.device_id,
                    &settings.app.file_search.exclusions,
                    false,
                );
            } else {
                private_jobs::cancel_all();
            }
        });
    });
}
