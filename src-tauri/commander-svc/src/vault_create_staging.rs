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

/// Space is checked before anything is created so a shortfall fails early.
#[derive(Debug)]
pub(super) enum PrepareFailure {
    InsufficientSpace,
    Other(VaultError),
}

impl From<VaultError> for PrepareFailure {
    fn from(error: VaultError) -> Self {
        Self::Other(error)
    }
}

impl PrepareFailure {
    pub(super) fn diagnostic_code(&self) -> &'static str {
        match self {
            Self::InsufficientSpace => "VLT.CREATE.INSUFFICIENT_SPACE",
            Self::Other(_) => "VLT.CREATE.PREPARATION_FAILED",
        }
    }
}

pub(super) struct Creation {
    destination: files::Destination,
    stage: files::Stage,
    output_path: String,
    expected_bytes: u64,
}

#[derive(Debug)]
pub(super) enum PublishFailure {
    BrokerPath,
    SourceOpen,
    SourceSize,
    Copy,
    Flush,
    SourceHash,
    DestinationHash,
    ContentMismatch,
}

impl PublishFailure {
    pub(super) fn diagnostic_code(&self) -> &'static str {
        match self {
            Self::BrokerPath => "VLT.CREATE.STAGE_PATH_FAILED",
            Self::SourceOpen => "VLT.CREATE.STAGE_OPEN_FAILED",
            Self::SourceSize => "VLT.CREATE.STAGE_SIZE_FAILED",
            Self::Copy => "VLT.CREATE.STAGE_COPY_FAILED",
            Self::Flush => "VLT.CREATE.STAGE_FLUSH_FAILED",
            Self::SourceHash => "VLT.CREATE.STAGE_SOURCE_HASH_FAILED",
            Self::DestinationHash => "VLT.CREATE.STAGE_DEST_HASH_FAILED",
            Self::ContentMismatch => "VLT.CREATE.STAGE_CONTENT_MISMATCH",
        }
    }
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
    pub(super) fn prepare(
        args: &mut Value,
        path: &str,
        token: HANDLE,
    ) -> Result<Self, PrepareFailure> {
        let expected_bytes = expected_bytes(args)?;
        let staging_root = std::path::PathBuf::from(
            std::env::var_os("ProgramData").ok_or(VaultError::Persistence)?,
        );
        let (destination_free, destination_volume) = with_caller_impersonation(token, || {
            let parent = Path::new(path).parent().ok_or(VaultError::Validation)?;
            Ok((files::available_space(parent)?, files::volume_id(parent)))
        })?;
        let same_volume = match (files::volume_id(&staging_root), destination_volume) {
            (Some(staging), Some(destination)) => staging == destination,
            // Unknown means assume they share one, the larger requirement.
            _ => true,
        };
        check_capacity(
            expected_bytes,
            files::available_space(&staging_root)?,
            destination_free,
            same_volume,
        )?;
        let destination =
            with_caller_impersonation(token, || files::Destination::create(Path::new(path)))?;
        let stage = files::Stage::create()?;
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

    pub(super) fn publish(&mut self, result: &mut Value) -> Result<(), PublishFailure> {
        let path = self.stage.path.join("container.bin");
        if result.get("path").and_then(Value::as_str) != path.to_str() {
            return Err(PublishFailure::BrokerPath);
        }
        let (mut source, _parents) =
            files::read_file(&path).map_err(|_| PublishFailure::SourceOpen)?;
        if files::regular(&source).map_err(|_| PublishFailure::SourceSize)? != self.expected_bytes {
            return Err(PublishFailure::SourceSize);
        }
        if std::io::copy(&mut source, &mut self.destination.file)
            .map_err(|_| PublishFailure::Copy)?
            != self.expected_bytes
        {
            return Err(PublishFailure::Copy);
        }
        self.destination
            .file
            .sync_all()
            .map_err(|_| PublishFailure::Flush)?;
        let source_hash = file_hash(&mut source).map_err(|_| PublishFailure::SourceHash)?;
        if file_hash(&mut self.destination.file).map_err(|_| PublishFailure::DestinationHash)?
            != source_hash
        {
            return Err(PublishFailure::ContentMismatch);
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

/// The staged container and its published copy coexist: both need `bytes`, on
/// one volume together, or one each when the destination is elsewhere.
fn check_capacity(
    bytes: u64,
    staging_free: u64,
    destination_free: u64,
    same_volume: bool,
) -> Result<(), PrepareFailure> {
    let both = bytes.checked_mul(2).ok_or(VaultError::Validation)?;
    let enough = if same_volume {
        staging_free.min(destination_free) >= both
    } else {
        staging_free >= bytes && destination_free >= bytes
    };
    if enough {
        Ok(())
    } else {
        Err(PrepareFailure::InsufficientSpace)
    }
}

#[cfg(test)]
#[path = "vault_create_staging_tests.rs"]
mod tests;
