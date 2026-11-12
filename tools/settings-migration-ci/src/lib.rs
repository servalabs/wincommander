#![allow(dead_code)]
mod datastore_io;
mod datastore;
mod paths {
    pub const APP_DIR_NAME: &str = "WinCommander";
    use std::path::PathBuf;
    pub fn current_user_install_uses_local_datastore() -> bool { false }
    pub fn datastore_data_dir() -> Result<PathBuf, String> { std::env::var_os("TEST_SETTINGS_ROOT").map(PathBuf::from).ok_or("missing fixture root".into()) }
    pub fn user_data_dir() -> Result<PathBuf, String> { datastore_data_dir() }
}
extern crate self as wincmd_shared;
pub mod diagnostics {
    pub fn seal_diagnostic_record(_: &[u8;32], _: &str, _: &str, _: &[u8], _: [u8;12]) -> Result<String,String> { Err("not part of this harness".into()) }
    pub fn open_diagnostic_record(_: &[u8;32], _: &str, _: &str) -> Option<Vec<u8>> { None }
}
#[cfg(test)] mod integration_fixture {
    #[test] fn production_save_and_load_roundtrip() {
        let temp = tempfile::tempdir().unwrap();
        std::env::set_var("TEST_SETTINGS_ROOT", temp.path());
        let expected = serde_json::json!({"setting": true});
        crate::datastore::save("settings", &expected).unwrap();
        let file = std::fs::read_to_string(temp.path().join("store/settings.dat")).unwrap();
        assert!(file.starts_with("enc:v2:"));
        assert_eq!(crate::datastore::load("settings").unwrap(), expected);
    }
}

extern crate self as ed25519_dalek;
pub struct SigningKey;
pub struct VerifyingKey;
impl SigningKey { pub fn verifying_key(&self)->VerifyingKey { VerifyingKey } }
pub mod wipe_auth { use crate::{SigningKey,VerifyingKey}; pub fn generate_provisioning_keypair()->(SigningKey,VerifyingKey) { (SigningKey,VerifyingKey) } pub fn signing_key_from_bytes(_: &[u8;32])->SigningKey { SigningKey } pub fn signing_key_to_bytes(_: &SigningKey)->[u8;32] { [0;32] } pub fn verifying_key_to_bytes(_: &VerifyingKey)->[u8;32] { [0;32] } }
fn log_message(_: &str, _: &str) {}


#[cfg(all(windows, feature="experimental-settings-generation"))] pub mod settings_generation_lab;
