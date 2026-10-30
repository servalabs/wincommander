// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;

#[cfg(windows)]
#[test]
fn project_cleanup_revalidates_plain_artifact_paths_after_preview() {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    let artifact = project.join(".next/cache");
    let documents = project.join("documents");
    fs::create_dir_all(&artifact).unwrap();
    fs::create_dir_all(documents.join("cache")).unwrap();
    fs::write(project.join("package.json"), "{}").unwrap();
    let original = artifact.join("cached");
    fs::write(&original, b"cache").unwrap();
    fs::File::options()
        .write(true)
        .open(&original)
        .unwrap()
        .set_modified(
            std::time::SystemTime::now()
                .checked_sub(Duration::from_secs(172800))
                .unwrap(),
        )
        .unwrap();
    let protected = documents.join("cache/cached");
    fs::write(&protected, b"notes").unwrap();
    let (targets, _) = discover(&project, &AtomicBool::new(false)).unwrap();
    let (_, cached) =
        super::super::scanner::scan_targets(targets, &AtomicBool::new(false)).unwrap();
    assert_eq!(cached.len(), 1);
    assert!(cached[0].requires_plain_path);
    fs::rename(project.join(".next"), project.join("old-next")).unwrap();
    let link = project.join(".next");
    assert!(std::process::Command::new("cmd")
        .args(["/d", "/c", "mklink", "/J"])
        .arg(&link)
        .arg(&documents)
        .output()
        .unwrap()
        .status
        .success());
    let (result, removed) = super::super::mutations::clean_items(cached, &AtomicBool::new(false));
    assert_eq!(result.files_cleaned, 0);
    assert!(removed.is_empty());
    assert_eq!(result.errors.len(), 1);
    assert!(result.errors[0]
        .reason
        .contains("Project artifact path changed"));
    assert_eq!(fs::read(&protected).unwrap(), b"notes");
    assert!(project.join("old-next/cache/cached").exists());
    fs::remove_dir(link).unwrap();
}

#[cfg(windows)]
#[test]
fn refuses_intermediate_artifact_junctions_even_when_the_target_is_inside_the_project() {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    let documents = project.join("documents");
    fs::create_dir_all(documents.join("cache")).unwrap();
    fs::write(project.join("package.json"), "{}").unwrap();
    let protected = documents.join("cache/important");
    fs::write(&protected, b"important documents").unwrap();
    fs::File::options()
        .write(true)
        .open(&protected)
        .unwrap()
        .set_modified(
            std::time::SystemTime::now()
                .checked_sub(Duration::from_secs(172800))
                .unwrap(),
        )
        .unwrap();
    let link = project.join(".next");
    assert!(std::process::Command::new("cmd")
        .args(["/d", "/c", "mklink", "/J"])
        .arg(&link)
        .arg(&documents)
        .output()
        .unwrap()
        .status
        .success());
    let (targets, skipped) = discover(&project, &AtomicBool::new(false)).unwrap();
    assert!(targets.is_empty());
    assert_eq!(skipped, 1);
    let direct = ScanTarget {
        category: "projects".into(),
        label: "Next cache".into(),
        description: None,
        path: link.join("cache"),
        operation: TargetOperation::Delete,
        recommended: false,
        minimum_age: Duration::ZERO,
        containment_root: Some(fs::canonicalize(&project).unwrap()),
        containment_source: Some(project.clone()),
    };
    let (scan, cached) =
        super::super::scanner::scan_targets(vec![direct], &AtomicBool::new(false)).unwrap();
    assert!(cached.is_empty());
    assert_eq!((scan.total_bytes, scan.skipped_targets), (0, 1));
    assert_eq!(fs::read(&protected).unwrap(), b"important documents");
    fs::remove_dir(link).unwrap();
}

#[test]
fn requires_project_markers_and_preserves_arbitrary_build_folders() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join("dist")).unwrap();
    fs::create_dir_all(temp.path().join("target")).unwrap();
    fs::create_dir_all(temp.path().join("node_modules/.cache")).unwrap();
    assert!(artifact_paths(temp.path()).is_empty());
    fs::write(temp.path().join("package.json"), "{}").unwrap();
    let (targets, _) = discover(temp.path(), &AtomicBool::new(false)).unwrap();
    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0].label, "JavaScript project build cache");
    assert!(!targets[0].recommended);
    assert!(targets.iter().all(|target| target.category == "projects"));
}

#[test]
fn cargo_artifacts_require_cache_signature() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir(temp.path().join("target")).unwrap();
    fs::write(temp.path().join("Cargo.toml"), "[package]").unwrap();
    assert!(artifact_paths(temp.path()).is_empty());
    fs::write(
        temp.path().join("target/CACHEDIR.TAG"),
        "Signature: 8a477f597d28d172789f06886806bc55\n",
    )
    .unwrap();
    assert_eq!(artifact_paths(temp.path()).len(), 1);
    fs::write(temp.path().join("target/CACHEDIR.TAG"), "user file").unwrap();
    assert!(artifact_paths(temp.path()).is_empty());
}

#[test]
fn cancellation_produces_no_candidates() {
    let temp = tempfile::tempdir().unwrap();
    assert!(discover(temp.path(), &AtomicBool::new(true))
        .unwrap()
        .0
        .is_empty());
}

#[cfg(windows)]
#[test]
fn rejects_project_junctions_and_caches_redirected_outside_the_chosen_root() {
    fn junction(link: &Path, target: &Path) {
        let output = std::process::Command::new("cmd")
            .args(["/d", "/c", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output()
            .unwrap();
        assert!(output.status.success());
    }
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    let outside = temp.path().join("outside");
    fs::create_dir_all(outside.join("cache")).unwrap();
    let protected = outside.join("cache/user-data");
    fs::write(&protected, b"preserve this outside file").unwrap();
    fs::File::options()
        .write(true)
        .open(&protected)
        .unwrap()
        .set_modified(
            std::time::SystemTime::now()
                .checked_sub(Duration::from_secs(172800))
                .unwrap(),
        )
        .unwrap();
    fs::create_dir(&project).unwrap();
    fs::write(project.join("package.json"), "{}").unwrap();
    junction(&project.join(".next"), &outside);
    let (targets, skipped) = discover(&project, &AtomicBool::new(false)).unwrap();
    assert_eq!(skipped, 1);
    assert!(targets.is_empty());
    let (scan, cached) =
        super::super::scanner::scan_targets(targets, &AtomicBool::new(false)).unwrap();
    assert!(cached.is_empty());
    assert_eq!(scan.total_files, 0);
    assert_eq!(scan.skipped_targets, 0);
    assert!(protected.exists());
    let link = temp.path().join("project-link");
    junction(&link, &project);
    assert!(discover(&link, &AtomicBool::new(false)).is_err());
    fs::remove_dir(link).unwrap();
    fs::remove_dir(project.join(".next")).unwrap();
}
