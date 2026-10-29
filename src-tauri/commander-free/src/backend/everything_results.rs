//! Refill visible search pages without exposing provider totals or private rows.
use std::{
    collections::HashSet,
    path::Path,
    time::{Duration, Instant},
};

use super::{
    observe_search_volume, parse_es_output, run_es_with_daemon_retry, search_policy_unchanged,
    EsResponse, EsResult, PublicSearchPolicy, ES_SEARCH_TIMEOUT,
};

const MAX_RAW_ROWS: usize = 20_000;
const MAX_PAGES: usize = 40;
const FINAL_POLICY_BUDGET: Duration = Duration::from_millis(250);
const PROVIDER_TIMEOUT: &str = "Filename search engine is still loading or not responding. Wait for Everything to finish indexing, then retry the search.";
const PRIVACY_TIMEOUT: &str =
    "Search result privacy verification did not finish in time. Run the search again.";

pub(super) async fn search(
    executable: String,
    args: Vec<String>,
    query: String,
    limit: usize,
    mut policy: PublicSearchPolicy,
) -> Result<EsResponse, String> {
    let deadline = Instant::now() + ES_SEARCH_TIMEOUT;
    let traversal_deadline = deadline - FINAL_POLICY_BUDGET;
    let mut pages = VisiblePages::new(limit);
    while pages.needs_page() {
        let Some(remaining) = traversal_deadline.checked_duration_since(Instant::now()) else {
            pages.provider_timed_out()?;
            break;
        };
        let page_args = build_page_args(&args, pages.offset, pages.page_size);
        // The outer deadline also bounds provider health checks and recovery.
        let stdout = match tokio::time::timeout(
            remaining,
            run_es_with_daemon_retry(&executable, &page_args, remaining),
        )
        .await
        {
            Ok(result) => result?,
            Err(_) => {
                pages.provider_timed_out()?;
                break;
            }
        };
        (pages, policy) = privacy_task(traversal_deadline, move || {
            let response = parse_es_output(stdout, String::new())?;
            pages.add_page(
                response.results,
                |row| policy.allows(Path::new(&row.full_path), observe_search_volume),
                || Instant::now() < traversal_deadline,
            );
            Ok::<_, String>((pages, policy))
        })
        .await?;
    }
    privacy_task(deadline, move || {
        pages.finish(query, search_policy_unchanged(&policy))
    })
    .await
}

fn build_page_args(base: &[String], offset: usize, page_size: usize) -> Vec<String> {
    let mut args = vec![
        "-viewport-offset".into(),
        offset.to_string(),
        "-viewport-count".into(),
        page_size.to_string(),
    ];
    let mut base = base.to_vec();
    // ES applies its find limit before slicing the output viewport.
    base[1] = offset.saturating_add(page_size).to_string();
    args.extend(base);
    args
}

async fn privacy_task<T: Send + 'static>(
    deadline: Instant,
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let remaining = deadline
        .checked_duration_since(Instant::now())
        .filter(|duration| !duration.is_zero())
        .ok_or_else(|| PRIVACY_TIMEOUT.to_string())?;
    let mut task = tokio::task::spawn_blocking(work);
    match tokio::time::timeout(remaining, &mut task).await {
        Ok(result) => result.map_err(|_| "Search result privacy check failed.".to_string())?,
        Err(_) => {
            task.abort();
            Err(PRIVACY_TIMEOUT.into())
        }
    }
}

struct VisiblePages {
    limit: usize,
    page_size: usize,
    offset: usize,
    page_count: usize,
    results: Vec<EsResult>,
    seen: HashSet<String>,
    exhausted: bool,
    has_more: bool,
    incomplete: bool,
}

impl VisiblePages {
    fn new(limit: usize) -> Self {
        Self {
            limit,
            page_size: limit.saturating_add(1).clamp(50, 500),
            offset: 0,
            page_count: 0,
            results: Vec::new(),
            seen: HashSet::new(),
            exhausted: false,
            has_more: false,
            incomplete: false,
        }
    }

    fn needs_page(&self) -> bool {
        !self.exhausted && !self.has_more && !self.incomplete
    }

    fn provider_timed_out(&mut self) -> Result<(), String> {
        if self.page_count == 0 {
            return Err(PROVIDER_TIMEOUT.into());
        }
        self.incomplete = true;
        Ok(())
    }

    fn add_page(
        &mut self,
        rows: Vec<EsResult>,
        mut allows: impl FnMut(&EsResult) -> bool,
        mut has_time: impl FnMut() -> bool,
    ) {
        let raw_count = rows.len();
        self.offset += raw_count;
        self.page_count += 1;
        self.exhausted = raw_count < self.page_size;
        for row in rows {
            if !has_time() {
                self.incomplete = true;
                break;
            }
            let key = row.full_path.replace('/', "\\").to_lowercase();
            if !self.seen.insert(key) || !allows(&row) {
                continue;
            }
            if self.results.len() == self.limit {
                self.has_more = true;
                break;
            }
            self.results.push(row);
        }
        if !self.exhausted
            && !self.has_more
            && (self.offset >= MAX_RAW_ROWS || self.page_count >= MAX_PAGES)
        {
            self.incomplete = true;
        }
    }

    fn finish(self, query: String, policy_unchanged: bool) -> Result<EsResponse, String> {
        if !policy_unchanged {
            return Err("Search volume state changed. Run the search again.".into());
        }
        Ok(EsResponse {
            total: self.results.len(),
            results: self.results,
            query,
            has_more: self.has_more,
            incomplete: self.incomplete,
        })
    }
}

#[cfg(test)]
#[path = "everything_results_tests.rs"]
mod tests;
