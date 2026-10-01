// SPDX-License-Identifier: AGPL-3.0-or-later
use crate::vault_access::{with_caller_impersonation, VaultError};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs::OpenOptions,
    io::{Read, Seek, SeekFrom},
    path::Path,
};
use windows_sys::Win32::Foundation::HANDLE;

#[path = "vault_create_files.rs"]
mod files;

const MAX_KEYFILE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_KEYFILES: usize = 64;

pub(super) struct Creation {
    destination: files::Destination,
    stage: files::Stage,
    output_path: String,
    expected_bytes: u64,
}

fn expected_bytes(args: &Value) -> Result<u64, VaultError> {
    let size = args
        .get("HostSizeMB")
        .or_else(|| args.get("SizeMB"))
        .ok_or(VaultError::Validation)?;
    size.as_u64()
        .or_else(|| size.as_str().and_then(|text| text.parse().ok()))
        .filter(|size| *size > 0)
        .and_then(|size| size.checked_mul(1024 * 1024))
        .ok_or(VaultError::Validation)
}

fn keyfile_paths(value: &Value) -> Result<Vec<String>, VaultError> {
    match value {
        Value::Null => Ok(Vec::new()),
        Value::String(text) if text.trim().is_empty() => Ok(Vec::new()),
        Value::String(text) if text.trim_start().starts_with('[') => {
            keyfile_paths(&serde_json::from_str::<Value>(text).map_err(|_| VaultError::Validation)?)
        }
        Value::String(text) => Ok(vec![text.clone()]),
        Value::Array(values) if values.len() <= MAX_KEYFILES => values
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .filter(|text| !text.is_empty())
                    .map(str::to_owned)
                    .ok_or(VaultError::Validation)
            })
            .collect(),
        _ => Err(VaultError::Validation),
    }
}

impl Creation {
    pub(super) fn prepare(args: &mut Value, path: &str, token: HANDLE) -> Result<Self, VaultError> {
        let expected_bytes = expected_bytes(args)?;
        let destination =
            with_caller_impersonation(token, || files::Destination::create(Path::new(path)))?;
        let stage = files::Stage::create()?;
        // Reserve enough for both encrypted copies even when they share a volume.
        check_capacity(
            expected_bytes,
            files::available_space(&stage.path)?,
            with_caller_impersonation(token, || {
                files::available_space(Path::new(path).parent().ok_or(VaultError::Validation)?)
            })?,
        )?;
        let mut creation = Self {
            destination,
            stage,
            output_path: path.into(),
            expected_bytes,
        };
        for (plural, legacy) in [
            ("Keyfiles", "Keyfile"),
            ("OuterKeyfiles", "OuterKeyfile"),
            ("InnerKeyfiles", "InnerKeyfile"),
        ] {
            if args.get(plural).is_some() || args.get(legacy).is_some() {
                let value = args
                    .get(plural)
                    .filter(|value| !value.is_null() && value.as_str() != Some(""))
                    .or_else(|| args.get(legacy))
                    .unwrap_or(&Value::Null);
                let selected = keyfile_paths(value)?;
                let mut staged = Vec::new();
                for path in selected {
                    creation.stage_keyfile(Path::new(&path), token, &mut staged)?;
                }
                args.as_object_mut()
                    .ok_or(VaultError::Validation)?
                    .remove(legacy);
                args[plural] = Value::Array(staged.into_iter().map(Value::String).collect());
            }
        }
        let container = creation.stage.path.join("container.bin");
        creation.stage.owned_files.push(container.clone());
        args["Path"] = Value::String(container.to_string_lossy().into_owned());
        args["TargetSessionId"] = Value::from(0);
        Ok(creation)
    }

    fn stage_keyfile(
        &mut self,
        path: &Path,
        token: HANDLE,
        out: &mut Vec<String>,
    ) -> Result<(), VaultError> {
        use std::os::windows::fs::MetadataExt;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_ATTRIBUTE_HIDDEN, FILE_ATTRIBUTE_REPARSE_POINT,
        };
        let sources = with_caller_impersonation(token, || {
            let _parents = files::pin_parents(path)?;
            let metadata = std::fs::symlink_metadata(path).map_err(|_| VaultError::AclReadback)?;
            if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
                return Err(VaultError::Validation);
            }
            if !metadata.is_dir() {
                return Ok(vec![path.to_path_buf()]);
            }
            let _directory = files::pin_parents(&path.join(".keyfile-probe"))?;
            let mut paths = Vec::new();
            for entry in std::fs::read_dir(path).map_err(|_| VaultError::AclReadback)? {
                let entry = entry.map_err(|_| VaultError::AclReadback)?;
                let metadata =
                    std::fs::symlink_metadata(entry.path()).map_err(|_| VaultError::AclReadback)?;
                if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
                    return Err(VaultError::Validation);
                }
                if metadata.is_file() && metadata.file_attributes() & FILE_ATTRIBUTE_HIDDEN == 0 {
                    paths.push(entry.path());
                }
                if paths.len() > MAX_KEYFILES {
                    return Err(VaultError::Validation);
                }
            }
            if paths.is_empty() {
                return Err(VaultError::Validation);
            }
            Ok(paths)
        })?;
        for source in sources {
            if self.stage.owned_files.len() >= MAX_KEYFILES {
                return Err(VaultError::Validation);
            }
            let (mut input, _parents) =
                with_caller_impersonation(token, || files::read_file(&source))?;
            let length = files::regular(&input)?;
            if length == 0 || length > MAX_KEYFILE_BYTES {
                return Err(VaultError::Validation);
            }
            let target = self
                .stage
                .path
                .join(format!("credential-{}.bin", self.stage.owned_files.len()));
            let mut output = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&target)
                .map_err(|_| VaultError::Persistence)?;
            self.stage.owned_files.push(target.clone());
            if std::io::copy(&mut input, &mut output).map_err(|_| VaultError::Persistence)?
                != length
            {
                return Err(VaultError::ContainerIdentity);
            }
            output.sync_all().map_err(|_| VaultError::Persistence)?;
            out.push(target.to_string_lossy().into_owned());
        }
        Ok(())
    }

    pub(super) fn publish(&mut self, result: &mut Value) -> Result<(), VaultError> {
        let path = self.stage.path.join("container.bin");
        if result.get("path").and_then(Value::as_str) != path.to_str() {
            return Err(VaultError::ContainerIdentity);
        }
        let (mut source, _parents) = files::read_file(&path)?;
        if files::regular(&source)? != self.expected_bytes {
            return Err(VaultError::ContainerIdentity);
        }
        if std::io::copy(&mut source, &mut self.destination.file)
            .map_err(|_| VaultError::Persistence)?
            != self.expected_bytes
        {
            return Err(VaultError::ContainerIdentity);
        }
        self.destination
            .file
            .sync_all()
            .map_err(|_| VaultError::Persistence)?;
        let source_hash = file_hash(&mut source)?;
        if file_hash(&mut self.destination.file)? != source_hash {
            return Err(VaultError::ContainerIdentity);
        }
        result["path"] = Value::String(self.output_path.clone());
        Ok(())
    }

    pub(super) fn commit(&mut self) {
        self.destination.commit();
    }
}

fn file_hash(file: &mut std::fs::File) -> Result<Vec<u8>, VaultError> {
    file.seek(SeekFrom::Start(0))
        .map_err(|_| VaultError::Persistence)?;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|_| VaultError::Persistence)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(digest.finalize().to_vec())
}

fn check_capacity(bytes: u64, staging_free: u64, destination_free: u64) -> Result<(), VaultError> {
    let required = bytes.checked_mul(2).ok_or(VaultError::Validation)?;
    if staging_free < required || destination_free < required {
        return Err(VaultError::Persistence);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn creation_sizes_are_positive_and_bounded_before_formatting() {
        assert_eq!(
            expected_bytes(&serde_json::json!({"SizeMB":32})).unwrap(),
            32 * 1024 * 1024
        );
        for size in [0, u64::MAX] {
            assert!(expected_bytes(&serde_json::json!({"SizeMB":size})).is_err());
        }
        assert!(
            expected_bytes(&serde_json::json!({"HostSizeMB":64,"SizeMB":32})).unwrap()
                == 64 * 1024 * 1024
        );
    }
    #[test]
    fn keyfile_values_preserve_multiple_selections_and_reject_nonpaths() {
        assert_eq!(
            keyfile_paths(&Value::String("[\"C:\\\\a\",\"C:\\\\b\"]".into()))
                .unwrap()
                .len(),
            2
        );
        assert!(keyfile_paths(&serde_json::json!([true])).is_err());
        assert!(keyfile_paths(&serde_json::json!({"Path":"C:\\a"})).is_err());
    }

    #[test]
    fn insufficient_space_or_overflow_fails_before_formatting() {
        assert!(check_capacity(32, 64, 64).is_ok());
        assert!(check_capacity(32, 63, 64).is_err());
        assert!(check_capacity(32, 64, 63).is_err());
        assert!(check_capacity(u64::MAX, u64::MAX, u64::MAX).is_err());
    }
}
