// SPDX-License-Identifier: AGPL-3.0-or-later
//! SYSTEM-owned vault-access policy.  The request is only intent; resolved
//! principals, file identity, and ACL plans never cross the pipe boundary.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use wincmd_shared::vault_access::{
    VaultAccess, VaultAccessEntry, VaultAccessPolicy, VaultAuthorizeMountResponse,
    VaultEntryResult, VaultEntryStatus, VaultMountDenial, VaultPolicyStatus, VaultPresentation,
    VaultValidationState, VAULT_ACCESS_SCHEMA_VERSION,
};

const POLICY_FILE: &str = "vault-access-v1.json";
const MAX_ENTRIES: usize = 64;
const MAX_GRANTS: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VaultError {
    Validation,
    VersionConflict,
    PrincipalResolution,
    ContainerIdentity,
    AclApply,
    AclReadback,
    Persistence,
}

pub trait VaultFs: Send + Sync {
    fn read(&self, path: &Path) -> std::io::Result<Vec<u8>>;
    fn atomic_write(&self, path: &Path, bytes: &[u8]) -> std::io::Result<()>;
    fn stable_file_identity(&self, path: &Path) -> Result<String, VaultError>;
    fn validate_dedicated_parent(&self, parent: &Path, container: &Path) -> Result<(), VaultError>;
}

pub trait PrincipalResolver: Send + Sync {
    fn resolve_sid(&self, name: &str) -> Result<String, VaultError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedGrant {
    pub sid: String,
    pub access: VaultAccess,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VaultAclPlan {
    pub parent: PathBuf,
    pub container: PathBuf,
    pub grants: Vec<ResolvedGrant>,
}

#[derive(Debug, Clone)]
pub struct AclSnapshot {
    pub path: PathBuf,
    pub descriptor: Vec<u8>,
    pub dacl_protected: bool,
}

/// An implementation must leave neither a broad inherited DACL nor an
/// unverified ACL behind.  A failed implementation returns a bounded error;
/// it must not report a successful policy apply.
pub trait AclApplier: Send + Sync {
    fn apply_and_verify(&self, plan: &VaultAclPlan) -> Result<(), VaultError>;
    fn snapshot(&self, plan: &VaultAclPlan) -> Result<Vec<AclSnapshot>, VaultError>;
    fn restore(&self, snapshots: &[AclSnapshot]) -> Result<(), VaultError>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PersistedPolicy {
    policy: VaultAccessPolicy,
    resolved: Vec<ResolvedEntry>,
    applied_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ResolvedEntry {
    id: String,
    identity: String,
    grants: Vec<ResolvedGrantRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ResolvedGrantRecord {
    sid: String,
    access: VaultAccess,
}

struct State {
    active: Option<PersistedPolicy>,
    status: VaultPolicyStatus,
}

pub struct VaultAccessStore {
    fs: Box<dyn VaultFs>,
    principals: Box<dyn PrincipalResolver>,
    acls: Box<dyn AclApplier>,
    path: PathBuf,
    state: Mutex<State>,
}

impl VaultAccessStore {
    pub fn open(
        fs: Box<dyn VaultFs>,
        principals: Box<dyn PrincipalResolver>,
        acls: Box<dyn AclApplier>,
        dir: PathBuf,
    ) -> Self {
        Self {
            fs,
            principals,
            acls,
            path: dir.join(POLICY_FILE),
            state: Mutex::new(State {
                active: None,
                status: empty_status(),
            }),
        }
    }

    pub fn load_at_startup(&self) {
        let loaded: Option<PersistedPolicy> = self
            .fs
            .read(&self.path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok());
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        match loaded {
            Some(policy) if policy.policy.schema_version == VAULT_ACCESS_SCHEMA_VERSION => {
                // A restart has no proof that a third party did not alter the
                // host/container DACL while svc was down.  Keep the durable
                // policy for inspection, but deny mount eligibility until an
                // admin performs a fresh apply/read-back.
                state.status = status_for(
                    &policy,
                    VaultValidationState::Degraded,
                    VaultEntryResult::AclReadbackFailed,
                );
                state.active = Some(policy);
            }
            Some(_) => state.status.validation_state = VaultValidationState::Degraded,
            None => {}
        }
    }

    pub fn status(&self) -> VaultPolicyStatus {
        self.state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .status
            .clone()
    }

    pub fn policy(&self) -> Option<VaultAccessPolicy> {
        self.state
            .lock()
            .ok()?
            .active
            .as_ref()
            .map(|p| p.policy.clone())
    }

    pub fn apply(
        &self,
        policy: VaultAccessPolicy,
        applied_at: i64,
    ) -> Result<VaultPolicyStatus, VaultError> {
        let mut state = self.state.lock().map_err(|_| VaultError::Persistence)?;
        let previous = state.active.as_ref().map(|p| p.policy.version).unwrap_or(0);
        if policy.expected_previous_version != previous
            || policy.version != previous.saturating_add(1)
        {
            return Err(VaultError::VersionConflict);
        }
        validate_policy(&policy)?;
        let resolved = self.resolve_and_plan(&policy)?;
        let mut snapshots = Vec::new();
        for (_, plan) in &resolved {
            snapshots.extend(self.acls.snapshot(plan)?);
        }
        for (_, plan) in &resolved {
            if let Err(error) = self.acls.apply_and_verify(plan) {
                // JSON replacement has not happened.  Restore every target
                // already touched before returning the bounded failure.
                self.rollback_after_apply(&mut state, &snapshots);
                return Err(error);
            }
        }
        let persisted = PersistedPolicy {
            policy,
            resolved: resolved
                .into_iter()
                .map(|(entry, plan)| ResolvedEntry {
                    id: entry.id.clone(),
                    identity: entry.container_identity.clone().unwrap_or_default(),
                    grants: plan
                        .grants
                        .into_iter()
                        .map(|g| ResolvedGrantRecord {
                            sid: g.sid,
                            access: g.access,
                        })
                        .collect(),
                })
                .collect(),
            applied_at,
        };
        let bytes = match serde_json::to_vec(&persisted) {
            Ok(bytes) => bytes,
            Err(_) => {
                self.rollback_after_apply(&mut state, &snapshots);
                return Err(VaultError::Persistence);
            }
        };
        if self.fs.atomic_write(&self.path, &bytes).is_err() {
            self.rollback_after_apply(&mut state, &snapshots);
            return Err(VaultError::Persistence);
        }
        // Host/container ACLs are proven here.  Mounted-root enforcement is
        // deliberately pending until a broker can make mount+ACL atomic.
        let status = status_for(
            &persisted,
            VaultValidationState::Current,
            VaultEntryResult::PendingMountBroker,
        );
        state.active = Some(persisted);
        state.status = status.clone();
        Ok(status)
    }

    /// Authorization is caller-token based.  Eligibility intentionally never
    /// becomes a launch authorization until a closed mount broker exists.
    pub fn authorize_mount(
        &self,
        entry_id: &str,
        caller_sids: &[String],
    ) -> VaultAuthorizeMountResponse {
        let state = match self.state.lock() {
            Ok(s) => s,
            Err(_) => return denied(VaultMountDenial::NotAuthorized),
        };
        if state.status.validation_state != VaultValidationState::Current {
            return denied(VaultMountDenial::NotAuthorized);
        }
        let Some(active) = state.active.as_ref() else {
            return denied(VaultMountDenial::NotAuthorized);
        };
        let Some(entry) = active
            .policy
            .entries
            .iter()
            .find(|entry| entry.id == entry_id)
        else {
            return denied(VaultMountDenial::NotAuthorized);
        };
        let Some(resolved) = active
            .resolved
            .iter()
            .find(|resolved| resolved.id == entry.id)
        else {
            return denied(VaultMountDenial::NotAuthorized);
        };
        if self
            .fs
            .stable_file_identity(Path::new(&entry.container_path))
            .map(|identity| identity == resolved.identity)
            .unwrap_or(false)
            == false
        {
            return denied(VaultMountDenial::NotAuthorized);
        }
        let allowed = resolved
            .grants
            .iter()
            .filter(|grant| caller_sids.iter().any(|sid| sid == &grant.sid))
            .map(|grant| grant.access)
            .max_by_key(|access| matches!(access, VaultAccess::Write));
        match allowed {
            Some(access) => VaultAuthorizeMountResponse {
                allowed: true,
                launch_ready: false,
                denial_reason: Some(VaultMountDenial::MountBrokerUnavailable),
                mode: Some(access),
                presentation: Some(entry.mount.presentation),
                preferred_letter: entry.mount.preferred_letter.clone(),
            },
            None => denied(VaultMountDenial::NotAuthorized),
        }
    }

    fn resolve_and_plan(
        &self,
        policy: &VaultAccessPolicy,
    ) -> Result<Vec<(VaultAccessEntry, VaultAclPlan)>, VaultError> {
        policy
            .entries
            .iter()
            .map(|entry| {
                let container = PathBuf::from(&entry.container_path);
                let identity = self.fs.stable_file_identity(&container)?;
                if let Some(expected) = &entry.container_identity {
                    if expected != &identity {
                        return Err(VaultError::ContainerIdentity);
                    }
                }
                // The owner is always an explicit Modify principal.  This avoids
                // relying on incidental Builtin Administrators membership and
                // preserves correct behaviour when the owner is a non-admin.
                let mut grants = vec![ResolvedGrant {
                    sid: self.principals.resolve_sid(&entry.owner_account)?,
                    access: VaultAccess::Write,
                }];
                for grant in &entry.grants {
                    let sid = self.principals.resolve_sid(&grant.principal_name)?;
                    if let Some(existing) = grants.iter_mut().find(|existing| existing.sid == sid) {
                        if grant.access == VaultAccess::Write {
                            existing.access = VaultAccess::Write;
                        }
                    } else {
                        grants.push(ResolvedGrant {
                            sid,
                            access: grant.access,
                        });
                    }
                }
                let mut entry = entry.clone();
                entry.container_identity = Some(identity);
                let parent = container
                    .parent()
                    .ok_or(VaultError::Validation)?
                    .to_path_buf();
                self.fs.validate_dedicated_parent(&parent, &container)?;
                Ok((
                    entry,
                    VaultAclPlan {
                        parent,
                        container,
                        grants,
                    },
                ))
            })
            .collect()
    }

    fn rollback_after_apply(&self, state: &mut State, snapshots: &[AclSnapshot]) {
        let result = self.acls.restore(snapshots);
        state.status = match state.active.as_ref() {
            Some(active) => status_for(
                active,
                VaultValidationState::Degraded,
                if result.is_ok() {
                    VaultEntryResult::AclApplyFailed
                } else {
                    VaultEntryResult::AclReadbackFailed
                },
            ),
            None => VaultPolicyStatus {
                validation_state: VaultValidationState::Degraded,
                ..empty_status()
            },
        };
    }
}

#[cfg(windows)]
pub struct WindowsVaultFs;

#[cfg(windows)]
impl VaultFs for WindowsVaultFs {
    fn read(&self, path: &Path) -> std::io::Result<Vec<u8>> {
        std::fs::read(path)
    }
    fn atomic_write(&self, path: &Path, bytes: &[u8]) -> std::io::Result<()> {
        let parent = path
            .parent()
            .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
        std::fs::create_dir_all(parent)?;
        let temp = parent.join(format!(".{}-{}.tmp", POLICY_FILE, std::process::id()));
        std::fs::write(&temp, bytes)?;
        std::fs::rename(temp, path)
    }
    fn stable_file_identity(&self, path: &Path) -> Result<String, VaultError> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{
            GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
        };
        let file = std::fs::File::open(path).map_err(|_| VaultError::ContainerIdentity)?;
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        if unsafe { GetFileInformationByHandle(file.as_raw_handle() as _, &mut info) } == 0 {
            return Err(VaultError::ContainerIdentity);
        }
        Ok(format!(
            "v:{}:i:{}",
            info.dwVolumeSerialNumber,
            ((info.nFileIndexHigh as u64) << 32) | info.nFileIndexLow as u64
        ))
    }
    fn validate_dedicated_parent(&self, parent: &Path, container: &Path) -> Result<(), VaultError> {
        let parent = std::fs::canonicalize(parent).map_err(|_| VaultError::Validation)?;
        let container = std::fs::canonicalize(container).map_err(|_| VaultError::Validation)?;
        if parent.parent().is_none() {
            return Err(VaultError::Validation);
        }
        let mut entries = std::fs::read_dir(&parent).map_err(|_| VaultError::Validation)?;
        let only_entry = entries
            .next()
            .transpose()
            .map_err(|_| VaultError::Validation)?
            .ok_or(VaultError::Validation)?
            .path();
        if entries.next().is_some()
            || std::fs::canonicalize(only_entry).map_err(|_| VaultError::Validation)? != container
        {
            return Err(VaultError::Validation);
        }
        Ok(())
    }
}

#[cfg(windows)]
pub struct WindowsPrincipalResolver;

#[cfg(windows)]
impl PrincipalResolver for WindowsPrincipalResolver {
    fn resolve_sid(&self, name: &str) -> Result<String, VaultError> {
        lookup_account_sid(name)
    }
}

#[cfg(windows)]
fn wide(value: &std::ffi::OsStr) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    value.encode_wide().chain(std::iter::once(0)).collect()
}

#[cfg(windows)]
fn lookup_account_sid(name: &str) -> Result<String, VaultError> {
    use windows_sys::Win32::Security::{LookupAccountNameW, PSID, SID_NAME_USE};
    let name = wide(std::ffi::OsStr::new(name));
    let mut sid_len = 0u32;
    let mut domain_len = 0u32;
    let mut use_type = 0i32 as SID_NAME_USE;
    unsafe {
        LookupAccountNameW(
            std::ptr::null(),
            name.as_ptr(),
            std::ptr::null_mut(),
            &mut sid_len,
            std::ptr::null_mut(),
            &mut domain_len,
            &mut use_type,
        );
    }
    if sid_len == 0 {
        return Err(VaultError::PrincipalResolution);
    }
    let mut sid = vec![0u8; sid_len as usize];
    let mut domain = vec![0u16; domain_len as usize + 1];
    if unsafe {
        LookupAccountNameW(
            std::ptr::null(),
            name.as_ptr(),
            sid.as_mut_ptr() as PSID,
            &mut sid_len,
            domain.as_mut_ptr(),
            &mut domain_len,
            &mut use_type,
        )
    } == 0
    {
        return Err(VaultError::PrincipalResolution);
    }
    sid_to_string(sid.as_mut_ptr() as PSID).ok_or(VaultError::PrincipalResolution)
}

#[cfg(windows)]
fn sid_to_string(sid: windows_sys::Win32::Security::PSID) -> Option<String> {
    use windows_sys::Win32::{
        Foundation::LocalFree, Security::Authorization::ConvertSidToStringSidW,
    };
    let mut raw = std::ptr::null_mut();
    if unsafe { ConvertSidToStringSidW(sid, &mut raw) } == 0 {
        return None;
    }
    let len = unsafe { (0..).take_while(|i| *raw.add(*i) != 0).count() };
    let result = String::from_utf16(unsafe { std::slice::from_raw_parts(raw, len) }).ok();
    unsafe {
        LocalFree(raw as _);
    }
    result
}

#[cfg(windows)]
pub struct WindowsAclApplier;

#[cfg(windows)]
impl AclApplier for WindowsAclApplier {
    fn apply_and_verify(&self, plan: &VaultAclPlan) -> Result<(), VaultError> {
        apply_one_acl(&plan.parent, &plan.grants)?;
        apply_one_acl(&plan.container, &plan.grants)?;
        Ok(())
    }
    fn snapshot(&self, plan: &VaultAclPlan) -> Result<Vec<AclSnapshot>, VaultError> {
        [plan.parent.as_path(), plan.container.as_path()]
            .into_iter()
            .map(snapshot_one_acl)
            .collect()
    }
    fn restore(&self, snapshots: &[AclSnapshot]) -> Result<(), VaultError> {
        for snapshot in snapshots {
            restore_one_acl(snapshot)?;
        }
        Ok(())
    }
}

#[cfg(windows)]
fn snapshot_one_acl(path: &Path) -> Result<AclSnapshot, VaultError> {
    use windows_sys::Win32::Foundation::{LocalFree, ERROR_SUCCESS};
    use windows_sys::Win32::Security::Authorization::{GetNamedSecurityInfoW, SE_FILE_OBJECT};
    use windows_sys::Win32::Security::{
        GetSecurityDescriptorControl, GetSecurityDescriptorLength, DACL_SECURITY_INFORMATION,
        SE_DACL_PROTECTED,
    };
    unsafe {
        let name = wide(path.as_os_str());
        let mut descriptor = std::ptr::null_mut();
        if GetNamedSecurityInfoW(
            name.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut descriptor,
        ) != ERROR_SUCCESS
            || descriptor.is_null()
        {
            return Err(VaultError::AclApply);
        }
        let length = GetSecurityDescriptorLength(descriptor) as usize;
        let bytes = std::slice::from_raw_parts(descriptor as *const u8, length).to_vec();
        let mut control = 0u16;
        let mut revision = 0u32;
        let dacl_protected = GetSecurityDescriptorControl(descriptor, &mut control, &mut revision)
            != 0
            && (control & SE_DACL_PROTECTED) != 0;
        LocalFree(descriptor as _);
        Ok(AclSnapshot {
            path: path.to_path_buf(),
            descriptor: bytes,
            dacl_protected,
        })
    }
}

#[cfg(windows)]
fn restore_one_acl(snapshot: &AclSnapshot) -> Result<(), VaultError> {
    use windows_sys::Win32::Security::{
        SetFileSecurityW, DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION,
        PSECURITY_DESCRIPTOR, UNPROTECTED_DACL_SECURITY_INFORMATION,
    };
    let name = wide(snapshot.path.as_os_str());
    let info = DACL_SECURITY_INFORMATION
        | if snapshot.dacl_protected {
            PROTECTED_DACL_SECURITY_INFORMATION
        } else {
            UNPROTECTED_DACL_SECURITY_INFORMATION
        };
    if unsafe {
        SetFileSecurityW(
            name.as_ptr(),
            info,
            snapshot.descriptor.as_ptr() as PSECURITY_DESCRIPTOR,
        )
    } == 0
    {
        Err(VaultError::AclApply)
    } else {
        Ok(())
    }
}

#[cfg(windows)]
fn apply_one_acl(path: &Path, grants: &[ResolvedGrant]) -> Result<(), VaultError> {
    use windows_sys::Win32::Foundation::{LocalFree, ERROR_SUCCESS};
    use windows_sys::Win32::Security::Authorization::{
        ConvertStringSidToSidW, GetNamedSecurityInfoW, SetNamedSecurityInfoW, SE_FILE_OBJECT,
    };
    use windows_sys::Win32::Security::{
        AddAccessAllowedAce, AllocateAndInitializeSid, EqualSid, GetAce,
        GetSecurityDescriptorControl, InitializeAcl, ACL, ACL_REVISION, DACL_SECURITY_INFORMATION,
        PROTECTED_DACL_SECURITY_INFORMATION, PSID, SECURITY_NT_AUTHORITY, SE_DACL_PROTECTED,
    };
    use windows_sys::Win32::System::SystemServices::{
        DOMAIN_ALIAS_RID_ADMINS, SECURITY_BUILTIN_DOMAIN_RID,
    };
    const FULL: u32 = 0x001F_01FF;
    const MODIFY: u32 = 0x0013_01BF;
    const READ_EXECUTE: u32 = 0x0012_00A9;
    unsafe {
        let mut system: PSID = std::ptr::null_mut();
        let mut admin: PSID = std::ptr::null_mut();
        let auth = SECURITY_NT_AUTHORITY;
        if AllocateAndInitializeSid(&auth, 1, 18, 0, 0, 0, 0, 0, 0, 0, &mut system) == 0
            || AllocateAndInitializeSid(
                &auth,
                2,
                SECURITY_BUILTIN_DOMAIN_RID as u32,
                DOMAIN_ALIAS_RID_ADMINS as u32,
                0,
                0,
                0,
                0,
                0,
                0,
                &mut admin,
            ) == 0
        {
            return Err(VaultError::AclApply);
        }
        let mut allocated = vec![system, admin];
        for grant in grants {
            let text = wide(std::ffi::OsStr::new(&grant.sid));
            let mut sid: PSID = std::ptr::null_mut();
            if ConvertStringSidToSidW(text.as_ptr(), &mut sid) == 0 {
                release_acl_sids(&allocated);
                return Err(VaultError::AclApply);
            }
            allocated.push(sid);
        }
        let mut buffer: Vec<u64> = vec![0; 256 + grants.len() * 32];
        let acl = buffer.as_mut_ptr() as *mut ACL;
        if InitializeAcl(acl, (buffer.len() * 8) as u32, ACL_REVISION) == 0
            || AddAccessAllowedAce(acl, ACL_REVISION, FULL, system) == 0
            || AddAccessAllowedAce(acl, ACL_REVISION, FULL, admin) == 0
        {
            release_acl_sids(&allocated);
            return Err(VaultError::AclApply);
        }
        for (grant, sid) in grants.iter().zip(allocated.iter().skip(2)) {
            let mask = if grant.access == VaultAccess::Write {
                MODIFY
            } else {
                READ_EXECUTE
            };
            if AddAccessAllowedAce(acl, ACL_REVISION, mask, *sid) == 0 {
                release_acl_sids(&allocated);
                return Err(VaultError::AclApply);
            }
        }
        let name = wide(path.as_os_str());
        if SetNamedSecurityInfoW(
            name.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            acl,
            std::ptr::null(),
        ) != ERROR_SUCCESS
        {
            release_acl_sids(&allocated);
            return Err(VaultError::AclApply);
        }
        let mut returned_acl = std::ptr::null_mut();
        let mut descriptor = std::ptr::null_mut();
        let outcome = GetNamedSecurityInfoW(
            name.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut returned_acl,
            std::ptr::null_mut(),
            &mut descriptor,
        );
        let expected: Vec<(PSID, u32)> = std::iter::once((system, FULL))
            .chain(std::iter::once((admin, FULL)))
            .chain(
                grants
                    .iter()
                    .zip(allocated.iter().skip(2))
                    .map(|(grant, sid)| {
                        (
                            *sid,
                            if grant.access == VaultAccess::Write {
                                MODIFY
                            } else {
                                READ_EXECUTE
                            },
                        )
                    }),
            )
            .collect();
        let mut control = 0u16;
        let mut revision = 0u32;
        let matches = outcome == ERROR_SUCCESS
            && !returned_acl.is_null()
            && (*returned_acl).AceCount as usize == expected.len()
            && !descriptor.is_null()
            && GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) != 0
            && (control & SE_DACL_PROTECTED) != 0
            && expected
                .iter()
                .enumerate()
                .all(|(index, (expected_sid, expected_mask))| {
                    let mut ace = std::ptr::null_mut();
                    if GetAce(returned_acl, index as u32, &mut ace) == 0 || *(ace as *const u8) != 0
                    {
                        return false;
                    }
                    let mask = *((ace as *const u8).add(4) as *const u32);
                    let sid = (ace as *const u8).add(8) as PSID;
                    mask == *expected_mask && EqualSid(sid, *expected_sid) != 0
                });
        if !descriptor.is_null() {
            LocalFree(descriptor as _);
        }
        release_acl_sids(&allocated);
        if matches {
            Ok(())
        } else {
            Err(VaultError::AclReadback)
        }
    }
}

#[cfg(windows)]
unsafe fn release_acl_sids(sids: &[windows_sys::Win32::Security::PSID]) {
    use windows_sys::Win32::{Foundation::LocalFree, Security::FreeSid};
    for (index, sid) in sids.iter().enumerate() {
        if index < 2 {
            FreeSid(*sid);
        } else {
            LocalFree(*sid as _);
        }
    }
}

/// Check the captured pipe client's process token against each stored SID.
/// It intentionally accepts a PID, never a renderer-supplied account name.
#[cfg(windows)]
pub fn authorize_mount_for_process(
    store: &VaultAccessStore,
    entry_id: &str,
    pid: u32,
) -> VaultAuthorizeMountResponse {
    use windows_sys::Win32::Foundation::{CloseHandle, LocalFree};
    use windows_sys::Win32::Security::Authorization::ConvertStringSidToSidW;
    use windows_sys::Win32::Security::{CheckTokenMembership, PSID, TOKEN_QUERY};
    use windows_sys::Win32::System::Threading::{
        OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    let sids = {
        let state = match store.state.lock() {
            Ok(s) => s,
            Err(_) => return denied(VaultMountDenial::NotAuthorized),
        };
        let Some(active) = state.active.as_ref() else {
            return denied(VaultMountDenial::NotAuthorized);
        };
        let Some(resolved) = active.resolved.iter().find(|entry| entry.id == entry_id) else {
            return denied(VaultMountDenial::NotAuthorized);
        };
        resolved.grants.clone()
    };
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        return denied(VaultMountDenial::NotAuthorized);
    };
    let mut token = std::ptr::null_mut();
    if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) } == 0 {
        unsafe { CloseHandle(process) };
        return denied(VaultMountDenial::NotAuthorized);
    }
    let mut allowed = Vec::new();
    for grant in sids {
        let sid_text = wide(std::ffi::OsStr::new(&grant.sid));
        let mut sid: PSID = std::ptr::null_mut();
        if unsafe { ConvertStringSidToSidW(sid_text.as_ptr(), &mut sid) } != 0 {
            let mut member = 0;
            if unsafe { CheckTokenMembership(token, sid, &mut member) } != 0 && member != 0 {
                allowed.push(grant.sid);
            }
            unsafe { LocalFree(sid as _) };
        }
    }
    unsafe {
        CloseHandle(token);
        CloseHandle(process)
    };
    store.authorize_mount(entry_id, &allowed)
}

fn validate_policy(policy: &VaultAccessPolicy) -> Result<(), VaultError> {
    if policy.schema_version != VAULT_ACCESS_SCHEMA_VERSION
        || policy.policy_id.is_empty()
        || policy.policy_id.len() > 64
        || policy.version == 0
        || policy.entries.is_empty()
        || policy.entries.len() > MAX_ENTRIES
    {
        return Err(VaultError::Validation);
    }
    let mut ids = HashSet::new();
    let mut parents = HashSet::new();
    for entry in &policy.entries {
        if !valid_id(&entry.id)
            || entry.label.is_empty()
            || entry.label.len() > 128
            || entry.owner_account.is_empty()
            || !Path::new(&entry.container_path).is_absolute()
            || entry.grants.is_empty()
            || entry.grants.len() > MAX_GRANTS
        {
            return Err(VaultError::Validation);
        }
        if !ids.insert(&entry.id) {
            return Err(VaultError::Validation);
        }
        let parent = Path::new(&entry.container_path)
            .parent()
            .ok_or(VaultError::Validation)?
            .to_string_lossy()
            .to_ascii_lowercase();
        if !parents.insert(parent) {
            return Err(VaultError::Validation);
        }
        if let Some(letter) = &entry.mount.preferred_letter {
            if letter.len() != 1 || !letter.as_bytes()[0].is_ascii_alphabetic() {
                return Err(VaultError::Validation);
            }
        }
        let mut principals = HashSet::new();
        for grant in &entry.grants {
            if grant.principal_name.is_empty()
                || grant.principal_name.len() > 256
                || !principals.insert(&grant.principal_name)
            {
                return Err(VaultError::Validation);
            }
        }
        if entry.mount.presentation == VaultPresentation::Machine && entry.grants.len() < 2 {
            return Err(VaultError::Validation);
        }
    }
    Ok(())
}

fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn empty_status() -> VaultPolicyStatus {
    VaultPolicyStatus {
        policy_id: None,
        version: 0,
        validation_state: VaultValidationState::NeverApplied,
        applied_at: None,
        entries: vec![],
    }
}
fn status_for(
    policy: &PersistedPolicy,
    validation_state: VaultValidationState,
    result: VaultEntryResult,
) -> VaultPolicyStatus {
    VaultPolicyStatus {
        policy_id: Some(policy.policy.policy_id.clone()),
        version: policy.policy.version,
        validation_state,
        applied_at: Some(policy.applied_at),
        entries: policy
            .policy
            .entries
            .iter()
            .map(|entry| VaultEntryStatus {
                id: entry.id.clone(),
                result: result.clone(),
            })
            .collect(),
    }
}
fn denied(reason: VaultMountDenial) -> VaultAuthorizeMountResponse {
    VaultAuthorizeMountResponse {
        allowed: false,
        launch_ready: false,
        denial_reason: Some(reason),
        mode: None,
        presentation: None,
        preferred_letter: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    struct Fs(Arc<Mutex<HashMap<PathBuf, Vec<u8>>>>);
    impl VaultFs for Fs {
        fn read(&self, p: &Path) -> std::io::Result<Vec<u8>> {
            self.0
                .lock()
                .unwrap()
                .get(p)
                .cloned()
                .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))
        }
        fn atomic_write(&self, p: &Path, b: &[u8]) -> std::io::Result<()> {
            self.0.lock().unwrap().insert(p.into(), b.into());
            Ok(())
        }
        fn stable_file_identity(&self, _: &Path) -> Result<String, VaultError> {
            Ok("volume:1:file:2".into())
        }
        fn validate_dedicated_parent(&self, _: &Path, _: &Path) -> Result<(), VaultError> {
            Ok(())
        }
    }
    struct Resolver;
    impl PrincipalResolver for Resolver {
        fn resolve_sid(&self, name: &str) -> Result<String, VaultError> {
            Ok(format!("S-1-test-{name}"))
        }
    }
    struct Acl;
    impl AclApplier for Acl {
        fn apply_and_verify(&self, plan: &VaultAclPlan) -> Result<(), VaultError> {
            if plan.grants.is_empty() {
                Err(VaultError::AclReadback)
            } else {
                Ok(())
            }
        }
        fn snapshot(&self, plan: &VaultAclPlan) -> Result<Vec<AclSnapshot>, VaultError> {
            Ok(vec![AclSnapshot {
                path: plan.container.clone(),
                descriptor: vec![],
                dacl_protected: false,
            }])
        }
        fn restore(&self, _: &[AclSnapshot]) -> Result<(), VaultError> {
            Ok(())
        }
    }
    fn policy(version: u64, expected: u64) -> VaultAccessPolicy {
        VaultAccessPolicy {
            schema_version: 1,
            policy_id: "p".into(),
            version,
            expected_previous_version: expected,
            entries: vec![VaultAccessEntry {
                id: "shared".into(),
                label: "Shared".into(),
                container_path: "C:\\vaults\\shared.hc".into(),
                container_identity: None,
                owner_account: "Admin".into(),
                grants: vec![
                    wincmd_shared::vault_access::VaultGrantInput {
                        principal_name: "Admin".into(),
                        access: VaultAccess::Write,
                    },
                    wincmd_shared::vault_access::VaultGrantInput {
                        principal_name: "Partner".into(),
                        access: VaultAccess::Write,
                    },
                ],
                mount: wincmd_shared::vault_access::VaultMountPolicy {
                    presentation: VaultPresentation::Machine,
                    preferred_letter: Some("V".into()),
                },
            }],
        }
    }
    fn store(files: Arc<Mutex<HashMap<PathBuf, Vec<u8>>>>) -> VaultAccessStore {
        VaultAccessStore::open(
            Box::new(Fs(files)),
            Box::new(Resolver),
            Box::new(Acl),
            PathBuf::from("/policy"),
        )
    }
    #[test]
    fn applies_atomically_after_acl_verification() {
        let files = Arc::new(Mutex::new(HashMap::new()));
        let s = store(files.clone());
        assert_eq!(s.apply(policy(1, 0), 7).unwrap().version, 1);
        assert!(files
            .lock()
            .unwrap()
            .contains_key(&PathBuf::from("/policy").join(POLICY_FILE)));
    }
    #[test]
    fn stale_write_retains_last_valid_policy() {
        let s = store(Arc::new(Mutex::new(HashMap::new())));
        s.apply(policy(1, 0), 7).unwrap();
        assert_eq!(s.apply(policy(1, 0), 8), Err(VaultError::VersionConflict));
        assert_eq!(s.policy().unwrap().version, 1);
    }
    #[test]
    fn caller_sid_controls_eligibility_but_cannot_launch() {
        let s = store(Arc::new(Mutex::new(HashMap::new())));
        s.apply(policy(1, 0), 7).unwrap();
        let r = s.authorize_mount("shared", &["S-1-test-Partner".into()]);
        assert!(r.allowed);
        assert!(!r.launch_ready);
        assert_eq!(
            r.denial_reason,
            Some(VaultMountDenial::MountBrokerUnavailable)
        );
        assert!(
            !s.authorize_mount("shared", &["S-1-test-Other".into()])
                .allowed
        );
    }
    #[test]
    fn validates_machine_group_shape_owner_and_dedicated_parent() {
        let mut p = policy(1, 0);
        p.entries[0].grants.remove(1);
        assert_eq!(validate_policy(&p), Err(VaultError::Validation));
        let mut p = policy(1, 0);
        let mut second = p.entries[0].clone();
        second.id = "private".into();
        second.container_path = "C:\\vaults\\private.hc".into();
        second.mount.presentation = VaultPresentation::PerUser;
        second.grants.truncate(1);
        p.entries.push(second);
        assert_eq!(validate_policy(&p), Err(VaultError::Validation));
        let s = store(Arc::new(Mutex::new(HashMap::new())));
        let resolved = s.resolve_and_plan(&policy(1, 0)).unwrap();
        assert_eq!(
            resolved[0]
                .1
                .grants
                .iter()
                .filter(|g| g.sid == "S-1-test-Admin")
                .count(),
            1
        );
        assert_eq!(
            resolved[0]
                .1
                .grants
                .iter()
                .find(|g| g.sid == "S-1-test-Admin")
                .unwrap()
                .access,
            VaultAccess::Write
        );
    }

    #[test]
    fn later_acl_failure_restores_every_snapshot_and_keeps_policy_inactive() {
        struct FailingAcl {
            applies: Arc<AtomicUsize>,
            restored: Arc<AtomicBool>,
        }
        impl AclApplier for FailingAcl {
            fn apply_and_verify(&self, _: &VaultAclPlan) -> Result<(), VaultError> {
                if self.applies.fetch_add(1, Ordering::SeqCst) == 0 {
                    Ok(())
                } else {
                    Err(VaultError::AclApply)
                }
            }
            fn snapshot(&self, plan: &VaultAclPlan) -> Result<Vec<AclSnapshot>, VaultError> {
                Ok(vec![AclSnapshot {
                    path: plan.container.clone(),
                    descriptor: vec![],
                    dacl_protected: false,
                }])
            }
            fn restore(&self, snapshots: &[AclSnapshot]) -> Result<(), VaultError> {
                assert_eq!(snapshots.len(), 2);
                self.restored.store(true, Ordering::SeqCst);
                Ok(())
            }
        }

        let mut requested = policy(1, 0);
        let mut private = requested.entries[0].clone();
        private.id = "private".into();
        private.container_path = "C:\\private-vault\\private.hc".into();
        private.mount.presentation = VaultPresentation::PerUser;
        private.grants.truncate(1);
        requested.entries.push(private);

        let applies = Arc::new(AtomicUsize::new(0));
        let restored = Arc::new(AtomicBool::new(false));
        let store = VaultAccessStore::open(
            Box::new(Fs(Arc::new(Mutex::new(HashMap::new())))),
            Box::new(Resolver),
            Box::new(FailingAcl {
                applies: Arc::clone(&applies),
                restored: Arc::clone(&restored),
            }),
            PathBuf::from("/policy"),
        );

        assert_eq!(store.apply(requested, 7), Err(VaultError::AclApply));
        assert!(restored.load(Ordering::SeqCst));
        assert!(store.policy().is_none());
        assert_eq!(
            store.status().validation_state,
            VaultValidationState::Degraded
        );
    }
}

#[cfg(test)]
pub fn test_store() -> std::sync::Arc<VaultAccessStore> {
    struct EmptyFs;
    impl VaultFs for EmptyFs {
        fn read(&self, _: &Path) -> std::io::Result<Vec<u8>> {
            Err(std::io::Error::from(std::io::ErrorKind::NotFound))
        }
        fn atomic_write(&self, _: &Path, _: &[u8]) -> std::io::Result<()> {
            Ok(())
        }
        fn stable_file_identity(&self, _: &Path) -> Result<String, VaultError> {
            Err(VaultError::ContainerIdentity)
        }
        fn validate_dedicated_parent(&self, _: &Path, _: &Path) -> Result<(), VaultError> {
            Err(VaultError::Validation)
        }
    }
    struct NoPrincipal;
    impl PrincipalResolver for NoPrincipal {
        fn resolve_sid(&self, _: &str) -> Result<String, VaultError> {
            Err(VaultError::PrincipalResolution)
        }
    }
    struct NoAcl;
    impl AclApplier for NoAcl {
        fn apply_and_verify(&self, _: &VaultAclPlan) -> Result<(), VaultError> {
            Err(VaultError::AclApply)
        }
        fn snapshot(&self, _: &VaultAclPlan) -> Result<Vec<AclSnapshot>, VaultError> {
            Ok(vec![])
        }
        fn restore(&self, _: &[AclSnapshot]) -> Result<(), VaultError> {
            Ok(())
        }
    }
    std::sync::Arc::new(VaultAccessStore::open(
        Box::new(EmptyFs),
        Box::new(NoPrincipal),
        Box::new(NoAcl),
        PathBuf::from("/test-vault-policy"),
    ))
}
