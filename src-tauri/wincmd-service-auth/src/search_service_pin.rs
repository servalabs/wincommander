// SPDX-License-Identifier: AGPL-3.0-or-later
//! Experimental service-image pin. This does not itself authorize cleanup.
//! The expected digest must come from the paired release, never from IPC.
use super::*;
use sha2::{Digest, Sha256};
use std::fs::{File, OpenOptions};
use std::io::Read;
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use windows_sys::Win32::Storage::FileSystem::{
    GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_REPARSE_POINT,
    FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ, FILE_SHARE_WRITE,
};
use windows_sys::Win32::System::Threading::GetProcessId;

/// Holds both the verified process and the non-replaceable image and ancestors.
/// Retain this guard until every cleanup mutation has ended.
pub struct PinnedSearchService {
    _peer: VerifiedServicePeer,
    _image: File,
    _ancestors: Vec<File>,
    path: PathBuf,
}

impl PinnedSearchService {
    /// Recheck the authenticated process against SCM; the retained process
    /// handle prevents PID reuse while this guard is alive.
    pub fn running_pid(&self) -> io::Result<u32> {
        let pid = unsafe { GetProcessId(self._peer._process.as_raw_handle() as HANDLE) };
        let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)
            .map_err(|_| denied())?;
        let service = manager
            .open_service(
                wincmd_shared::svc::SVC_WINDOWS_SERVICE_NAME,
                ServiceAccess::QUERY_STATUS | ServiceAccess::QUERY_CONFIG,
            )
            .map_err(|_| denied())?;
        let status = service.query_status().map_err(|_| denied())?;
        require_running_pid(
            status.current_state == ServiceState::Running,
            status.process_id,
            pid,
        )?;
        let config = service.query_config().map_err(|_| denied())?;
        require_service_configuration(
            status.service_type,
            config.service_type,
            config.account_name.as_deref(),
        )?;
        if service_executable(&config.executable_path)? != self.path {
            return Err(denied());
        }
        Ok(pid)
    }
}

/// A release image pinned while SCM reports the service stopped. A privileged
/// coordinator must hold this across the service start and session verification.
/// The coordinator must also exclude SCM reconfiguration for the whole start.
pub struct StoppedSearchServiceImage {
    path: PathBuf,
    image: File,
    ancestors: Vec<File>,
}

/// Prepare a controlled start without starting or stopping any service itself.
/// This proof is experimental and is not authorization for cleanup.
pub fn pin_stopped_search_service(expected: &[u8; 32]) -> io::Result<StoppedSearchServiceImage> {
    let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)
        .map_err(|_| denied())?;
    let service = manager
        .open_service(
            wincmd_shared::svc::SVC_WINDOWS_SERVICE_NAME,
            ServiceAccess::QUERY_STATUS | ServiceAccess::QUERY_CONFIG,
        )
        .map_err(|_| denied())?;
    let status = service.query_status().map_err(|_| denied())?;
    if status.current_state != ServiceState::Stopped {
        return Err(denied());
    }
    let config = service.query_config().map_err(|_| denied())?;
    // Stopped status may not report a service type; inspect the configured type.
    require_service_configuration(
        config.service_type,
        config.service_type,
        config.account_name.as_deref(),
    )?;
    let path = service_executable(&config.executable_path)?;
    if !path.file_name().is_some_and(|name| {
        name.to_string_lossy()
            .eq_ignore_ascii_case("wincommander-svc.exe")
    }) {
        return Err(denied());
    }
    let (image, ancestors) = pin_file(&path, expected)?;
    let again = service.query_config().map_err(|_| denied())?;
    if service.query_status().map_err(|_| denied())?.current_state != ServiceState::Stopped
        || service_executable(&again.executable_path)? != path
    {
        return Err(denied());
    }
    require_service_configuration(
        again.service_type,
        again.service_type,
        again.account_name.as_deref(),
    )?;
    Ok(StoppedSearchServiceImage {
        path,
        image,
        ancestors,
    })
}

impl StoppedSearchServiceImage {
    /// Bind the already-authenticated pipe to the process started while this pin
    /// was held. SCM configuration and older processes still require fencing.
    pub fn bind_started(self, peer: VerifiedServicePeer) -> io::Result<PinnedSearchService> {
        let pid = unsafe { GetProcessId(peer._process.as_raw_handle() as HANDLE) };
        let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)
            .map_err(|_| denied())?;
        let service = manager
            .open_service(
                wincmd_shared::svc::SVC_WINDOWS_SERVICE_NAME,
                ServiceAccess::QUERY_STATUS | ServiceAccess::QUERY_CONFIG,
            )
            .map_err(|_| denied())?;
        let status = service.query_status().map_err(|_| denied())?;
        require_running_pid(
            status.current_state == ServiceState::Running,
            status.process_id,
            pid,
        )?;
        let config = service.query_config().map_err(|_| denied())?;
        require_service_configuration(
            status.service_type,
            config.service_type,
            config.account_name.as_deref(),
        )?;
        if service_executable(&config.executable_path)? != self.path {
            return Err(denied());
        }
        verify_process_image(
            &peer._process,
            &std::fs::canonicalize(&self.path).map_err(|_| denied())?,
        )?;
        let status = service.query_status().map_err(|_| denied())?;
        require_running_pid(
            status.current_state == ServiceState::Running,
            status.process_id,
            pid,
        )?;
        Ok(PinnedSearchService {
            _peer: peer,
            _image: self.image,
            _ancestors: self.ancestors,
            path: self.path,
        })
    }
}

/// Parse the release-provided SHA-256. Missing or malformed input fails closed.
pub fn release_digest(value: Option<&str>) -> io::Result<[u8; 32]> {
    let text = value.ok_or_else(denied)?;
    if text.len() != 64 || !text.is_ascii() {
        return Err(denied());
    }
    let mut digest = [0; 32];
    for (i, byte) in digest.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).map_err(|_| denied())?;
    }
    Ok(digest)
}

fn pin_file(path: &Path, expected: &[u8; 32]) -> io::Result<(File, Vec<File>)> {
    use std::path::Component;
    if !path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
    {
        return Err(denied());
    }
    let mut parents: Vec<_> = path.parent().ok_or_else(denied)?.ancestors().collect();
    parents.reverse();
    let mut pins = Vec::new();
    for parent in parents {
        let file = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(parent)?;
        let meta = file.metadata()?;
        if !meta.is_dir() || meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(denied());
        }
        pins.push(file);
    }
    let mut file = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)?;
    let meta = file.metadata()?;
    if !meta.is_file()
        || meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || meta.len() > 512 * 1024 * 1024
    {
        return Err(denied());
    }
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    if unsafe { GetFileInformationByHandle(file.as_raw_handle() as HANDLE, &mut info) } == 0
        || info.nNumberOfLinks != 1
    {
        return Err(denied());
    }
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    let actual: [u8; 32] = hash.finalize().into();
    if &actual != expected {
        return Err(denied());
    }
    Ok((file, pins))
}

impl VerifiedServicePeer {
    /// Pin the currently registered service image, after authenticating the pipe.
    /// Callers must also verify a signed capability on that same pipe and fence
    /// existing old processes; this method alone is insufficient for cleanup.
    pub fn pin_search_service(self, expected: &[u8; 32]) -> io::Result<PinnedSearchService> {
        let pid = unsafe { GetProcessId(self._process.as_raw_handle() as HANDLE) };
        let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)
            .map_err(|_| denied())?;
        let service = manager
            .open_service(
                wincmd_shared::svc::SVC_WINDOWS_SERVICE_NAME,
                ServiceAccess::QUERY_STATUS | ServiceAccess::QUERY_CONFIG,
            )
            .map_err(|_| denied())?;
        let status = service.query_status().map_err(|_| denied())?;
        require_running_pid(
            status.current_state == ServiceState::Running,
            status.process_id,
            pid,
        )?;
        let config = service.query_config().map_err(|_| denied())?;
        require_service_configuration(
            status.service_type,
            config.service_type,
            config.account_name.as_deref(),
        )?;
        let path = service_executable(&config.executable_path)?;
        if !path.file_name().is_some_and(|name| {
            name.to_string_lossy()
                .eq_ignore_ascii_case("wincommander-svc.exe")
        }) {
            return Err(denied());
        }
        let (image, ancestors) = pin_file(&path, expected)?;
        let canonical = std::fs::canonicalize(&path).map_err(|_| denied())?;
        verify_process_image(&self._process, &canonical)?;
        let again = service.query_config().map_err(|_| denied())?;
        let status = service.query_status().map_err(|_| denied())?;
        require_running_pid(
            status.current_state == ServiceState::Running,
            status.process_id,
            pid,
        )?;
        require_service_configuration(
            status.service_type,
            again.service_type,
            again.account_name.as_deref(),
        )?;
        if service_executable(&again.executable_path)? != path {
            return Err(denied());
        }
        Ok(PinnedSearchService {
            _peer: self,
            _image: image,
            _ancestors: ancestors,
            path,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pinned_image_rejects_wrong_hash_and_prevents_replacement() {
        let root = std::env::temp_dir().join(format!("wincmd-service-pin-{}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("service.exe");
        std::fs::write(&path, b"fixture").unwrap();
        assert!(pin_file(&path, &[0; 32]).is_err());
        let expected: [u8; 32] = Sha256::digest(b"fixture").into();
        let alias = root.join("alias.exe");
        std::fs::hard_link(&path, &alias).unwrap();
        assert!(pin_file(&path, &expected).is_err());
        std::fs::remove_file(alias).unwrap();
        let guard = pin_file(&path, &expected).unwrap();
        assert!(std::fs::write(&path, b"changed").is_err());
        assert!(std::fs::remove_file(&path).is_err());
        assert!(std::fs::rename(&root, root.with_extension("moved")).is_err());
        drop(guard);
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(root).unwrap();
        assert!(release_digest(None).is_err());
        assert!(release_digest(Some("garbage")).is_err());
    }
}
