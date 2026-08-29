// SPDX-License-Identifier: AGPL-3.0-or-later
//! Desired-state reconciler loop. A2 owns the implementation after G-1.

/// Phase 3 fills this in — settings reconciler (drift detection, repair).
pub(crate) async fn run() {
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
    }
}
