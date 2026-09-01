//! Fail-closed local semantic-model admission. No network or caller path.
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub struct SemanticModelManifest {
    pub version: &'static str,
    pub sha256: &'static str,
    pub dimensions: usize,
    pub max_input_chars: usize,
}
pub trait PackagedModelLoader {
    fn packaged_model(&self) -> Option<PathBuf>;
}
pub enum SemanticModelState {
    Unavailable,
    Ready(SemanticModelManifest),
}
pub fn verify_packaged_model(
    loader: &dyn PackagedModelLoader,
    manifest: SemanticModelManifest,
) -> SemanticModelState {
    let Some(path) = loader.packaged_model() else {
        return SemanticModelState::Unavailable;
    };
    let Ok(bytes) = std::fs::read(&path) else {
        return SemanticModelState::Unavailable;
    };
    let hash = hex::encode(Sha256::digest(bytes));
    if hash != manifest.sha256 || manifest.dimensions == 0 || manifest.max_input_chars == 0 {
        return SemanticModelState::Unavailable;
    }
    SemanticModelState::Ready(manifest)
}
pub fn packaged_resource_path(root: &Path) -> PathBuf {
    root.join("models").join("semantic-model.bin")
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fake(Option<PathBuf>);
    impl PackagedModelLoader for Fake {
        fn packaged_model(&self) -> Option<PathBuf> {
            self.0.clone()
        }
    }
    fn manifest(hash: &'static str) -> SemanticModelManifest {
        SemanticModelManifest {
            version: "v1",
            sha256: hash,
            dimensions: 8,
            max_input_chars: 128,
        }
    }
    #[test]
    fn missing_is_unavailable() {
        assert!(matches!(
            verify_packaged_model(&Fake(None), manifest("x")),
            SemanticModelState::Unavailable
        ));
    }
    #[test]
    fn mismatch_and_invalid_bounds_are_unavailable() {
        let path = std::env::temp_dir().join("wc-semantic-test.bin");
        std::fs::write(&path, b"model").unwrap();
        assert!(matches!(
            verify_packaged_model(&Fake(Some(path.clone())), manifest("00")),
            SemanticModelState::Unavailable
        ));
        assert!(matches!(
            verify_packaged_model(
                &Fake(Some(path)),
                SemanticModelManifest {
                    version: "",
                    sha256: "00",
                    dimensions: 0,
                    max_input_chars: 0
                }
            ),
            SemanticModelState::Unavailable
        ));
    }
    #[test]
    fn valid_pinned_asset_is_ready() {
        let root = std::env::temp_dir().join("wc-semantic-fixed-root");
        let path = packaged_resource_path(&root);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"model").unwrap();
        let digest = Box::leak(hex::encode(Sha256::digest(b"model")).into_boxed_str());
        match verify_packaged_model(&Fake(Some(path)), manifest(digest)) {
            SemanticModelState::Ready(m) => {
                assert_eq!(m.version, "v1");
                assert_eq!(m.dimensions, 8)
            }
            _ => panic!("valid pinned model rejected"),
        };
    }
    #[test]
    fn resource_path_is_fixed_under_models() {
        let p = packaged_resource_path(Path::new("C:\\package"));
        assert!(p.ends_with("models\\semantic-model.bin"));
        assert!(!p.to_string_lossy().contains(".."));
    }
}
