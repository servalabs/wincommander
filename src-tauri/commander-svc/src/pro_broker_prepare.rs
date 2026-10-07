// SPDX-License-Identifier: AGPL-3.0-or-later
//! Prepare the fixed engine payload under the service identity after Pro updates.

use sha2::{Digest, Sha256};
use std::io::Read;
use tokio::{
    sync::Mutex,
    time::{timeout_at, Instant},
};
use wincmd_shared::vault_access::{VaultMountReason, VaultPresentation};

static PREPARED_IMAGE: Mutex<Option<String>> = Mutex::const_new(None);

fn installed_digest() -> Result<String, VaultMountReason> {
    let mut file = std::fs::File::open(super::fixed_pro_path())
        .map_err(|_| VaultMountReason::BrokerUnavailable)?;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let size = file
            .read(&mut buffer)
            .map_err(|_| VaultMountReason::BrokerUnavailable)?;
        if size == 0 {
            break;
        }
        digest.update(&buffer[..size]);
    }
    Ok(super::bytes_to_hex(&digest.finalize()))
}

fn cache_matches(prepared: &Option<String>, image: &str) -> bool {
    !image.is_empty() && prepared.as_deref() == Some(image)
}

pub(super) async fn ensure(request_id: u64, deadline: Instant) -> Result<(), VaultMountReason> {
    // The parent retains its image operation lease across preparation and use.
    let mut prepared = timeout_at(deadline, PREPARED_IMAGE.lock())
        .await
        .map_err(|_| VaultMountReason::BrokerUnavailable)?;
    let image = installed_digest()?;
    if cache_matches(&prepared, &image) {
        return Ok(());
    }
    *prepared = None;
    let result = Box::pin(super::vault_call_until(
        super::VaultCall {
            request_id,
            target_session_id: 0,
            caller_sid: "S-1-5-18",
            caller_token: None,
            caller_authentication_id: None,
            presentation: VaultPresentation::Machine,
            feature_id: "vault.broker.prepare_driver",
            args: serde_json::json!({}),
        },
        deadline,
    ))
    .await?;
    if result != serde_json::json!({"prepared":true}) || installed_digest()? != image {
        return Err(VaultMountReason::BrokerReplyRejected);
    }
    *prepared = Some(image);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fresh_or_replaced_pro_requires_privileged_preparation() {
        assert!(!cache_matches(&None, "new"));
        assert!(!cache_matches(&Some("old".into()), "new"));
        assert!(cache_matches(&Some("new".into()), "new"));
        assert!(!cache_matches(&Some(String::new()), ""));
    }
}
