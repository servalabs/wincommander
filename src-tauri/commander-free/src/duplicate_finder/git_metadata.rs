// SPDX-License-Identifier: AGPL-3.0-or-later
use std::path::Path;

pub(super) fn contains_git_metadata_component(path: &Path) -> bool {
    path.components().any(|part| {
        part.as_os_str()
            .to_string_lossy()
            .eq_ignore_ascii_case(".git")
    })
}

#[cfg(test)]
mod tests {
    use super::contains_git_metadata_component;
    use std::path::Path;

    #[test]
    fn matches_only_literal_case_insensitive_components() {
        let base = Path::new("synthetic");
        assert!(contains_git_metadata_component(
            &base.join(".git").join("objects").join("blob")
        ));
        assert!(contains_git_metadata_component(
            &base.join(".GiT").join("lfs").join("blob")
        ));
        assert!(!contains_git_metadata_component(&base.join(".gitignore")));
        assert!(!contains_git_metadata_component(
            &base.join(".git-backup").join("blob")
        ));
        assert!(!contains_git_metadata_component(
            &base.join("regular").join("blob")
        ));
    }
}
