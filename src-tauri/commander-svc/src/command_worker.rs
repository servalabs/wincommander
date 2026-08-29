// SPDX-License-Identifier: AGPL-3.0-or-later
//! Fleet command worker. A1 owns the implementation after G-1.

/// Phase 3 fills this in — command worker (dequeue + execute fleet commands).
pub(crate) async fn run() {
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
    }
}
