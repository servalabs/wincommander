use super::*;

fn row(path: &str) -> EsResult {
    EsResult {
        name: path.rsplit('\\').next().unwrap().into(),
        directory: path.rsplit_once('\\').map_or("", |(dir, _)| dir).into(),
        full_path: path.into(),
        size: "1".into(),
        modified: String::new(),
        icon_data: None,
    }
}

fn denied_page(start: usize, count: usize) -> Vec<EsResult> {
    (start..start + count)
        .map(|i| row(&format!(r"C:\private\hidden-{i}.txt")))
        .collect()
}

fn public(row: &EsResult) -> bool {
    row.full_path.starts_with("D:\\")
}

#[test]
fn denied_first_page_does_not_hide_accessible_d_drive_result_on_next_page() {
    let mut pages = VisiblePages::new(50);
    pages.add_page(denied_page(0, 51), public, || true);
    assert!(pages.needs_page());
    assert_eq!(pages.offset, 51);
    pages.add_page(vec![row(r"D:\probe.txt")], public, || true);
    assert!(!pages.needs_page());
    let response = pages.finish("probe".into(), true).unwrap();
    assert_eq!(response.results[0].full_path, r"D:\probe.txt");
    assert_eq!(response.total, 1);
    assert!(!response.has_more);
    assert!(!response.incomplete);
}

#[test]
fn fifty_denied_rows_do_not_hide_accessible_d_drive_row_fifty_one() {
    let mut pages = VisiblePages::new(50);
    let mut rows = denied_page(0, 50);
    rows.push(row(r"D:\probe.txt"));
    pages.add_page(rows, public, || true);
    assert_eq!(pages.results.len(), 1);
    assert!(pages.needs_page());
    pages.add_page(vec![], public, || true);
    let response = pages.finish("probe".into(), true).unwrap();
    assert_eq!(response.results[0].full_path, r"D:\probe.txt");
    assert_eq!(response.total, 1);
    assert!(!response.incomplete);
}

#[test]
fn normal_limit_plus_one_page_proves_more_without_a_second_provider_request() {
    let mut pages = VisiblePages::new(50);
    assert_eq!(pages.page_size, 51);
    pages.add_page(
        (0..51)
            .map(|i| row(&format!(r"D:\public-{i}.txt")))
            .collect(),
        public,
        || true,
    );
    assert!(!pages.needs_page());
    let response = pages.finish("public".into(), true).unwrap();
    assert_eq!(response.total, 50);
    assert!(response.has_more);
    assert!(!response.incomplete);
}

#[test]
fn exhausted_healthy_provider_confirms_zero_matches() {
    let mut pages = VisiblePages::new(50);
    pages.add_page(vec![], |_| panic!("empty page was probed"), || true);
    assert!(!pages.needs_page());
    let response = pages.finish("absent".into(), true).unwrap();
    assert!(response.results.is_empty());
    assert_eq!(response.total, 0);
    assert!(!response.has_more);
    assert!(!response.incomplete);
}

#[test]
fn has_more_requires_an_extra_verified_unique_accessible_result() {
    let mut pages = VisiblePages::new(50);
    pages.add_page(
        (0..51)
            .map(|i| {
                if i == 50 {
                    row(r"C:\private\first.txt")
                } else {
                    row(&format!(r"D:\public-{i}.txt"))
                }
            })
            .collect(),
        public,
        || true,
    );
    assert!(!pages.has_more);
    let mut next = denied_page(0, 49);
    next.push(row(r"d:/public-0.txt"));
    next.push(row(r"D:\extra.txt"));
    pages.add_page(next, public, || true);
    // The first visible extra is enough; denied rows cannot establish has_more.
    let response = pages.finish("public".into(), true).unwrap();
    assert_eq!(response.total, 50);
    assert!(response.has_more);
    assert!(!response.incomplete);
    assert!(response.results.iter().all(public));
}

#[test]
fn private_extra_and_duplicate_do_not_claim_more_matches() {
    let mut pages = VisiblePages::new(50);
    pages.add_page(
        (0..51)
            .map(|i| {
                if i == 50 {
                    row(r"C:\private\first.txt")
                } else {
                    row(&format!(r"D:\public-{i}.txt"))
                }
            })
            .collect(),
        public,
        || true,
    );
    pages.add_page(
        vec![row(r"d:/public-0.txt"), row(r"C:\private\secret.txt")],
        public,
        || true,
    );
    let response = pages.finish("public".into(), true).unwrap();
    assert_eq!(response.total, 50);
    assert!(!response.has_more);
    assert!(!response.incomplete);
}

#[test]
fn scan_bound_with_no_visible_matches_is_incomplete_instead_of_empty_success() {
    let mut pages = VisiblePages::new(50);
    for page in 0..MAX_PAGES {
        assert!(pages.needs_page());
        pages.add_page(denied_page(page * 51, 51), public, || true);
    }
    assert!(!pages.needs_page());
    let response = pages.finish("probe".into(), true).unwrap();
    assert!(response.results.is_empty());
    assert_eq!(response.total, 0);
    assert!(!response.has_more);
    assert!(response.incomplete);
}

#[test]
fn deadline_can_end_before_any_visible_match_without_claiming_zero_matches() {
    let mut pages = VisiblePages::new(50);
    pages.add_page(vec![row(r"D:\not-yet-verified.txt")], public, || false);
    let response = pages.finish("probe".into(), true).unwrap();
    assert!(response.results.is_empty());
    assert!(response.incomplete);
    assert!(!response.has_more);
}

#[test]
fn privacy_generation_change_discards_previously_verified_results() {
    let mut pages = VisiblePages::new(50);
    pages.add_page(vec![row(r"D:\probe.txt")], public, || true);
    let error = pages.finish("probe".into(), false).err().unwrap();
    assert_eq!(error, "Search volume state changed. Run the search again.");
}

#[test]
fn scan_bound_preserves_visible_results_without_reporting_provider_counts() {
    let mut pages = VisiblePages::new(50);
    for page in 0..MAX_PAGES {
        let mut rows = denied_page(page * 51, 51);
        if page == 0 {
            rows[0] = row(r"D:\visible.txt");
        }
        pages.add_page(rows, public, || true);
    }
    let response = pages.finish("probe".into(), true).unwrap();
    assert_eq!(response.total, 1);
    assert_eq!(response.results.len(), 1);
    assert!(response.incomplete);
    assert!(!response.has_more);
}

#[test]
fn viewport_expands_find_limit_before_slicing_the_next_page() {
    let base = vec!["-n".into(), "50".into(), "probe".into()];
    for offset in [0, 50, 100] {
        let args = build_page_args(&base, offset, 50);
        assert_eq!(
            args[0..4],
            [
                "-viewport-offset",
                &offset.to_string(),
                "-viewport-count",
                "50"
            ]
        );
        assert_eq!(args[4..6], ["-n", &(offset + 50).to_string()]);
        assert_eq!(args[6], "probe");
    }
    assert_eq!(base[1], "50");
}

#[test]
fn provider_timeout_before_any_page_preserves_an_explicit_engine_error() {
    let mut pages = VisiblePages::new(50);
    assert_eq!(pages.provider_timed_out().unwrap_err(), PROVIDER_TIMEOUT);
    assert!(!pages.incomplete);
}

#[test]
fn later_provider_timeout_can_return_already_verified_partial_results() {
    let mut pages = VisiblePages::new(50);
    let mut rows = denied_page(0, 51);
    rows[0] = row(r"D:\verified.txt");
    pages.add_page(rows, public, || true);
    pages.provider_timed_out().unwrap();
    let response = pages.finish("probe".into(), true).unwrap();
    assert_eq!(response.total, 1);
    assert!(response.incomplete);
    assert!(!response.has_more);
}

#[tokio::test]
async fn stalled_native_privacy_worker_returns_no_unverified_results() {
    let (release, wait) = std::sync::mpsc::channel::<()>();
    let result = privacy_task(Instant::now() + Duration::from_millis(30), move || {
        wait.recv_timeout(Duration::from_secs(2))
            .map_err(|_| "worker outlived its caller".to_string())?;
        Ok(vec![row(r"D:\unverified.txt")])
    })
    .await;
    let _ = release.send(());
    assert_eq!(result.err().unwrap(), PRIVACY_TIMEOUT);
}

#[tokio::test]
async fn expired_final_privacy_deadline_never_starts_a_native_recheck() {
    let result = privacy_task(
        Instant::now() - Duration::from_millis(1),
        || -> Result<(), String> {
            panic!("expired privacy work must not run");
        },
    )
    .await;
    assert_eq!(result.unwrap_err(), PRIVACY_TIMEOUT);
}
