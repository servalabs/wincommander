// SPDX-License-Identifier: AGPL-3.0-or-later
use super::{cache, LargeDiskItem, MIN_LARGE_BYTES};
use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LargeDiskResult {
    pub items: Vec<LargeDiskItem>,
    pub matched_count: usize,
    pub matched_size: u64,
    pub result_limit: usize,
    pub limited: bool,
    pub minimum_size: u64,
    pub scan_id: String,
}

fn threshold_result(
    items: &[LargeDiskItem],
    min: u64,
    limit: usize,
    include_dirs: bool,
    scan_id: String,
) -> LargeDiskResult {
    let matches: Vec<_> = items
        .iter()
        .filter(|item| item.size >= min && (include_dirs || !item.is_dir))
        .collect();
    LargeDiskResult {
        matched_count: matches.len(),
        matched_size: matches
            .iter()
            .fold(0_u64, |sum, item| sum.saturating_add(item.size)),
        limited: matches.len() > limit,
        items: matches.into_iter().take(limit).cloned().collect(),
        result_limit: limit,
        minimum_size: min,
        scan_id,
    }
}

pub async fn get_large_disk_items(
    min_size_bytes: Option<u64>,
    limit: Option<usize>,
    include_dirs: Option<bool>,
) -> Result<LargeDiskResult, String> {
    let guard = cache()
        .lock()
        .map_err(|error| format!("cache lock: {error}"))?;
    let meta = guard
        .meta
        .as_ref()
        .ok_or("Analyze a drive or folder first.")?;
    let min = min_size_bytes.unwrap_or(MIN_LARGE_BYTES);
    if min < MIN_LARGE_BYTES {
        return Err(
            "Minimum size must be at least 16 MiB. Smaller files are not retained by the scan."
                .into(),
        );
    }
    Ok(threshold_result(
        &guard.large_items,
        min,
        limit.unwrap_or(5000).clamp(1, 5000),
        include_dirs.unwrap_or(false),
        meta.scan_id.clone(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn item(id: u64, size: u64, is_dir: bool) -> LargeDiskItem {
        LargeDiskItem {
            entry_id: id,
            name: id.to_string(),
            full_path: id.to_string(),
            size,
            allocated: None,
            is_dir,
            last_modified: String::new(),
            file_count: 0,
            folder_count: 0,
            item_type: String::new(),
            cleanup_hint: String::new(),
            risk: String::new(),
        }
    }
    #[test]
    fn threshold_totals_cover_all_matches_even_when_visible_results_are_limited() {
        let items = vec![
            item(1, 600, true),
            item(2, 300, false),
            item(3, 200, false),
            item(4, 100, false),
        ];
        let result = threshold_result(&items, 200, 1, false, "session".into());
        assert_eq!(
            (
                result.matched_count,
                result.matched_size,
                result.items.len()
            ),
            (2, 500, 1)
        );
        assert!(result.limited);
        assert!(!result.items[0].is_dir);
        let result = threshold_result(&items, 400, 5000, false, "session".into());
        assert_eq!(result.matched_count, 0);
        assert!(!result.limited);
    }
}
