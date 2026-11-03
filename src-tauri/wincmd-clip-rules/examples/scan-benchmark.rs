// SPDX-License-Identifier: AGPL-3.0-or-later
use std::time::Instant;
use wincmd_clip_rules::*;
fn main() {
    let compiled = compile_content_rules(&ContentRulesSubtree::default()).expect("valid catalog");
    let mut text =
        "ordinary prose row = 12345; // this is a benign source code example\n".repeat(1100);
    let suffix = "\nSSN: 123-45-6789\n";
    text.truncate(65_536 - suffix.len());
    text.push_str(suffix);
    for _ in 0..20 {
        std::hint::black_box(compiled.scan_with_budget(
            &text,
            Channel::Clipboard,
            std::time::Duration::from_secs(5),
        ));
    }
    let mut timings = Vec::new();
    for _ in 0..200 {
        let started = Instant::now();
        let report =
            compiled.scan_with_budget(&text, Channel::Clipboard, std::time::Duration::from_secs(5));
        assert!(
            !report.truncated,
            "partial: {} findings, {} spans",
            report.findings.len(),
            report.local_spans.len()
        );
        assert!(report.findings.iter().any(|p| p.detector_id == "us_ssn"));
        timings.push(started.elapsed().as_micros());
    }
    timings.sort_unstable();
    println!(
        "{{\"bytes\":{},\"samples\":200,\"p50_us\":{},\"p95_us\":{},\"max_us\":{}}}",
        text.len(),
        timings[100],
        timings[189],
        timings[199]
    );
}
