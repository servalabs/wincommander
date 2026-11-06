// SPDX-License-Identifier: AGPL-3.0-or-later
use std::path::Path;
const PROTECTED_HANDOFF_MAGIC: &[u8] = b"WCSAFE4\0";

pub(crate) fn read_protected<T: serde::de::DeserializeOwned>(
    path: &Path,
) -> Result<Option<T>, String> {
    use std::io::Read;
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("Safe Copy could not read its protected selection.".into()),
    };
    let mut bytes = Vec::new();
    file.take(4 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Safe Copy could not read its protected selection.".to_string())?;
    if bytes.len() > 4 * 1024 * 1024 || !bytes.starts_with(PROTECTED_HANDOFF_MAGIC) {
        return Err("Safe Copy selection is invalid; use Safe Copy again.".into());
    }
    let plain = zeroize::Zeroizing::new(crate::datastore::user_dpapi_unprotect(
        &bytes[PROTECTED_HANDOFF_MAGIC.len()..],
    )?);
    serde_json::from_slice(&plain)
        .map(Some)
        .map_err(|_| "Safe Copy selection is invalid; use Safe Copy again.".into())
}

pub(crate) fn write_json_atomic<T: serde::Serialize>(
    path: &Path,
    value: &T,
    tmp_name: &str,
) -> Result<(), String> {
    let parent = path.parent().ok_or("safe-clip path has no parent")?;
    std::fs::create_dir_all(parent).map_err(|e| format!("create dir: {e}"))?;
    let bytes =
        zeroize::Zeroizing::new(serde_json::to_vec(value).map_err(|e| format!("serialize: {e}"))?);
    let protected = crate::datastore::user_dpapi_protect(&bytes)?;
    let mut envelope = PROTECTED_HANDOFF_MAGIC.to_vec();
    envelope.extend_from_slice(&protected);
    let tmp = parent.join(tmp_name);
    std::fs::write(&tmp, &envelope).map_err(|e| format!("write protected selection: {e}"))?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("publish protected selection: {e}")
    })
}
