// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;

#[tokio::test]
#[ignore = "requires an explicitly selected read-only local scan scope"]
async fn live_scope_counts_and_file_types_match_cached_content() {
    let root = std::env::var("WINCOMMANDER_ANALYZER_LIVE_ROOT")
        .expect("Set WINCOMMANDER_ANALYZER_LIVE_ROOT to the folder or drive to analyze");
    let meta = run_disk_scan(root).await.expect("read-only scan failed");
    let types = get_disk_file_types(meta.scan_id.clone(), meta.scan_root.clone())
        .await
        .expect("file type totals unavailable");
    let children = get_disk_children(meta.scan_root.clone())
        .await
        .expect("cached folder content unavailable");
    assert_eq!(
        types
            .categories
            .iter()
            .map(|row| row.file_count)
            .sum::<u64>(),
        meta.file_count
    );
    assert_eq!(
        types
            .categories
            .iter()
            .map(|row| row.logical_size)
            .sum::<u64>(),
        meta.total_size
    );
    assert_eq!(
        children.iter().map(|row| row.size).sum::<u64>(),
        meta.total_size
    );
    assert_eq!(
        children
            .iter()
            .map(|row| if row.is_dir { row.file_count } else { 1 })
            .sum::<u64>(),
        meta.file_count
    );
    assert_eq!(
        children
            .iter()
            .filter(|row| row.is_dir)
            .map(|row| row.folder_count + 1)
            .sum::<u64>(),
        meta.folder_count
    );
    assert_eq!(types.complete, meta.complete);
    println!("files={} folders={} logical_bytes={} complete={} skipped_entries={} skipped_links={} truncated={} cancelled={}", meta.file_count, meta.folder_count, meta.total_size, meta.complete, meta.skipped_entries, meta.skipped_links, meta.truncated, meta.cancelled);
}
