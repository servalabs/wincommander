#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_indexed_roots_cannot_be_widened_by_an_explicit_scope() {
        assert!(resolve_content_roots(Some(r"V:\Secret".into()), vec![]).unwrap().is_empty());
        assert!(resolve_content_roots(Some(r"V:\Secret".into()), vec![PathBuf::from(r"C:\Public")]).unwrap().is_empty());
    }

    #[test]
    fn query_scope_intersects_current_allowed_roots() {
        let roots = vec![PathBuf::from(r"V:\Allowed")];
        assert_eq!(resolve_content_roots(None, roots.clone()).unwrap(), roots);
        assert_eq!(resolve_content_roots(Some(r"V:\".into()), roots.clone()).unwrap(), roots);
        assert_eq!(resolve_content_roots(Some(r"V:\Allowed\Child".into()), roots.clone()).unwrap(),
            vec![PathBuf::from(r"V:\Allowed\Child")]);
        assert!(resolve_content_roots(Some(r"V:\AllowedOther".into()), roots).unwrap().is_empty());
    }

    #[test]
    fn scope_rejects_empty_and_control_character_requests() {
        assert!(resolve_content_roots(Some("   ".into()), vec![]).is_err());
        assert!(resolve_content_roots(Some("C:\\a\n".into()), vec![]).is_ok());
        assert!(resolve_content_roots(Some("C:\\a\u{0007}b".into()), vec![]).is_err());
    }

    #[test]
    fn query_limits_are_bounded_and_default_to_keyword_search() {
        let default = build_content_query("test".into(), vec![], None, None, None);
        assert_eq!(default.limit, 50);
        assert_eq!(default.offset, 0);
        assert!(default.keyword_only);
        let capped = build_content_query("test".into(), vec![], Some(usize::MAX), Some(usize::MAX), Some(false));
        assert_eq!(capped.limit, 12_000);
        assert_eq!(capped.offset, 10_000);
        assert!(!capped.keyword_only);
    }

    #[test]
    fn default_roots_only_seed_existing_personal_folders() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("Desktop")).unwrap();
        std::fs::create_dir(dir.path().join("Private")).unwrap();
        assert_eq!(defaults::default_roots_from(Some(dir.path().to_string_lossy().into_owned())),
            vec![dir.path().join("Desktop")]);
        assert!(defaults::default_roots_from(None).is_empty());
    }

    #[test]
    fn old_settings_migrate_without_inventing_private_bindings() {
        let old: FileSearchSettings = serde_json::from_str(r#"{"roots":[],"exclusions":[]}"#).unwrap();
        assert!(!old.initialized);
        assert!(old.private_roots.is_empty());
        assert_eq!(old.result_limit, 200);
        let mut settings = old;
        settings.private_roots.push(crate::settings::PrivateSearchRoot {
            path: PathBuf::from(r"V:\Docs"), volume_root: PathBuf::from(r"V:\"),
            relative_path: PathBuf::from("Docs"), volume_id: "opaque-header-id".into(),
        });
        let parsed: FileSearchSettings = serde_json::from_slice(&serde_json::to_vec(&settings).unwrap()).unwrap();
        assert_eq!(parsed.private_roots, settings.private_roots);
    }
}
