// SPDX-License-Identifier: AGPL-3.0-or-later
fn main() {
    println!("{}",serde_json::to_string(&wincmd_clip_rules::detector_patterns()).expect("serializable static patterns"));
}
