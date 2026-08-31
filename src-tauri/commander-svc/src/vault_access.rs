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
const ACTIVE_MOUNTS_FILE: &str = "vault-active-mounts-v1.json";
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
    fn resolve_principal(&self, name: &str) -> Result<ResolvedPrincipal, VaultError> {
        Ok(ResolvedPrincipal {
            sid: self.resolve_sid(name)?,
            kind: PrincipalKind::User,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrincipalKind {
    User,
    Group,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedPrincipal {
    pub sid: String,
    pub kind: PrincipalKind,
}

/// Reconciles only deterministic service-owned per-entry groups. Existing
/// policy group principals are kept as ACL principals and are never nested.
pub trait LocalGroupReconciler: Send + Sync {
    fn reconcile_exact_members(
        &self,
        group: &str,
        member_sids: &[String],
    ) -> Result<(), VaultError>;
    fn snapshot(
        &self,
        plans: &[GroupMembershipPlan],
    ) -> Result<Vec<GroupMembershipSnapshot>, VaultError>;
    fn restore(&self, snapshots: &[GroupMembershipSnapshot]) -> Result<(), VaultError>;
}

#[cfg_attr(windows, allow(dead_code))]
struct NoopLocalGroupReconciler;
impl LocalGroupReconciler for NoopLocalGroupReconciler {
    fn reconcile_exact_members(&self, _: &str, _: &[String]) -> Result<(), VaultError> {
        Ok(())
    }
    fn snapshot(
        &self,
        plans: &[GroupMembershipPlan],
    ) -> Result<Vec<GroupMembershipSnapshot>, VaultError> {
        Ok(plans
            .iter()
            .map(|plan| GroupMembershipSnapshot {
                group: plan.group.clone(),
                members: plan.members.clone(),
                existed: true,
            })
            .collect())
    }
    fn restore(&self, _: &[GroupMembershipSnapshot]) -> Result<(), VaultError> {
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupMembershipPlan {
    pub group: String,
    pub members: Vec<String>,
    pub access: VaultAccess,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupMembershipSnapshot {
    pub group: String,
    pub members: Vec<String>,
    pub existed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedGrant {
    pub sid: String,
    pub access: VaultAccess,
}

fn merge_grant(grants: &mut Vec<ResolvedGrant>, sid: String, access: VaultAccess) {
    if let Some(existing) = grants.iter_mut().find(|existing| existing.sid == sid) {
        if access == VaultAccess::Write {
            existing.access = VaultAccess::Write;
        }
    } else {
        grants.push(ResolvedGrant { sid, access });
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VaultAclPlan {
    pub parent: PathBuf,
    pub container: PathBuf,
    pub grants: Vec<ResolvedGrant>,
    /// Principals eligible to request a mount. This includes direct user SIDs
    /// as well as the group SIDs used by the host/container ACL. Keeping the
    /// direct SIDs here lets an explicit grant take effect for an already
    /// logged-on user whose Windows token predates managed-group creation.
    pub authorization_grants: Vec<ResolvedGrant>,
    pub managed_groups: Vec<GroupMembershipPlan>,
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
    /// Production implementations must compare protected DACL ACE identity,
    /// mask, order and count; a protected bit alone is not evidence.
    fn verify_exact(&self, plan: &VaultAclPlan) -> Result<(), VaultError> {
        if self
            .snapshot(plan)?
            .iter()
            .any(|snapshot| !snapshot.dacl_protected)
        {
            Err(VaultError::AclReadback)
        } else {
            Ok(())
        }
    }
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
    #[serde(default)]
    authorization_grants: Vec<ResolvedGrantRecord>,
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
    groups: Box<dyn LocalGroupReconciler>,
    path: PathBuf,
    state: Mutex<State>,
}

impl VaultAccessStore {
    #[cfg_attr(windows, allow(dead_code))]
    pub fn open(
        fs: Box<dyn VaultFs>,
        principals: Box<dyn PrincipalResolver>,
        acls: Box<dyn AclApplier>,
        dir: PathBuf,
    ) -> Self {
        Self::open_with_groups(
            fs,
            principals,
            acls,
            Box::new(NoopLocalGroupReconciler),
            dir,
        )
    }

    pub fn open_with_groups(
        fs: Box<dyn VaultFs>,
        principals: Box<dyn PrincipalResolver>,
        acls: Box<dyn AclApplier>,
        groups: Box<dyn LocalGroupReconciler>,
        dir: PathBuf,
    ) -> Self {
        Self {
            fs,
            principals,
            acls,
            groups,
            path: dir.join(POLICY_FILE),
            state: Mutex::new(State {
                active: None,
                status: empty_status(),
            }),
        }
    }

    pub fn load_at_startup(&self) {
        let mut loaded: Option<PersistedPolicy> = self
            .fs
            .read(&self.path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok());
        let revalidated: Result<Option<()>, VaultError> = loaded
            .as_mut()
            .filter(|policy| policy.policy.schema_version == VAULT_ACCESS_SCHEMA_VERSION)
            .map(|policy| {
                let migrated = self.migrate_authorization_grants(policy)?;
                self.revalidate_persisted_policy(policy)?;
                if migrated {
                    let bytes = serde_json::to_vec(policy).map_err(|_| VaultError::Persistence)?;
                    self.fs
                        .atomic_write(&self.path, &bytes)
                        .map_err(|_| VaultError::Persistence)?;
                }
                Ok(())
            })
            .transpose();
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        match loaded {
            Some(policy) if revalidated == Ok(Some(())) => {
                state.status = status_for(
                    &policy,
                    VaultValidationState::Current,
                    VaultEntryResult::Applied,
                );
                state.active = Some(policy);
            }
            Some(policy) => {
                state.status = status_for(
                    &policy,
                    VaultValidationState::Degraded,
                    VaultEntryResult::AclReadbackFailed,
                );
                state.active = Some(policy);
            }
            None => {}
        }
    }

    /// Policies written before direct authorization SIDs were persisted are
    /// upgraded only after re-deriving them from the trusted policy and current
    /// principal resolver. Exact ACL and membership checks still run before the
    /// migrated bytes replace the durable policy.
    fn migrate_authorization_grants(
        &self,
        persisted: &mut PersistedPolicy,
    ) -> Result<bool, VaultError> {
        let mut plans = self.resolve_and_plan(&persisted.policy)?;
        if plans.len() != persisted.resolved.len() {
            return Err(VaultError::Validation);
        }
        let mut migrated = false;
        for (entry, plan) in &mut plans {
            let stored = persisted
                .resolved
                .iter_mut()
                .find(|stored| stored.id == entry.id)
                .ok_or(VaultError::Validation)?;
            if stored.authorization_grants.is_empty() {
                self.hydrate_managed_group_sids(plan)?;
                stored.authorization_grants = plan
                    .authorization_grants
                    .iter()
                    .map(|grant| ResolvedGrantRecord {
                        sid: grant.sid.clone(),
                        access: grant.access,
                    })
                    .collect();
                migrated = true;
            }
        }
        Ok(migrated)
    }

    /// Re-derive every durable identity and principal mapping on boot.  ACL
    /// snapshots must at least remain protected; the broker makes the mounted
    /// root exact before it is presented. Any failed check leaves policy
    /// degraded and denies mounts rather than trusting stale on-disk state.
    fn revalidate_persisted_policy(&self, persisted: &PersistedPolicy) -> Result<(), VaultError> {
        validate_policy(&persisted.policy)?;
        let plans = self.resolve_and_plan(&persisted.policy)?;
        if plans.len() != persisted.resolved.len() {
            return Err(VaultError::Validation);
        }
        for (entry, mut plan) in plans {
            let stored = persisted
                .resolved
                .iter()
                .find(|stored| stored.id == entry.id)
                .ok_or(VaultError::Validation)?;
            let identity = self
                .fs
                .stable_file_identity(Path::new(&entry.container_path))?;
            if identity != stored.identity {
                return Err(VaultError::ContainerIdentity);
            }
            let group_snapshots = self.groups.snapshot(&plan.managed_groups)?;
            if group_snapshots.len() != plan.managed_groups.len()
                || group_snapshots.iter().any(|snapshot| {
                    plan.managed_groups
                        .iter()
                        .find(|plan| plan.group == snapshot.group)
                        .map(|plan| plan.members.as_slice() == snapshot.members.as_slice())
                        != Some(true)
                })
            {
                return Err(VaultError::PrincipalResolution);
            }
            self.hydrate_managed_group_sids(&mut plan)?;
            let mut derived = plan
                .grants
                .iter()
                .map(|grant| (&grant.sid, grant.access))
                .collect::<Vec<_>>();
            let mut stored_grants = stored
                .grants
                .iter()
                .map(|grant| (&grant.sid, grant.access))
                .collect::<Vec<_>>();
            derived.sort_by(|left, right| left.0.cmp(right.0));
            stored_grants.sort_by(|left, right| left.0.cmp(right.0));
            if derived != stored_grants {
                return Err(VaultError::PrincipalResolution);
            }
            let mut derived_authorization = plan
                .authorization_grants
                .iter()
                .map(|grant| (&grant.sid, grant.access))
                .collect::<Vec<_>>();
            let mut stored_authorization = stored
                .authorization_grants
                .iter()
                .map(|grant| (&grant.sid, grant.access))
                .collect::<Vec<_>>();
            derived_authorization.sort_by(|left, right| left.0.cmp(right.0));
            stored_authorization.sort_by(|left, right| left.0.cmp(right.0));
            if derived_authorization != stored_authorization {
                return Err(VaultError::PrincipalResolution);
            }
            self.acls.verify_exact(&plan)?;
        }
        Ok(())
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

    /// Return service-only mount facts after the pipe caller has already been
    /// authorized. These paths and resolved SIDs must never be serialized onto
    /// the UI wire.
    pub fn mount_plan(
        &self,
        entry_id: &str,
    ) -> Option<(
        VaultAclPlan,
        VaultPresentation,
        Option<String>,
        String,
        wincmd_shared::vault_access::VaultContainerKind,
    )> {
        let state = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let active = state.active.as_ref()?;
        if state.status.validation_state != VaultValidationState::Current {
            return None;
        }
        let entry = active
            .policy
            .entries
            .iter()
            .find(|entry| entry.id == entry_id)?;
        let resolved = active
            .resolved
            .iter()
            .find(|resolved| resolved.id == entry_id)?;
        let container = PathBuf::from(&entry.container_path);
        Some((
            VaultAclPlan {
                parent: container.parent()?.to_path_buf(),
                container,
                grants: resolved
                    .grants
                    .iter()
                    .map(|grant| ResolvedGrant {
                        sid: grant.sid.clone(),
                        access: grant.access,
                    })
                    .collect(),
                authorization_grants: Vec::new(),
                managed_groups: Vec::new(),
            },
            entry.mount.presentation,
            entry.mount.preferred_letter.clone(),
            resolved.identity.clone(),
            entry.container_kind,
        ))
    }

    /// Bounded internal lookup used to construct a caller-specific projection.
    /// It intentionally returns no container, grants, SID, or ACL detail.
    pub fn entry_summaries(
        &self,
    ) -> Vec<(
        String,
        String,
        wincmd_shared::vault_access::VaultContainerKind,
    )> {
        self.state
            .lock()
            .ok()
            .and_then(|state| {
                (state.status.validation_state == VaultValidationState::Current)
                    .then(|| {
                        state.active.as_ref().map(|active| {
                            active
                                .policy
                                .entries
                                .iter()
                                .map(|entry| {
                                    (entry.id.clone(), entry.label.clone(), entry.container_kind)
                                })
                                .collect()
                        })
                    })
                    .flatten()
            })
            .unwrap_or_default()
    }

    /// The policy directory is already created and ACL-verified by the
    /// SYSTEM policy store. Active-mount state therefore uses the same atomic
    /// filesystem seam rather than an ad-hoc user-writable registry.
    pub fn read_active_mounts(&self) -> std::io::Result<Vec<u8>> {
        self.fs.read(&self.path.with_file_name(ACTIVE_MOUNTS_FILE))
    }

    pub fn write_active_mounts(&self, bytes: &[u8]) -> std::io::Result<()> {
        self.fs
            .atomic_write(&self.path.with_file_name(ACTIVE_MOUNTS_FILE), bytes)
    }

    /// Service-only policy identity copied into protected active-mount state.
    /// It is deliberately not part of any renderer/status projection.
    pub fn active_policy_identity(&self) -> Option<(String, u64)> {
        let state = self.state.lock().ok()?;
        let active = state.active.as_ref()?;
        (state.status.validation_state == VaultValidationState::Current)
            .then(|| (active.policy.policy_id.clone(), active.policy.version))
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
        let mut resolved = self.resolve_and_plan(&policy)?;
        let mut snapshots = Vec::new();
        for (_, plan) in &resolved {
            snapshots.extend(self.acls.snapshot(plan)?);
        }
        let group_plans = resolved
            .iter()
            .flat_map(|(_, plan)| plan.managed_groups.clone())
            .collect::<Vec<_>>();
        let group_snapshots = self.groups.snapshot(&group_plans)?;
        for group in &group_plans {
            if let Err(error) = self
                .groups
                .reconcile_exact_members(&group.group, &group.members)
            {
                self.rollback_after_apply(&mut state, &snapshots);
                let _ = self.groups.restore(&group_snapshots);
                return Err(error);
            }
        }
        for (_, plan) in &mut resolved {
            if let Err(error) = self.hydrate_managed_group_sids(plan) {
                self.rollback_after_apply(&mut state, &snapshots);
                let _ = self.groups.restore(&group_snapshots);
                return Err(error);
            }
        }
        for (_, plan) in &resolved {
            if let Err(error) = self.acls.apply_and_verify(plan) {
                // JSON replacement has not happened.  Restore every target
                // already touched before returning the bounded failure.
                self.rollback_after_apply(&mut state, &snapshots);
                let _ = self.groups.restore(&group_snapshots);
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
                    authorization_grants: plan
                        .authorization_grants
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
                let _ = self.groups.restore(&group_snapshots);
                return Err(VaultError::Persistence);
            }
        };
        if self.fs.atomic_write(&self.path, &bytes).is_err() {
            self.rollback_after_apply(&mut state, &snapshots);
            let _ = self.groups.restore(&group_snapshots);
            return Err(VaultError::Persistence);
        }
        // Host/container ACLs are proven here.  Mounted-root enforcement is
        // deliberately pending until a broker can make mount+ACL atomic.
        let status = status_for(
            &persisted,
            VaultValidationState::Current,
            VaultEntryResult::Applied,
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
        if !self
            .fs
            .stable_file_identity(Path::new(&entry.container_path))
            .map(|identity| identity == resolved.identity)
            .unwrap_or(false)
        {
            return denied(VaultMountDenial::NotAuthorized);
        }
        let allowed = resolved
            .authorization_grants
            .iter()
            .filter(|grant| caller_sids.iter().any(|sid| sid == &grant.sid))
            .map(|grant| grant.access)
            .max_by_key(|access| matches!(access, VaultAccess::Write));
        match allowed {
            Some(access) => VaultAuthorizeMountResponse {
                allowed: true,
                launch_ready: true,
                denial_reason: None,
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
                // Direct user grants are materialized as exact membership of
                // deterministic local groups. Existing group grants remain
                // direct ACL principals; we never infer or create nesting.
                let mut grants = Vec::new();
                let mut authorization_grants = Vec::new();
                let mut read_members = Vec::new();
                let mut write_members = Vec::new();
                let mut managed_groups = Vec::new();
                let owner = self.principals.resolve_principal(&entry.owner_account)?;
                match owner.kind {
                    PrincipalKind::User => {
                        write_members.push(owner.sid.clone());
                        merge_grant(&mut authorization_grants, owner.sid, VaultAccess::Write);
                    }
                    PrincipalKind::Group => {
                        merge_grant(&mut grants, owner.sid.clone(), VaultAccess::Write);
                        merge_grant(&mut authorization_grants, owner.sid, VaultAccess::Write);
                    }
                }
                for grant in &entry.grants {
                    let principal = self.principals.resolve_principal(&grant.principal_name)?;
                    if principal.kind == PrincipalKind::User {
                        merge_grant(
                            &mut authorization_grants,
                            principal.sid.clone(),
                            grant.access,
                        );
                        match grant.access {
                            VaultAccess::Read => read_members.push(principal.sid),
                            VaultAccess::Write => write_members.push(principal.sid),
                        }
                        continue;
                    }
                    let sid = principal.sid;
                    merge_grant(&mut grants, sid.clone(), grant.access);
                    merge_grant(&mut authorization_grants, sid, grant.access);
                }
                for (access, members) in [
                    (VaultAccess::Read, &mut read_members),
                    (VaultAccess::Write, &mut write_members),
                ] {
                    members.sort();
                    members.dedup();
                    if members.is_empty() {
                        continue;
                    }
                    let group = managed_group_name(&entry.id, access);
                    managed_groups.push(GroupMembershipPlan {
                        group,
                        members: members.clone(),
                        access,
                    });
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
                        authorization_grants,
                        managed_groups,
                    },
                ))
            })
            .collect()
    }

    fn hydrate_managed_group_sids(&self, plan: &mut VaultAclPlan) -> Result<(), VaultError> {
        for group in &plan.managed_groups {
            let sid = self.principals.resolve_sid(&group.group)?;
            if let Some(existing) = plan.grants.iter_mut().find(|existing| existing.sid == sid) {
                if group.access == VaultAccess::Write {
                    existing.access = VaultAccess::Write;
                }
            } else {
                plan.grants.push(ResolvedGrant {
                    sid: sid.clone(),
                    access: group.access,
                });
            }
            merge_grant(&mut plan.authorization_grants, sid, group.access);
        }
        Ok(())
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
        use std::sync::atomic::{AtomicU64, Ordering};
        use windows_sys::Win32::Storage::FileSystem::{
            MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
        };
        static NEXT_TEMP: AtomicU64 = AtomicU64::new(1);
        let parent = path
            .parent()
            .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
        std::fs::create_dir_all(parent)?;
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
        let temp = parent.join(format!(
            ".{name}.{}.{}.tmp",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::write(&temp, bytes)?;
        let moved = unsafe {
            MoveFileExW(
                wide(temp.as_os_str()).as_ptr(),
                wide(path.as_os_str()).as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        };
        if moved == 0 {
            let error = std::io::Error::last_os_error();
            let _ = std::fs::remove_file(&temp);
            return Err(error);
        }
        Ok(())
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
    fn resolve_principal(&self, name: &str) -> Result<ResolvedPrincipal, VaultError> {
        let (sid, use_type) = lookup_account(name)?;
        // Users (including computer/service accounts) are safe managed-group
        // members. Groups/aliases/well-known groups stay direct ACL grants.
        let kind = if use_type == 1 || use_type == 9 {
            PrincipalKind::User
        } else {
            PrincipalKind::Group
        };
        Ok(ResolvedPrincipal { sid, kind })
    }
}

#[cfg(windows)]
fn wide(value: &std::ffi::OsStr) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    value.encode_wide().chain(std::iter::once(0)).collect()
}

#[cfg(windows)]
fn lookup_account_sid(name: &str) -> Result<String, VaultError> {
    lookup_account(name).map(|(sid, _)| sid)
}

#[cfg(windows)]
fn lookup_account(
    name: &str,
) -> Result<(String, windows_sys::Win32::Security::SID_NAME_USE), VaultError> {
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
    sid_to_string(sid.as_mut_ptr() as PSID)
        .map(|sid| (sid, use_type))
        .ok_or(VaultError::PrincipalResolution)
}

#[cfg(windows)]
pub struct WindowsLocalGroupReconciler;

#[cfg(windows)]
impl LocalGroupReconciler for WindowsLocalGroupReconciler {
    fn reconcile_exact_members(
        &self,
        group: &str,
        member_sids: &[String],
    ) -> Result<(), VaultError> {
        ensure_local_group(group)?;
        let current = local_group_members(group)?.ok_or(VaultError::PrincipalResolution)?;
        let desired: std::collections::HashSet<_> = member_sids.iter().cloned().collect();
        let remove = current.difference(&desired).cloned().collect::<Vec<_>>();
        let add = desired.difference(&current).cloned().collect::<Vec<_>>();
        if !remove.is_empty() {
            set_local_group_members(group, &remove, false)?;
        }
        if !add.is_empty() {
            set_local_group_members(group, &add, true)?;
        }
        if local_group_members(group)? != Some(desired) {
            return Err(VaultError::PrincipalResolution);
        }
        Ok(())
    }
    fn snapshot(
        &self,
        plans: &[GroupMembershipPlan],
    ) -> Result<Vec<GroupMembershipSnapshot>, VaultError> {
        let mut seen = std::collections::HashSet::new();
        plans
            .iter()
            .filter(|plan| seen.insert(plan.group.clone()))
            .map(|plan| {
                let Some(current) = local_group_members(&plan.group)? else {
                    return Ok(GroupMembershipSnapshot {
                        group: plan.group.clone(),
                        members: vec![],
                        existed: false,
                    });
                };
                let mut members = current.into_iter().collect::<Vec<_>>();
                members.sort();
                Ok(GroupMembershipSnapshot {
                    group: plan.group.clone(),
                    members,
                    existed: true,
                })
            })
            .collect()
    }
    fn restore(&self, snapshots: &[GroupMembershipSnapshot]) -> Result<(), VaultError> {
        for snapshot in snapshots {
            if snapshot.existed {
                self.reconcile_exact_members(&snapshot.group, &snapshot.members)?;
            } else {
                delete_local_group(&snapshot.group)?;
            }
        }
        Ok(())
    }
}

#[cfg(windows)]
fn ensure_local_group(group: &str) -> Result<(), VaultError> {
    use windows_sys::Win32::NetworkManagement::NetManagement::{
        NetLocalGroupAdd, LOCALGROUP_INFO_1,
    };
    let mut name = wide(std::ffi::OsStr::new(group));
    let info = LOCALGROUP_INFO_1 {
        lgrpi1_name: name.as_mut_ptr(),
        lgrpi1_comment: std::ptr::null_mut(),
    };
    let status = unsafe {
        NetLocalGroupAdd(
            std::ptr::null(),
            1,
            &info as *const _ as *const u8,
            std::ptr::null_mut(),
        )
    };
    // NERR_GroupExists is deliberately accepted: the group is service-owned
    // by its opaque deterministic name and will be exactly reconciled below.
    if status == 0 || status == 2223 {
        Ok(())
    } else {
        Err(VaultError::PrincipalResolution)
    }
}

#[cfg(windows)]
fn local_group_members(
    group: &str,
) -> Result<Option<std::collections::HashSet<String>>, VaultError> {
    use windows_sys::Win32::NetworkManagement::NetManagement::{
        NetApiBufferFree, NetLocalGroupGetMembers, LOCALGROUP_MEMBERS_INFO_0, MAX_PREFERRED_LENGTH,
    };
    let name = wide(std::ffi::OsStr::new(group));
    let mut buffer: *mut u8 = std::ptr::null_mut();
    let mut read = 0u32;
    let mut total = 0u32;
    let status = unsafe {
        NetLocalGroupGetMembers(
            std::ptr::null(),
            name.as_ptr(),
            0,
            &mut buffer,
            MAX_PREFERRED_LENGTH,
            &mut read,
            &mut total,
            std::ptr::null_mut(),
        )
    };
    // NERR_GroupNotFound: caller may be taking a pre-create transaction
    // snapshot; this is represented explicitly so rollback can delete it.
    if status == 2220 {
        return Ok(None);
    }
    if status != 0 || (read != 0 && buffer.is_null()) {
        return Err(VaultError::PrincipalResolution);
    }
    let mut members = std::collections::HashSet::new();
    if !buffer.is_null() {
        let rows = unsafe {
            std::slice::from_raw_parts(buffer as *const LOCALGROUP_MEMBERS_INFO_0, read as usize)
        };
        for row in rows {
            members.insert(sid_to_string(row.lgrmi0_sid).ok_or(VaultError::PrincipalResolution)?);
        }
        unsafe {
            NetApiBufferFree(buffer as _);
        }
    }
    Ok(Some(members))
}

#[cfg(windows)]
fn delete_local_group(group: &str) -> Result<(), VaultError> {
    use windows_sys::Win32::NetworkManagement::NetManagement::NetLocalGroupDel;
    let name = wide(std::ffi::OsStr::new(group));
    let status = unsafe { NetLocalGroupDel(std::ptr::null(), name.as_ptr()) };
    if status == 0 || status == 2220 {
        Ok(())
    } else {
        Err(VaultError::PrincipalResolution)
    }
}

#[cfg(windows)]
fn set_local_group_members(group: &str, sids: &[String], add: bool) -> Result<(), VaultError> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::NetworkManagement::NetManagement::{
        NetLocalGroupAddMembers, NetLocalGroupDelMembers, LOCALGROUP_MEMBERS_INFO_0,
    };
    use windows_sys::Win32::Security::Authorization::ConvertStringSidToSidW;
    let name = wide(std::ffi::OsStr::new(group));
    let mut allocated = Vec::with_capacity(sids.len());
    for value in sids {
        let text = wide(std::ffi::OsStr::new(value));
        let mut sid = std::ptr::null_mut();
        if unsafe { ConvertStringSidToSidW(text.as_ptr(), &mut sid) } == 0 {
            return Err(VaultError::PrincipalResolution);
        }
        allocated.push(sid);
    }
    let rows = allocated
        .iter()
        .map(|sid| LOCALGROUP_MEMBERS_INFO_0 { lgrmi0_sid: *sid })
        .collect::<Vec<_>>();
    let status = unsafe {
        if add {
            NetLocalGroupAddMembers(
                std::ptr::null(),
                name.as_ptr(),
                0,
                rows.as_ptr() as *const u8,
                rows.len() as u32,
            )
        } else {
            NetLocalGroupDelMembers(
                std::ptr::null(),
                name.as_ptr(),
                0,
                rows.as_ptr() as *const u8,
                rows.len() as u32,
            )
        }
    };
    for sid in allocated {
        unsafe {
            LocalFree(sid as _);
        }
    }
    if status == 0 {
        Ok(())
    } else {
        Err(VaultError::PrincipalResolution)
    }
}

#[cfg(windows)]
pub(crate) fn sid_to_string(sid: windows_sys::Win32::Security::PSID) -> Option<String> {
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
    fn verify_exact(&self, plan: &VaultAclPlan) -> Result<(), VaultError> {
        verify_one_acl(&plan.parent, &plan.grants)?;
        verify_one_acl(&plan.container, &plan.grants)
    }
}

#[cfg(windows)]
fn verify_one_acl(path: &Path, grants: &[ResolvedGrant]) -> Result<(), VaultError> {
    use windows_sys::Win32::Foundation::{LocalFree, ERROR_SUCCESS};
    use windows_sys::Win32::Security::Authorization::{
        ConvertStringSidToSidW, GetNamedSecurityInfoW, SE_FILE_OBJECT,
    };
    use windows_sys::Win32::Security::{
        AllocateAndInitializeSid, EqualSid, GetAce, GetSecurityDescriptorControl,
        DACL_SECURITY_INFORMATION, PSID, SECURITY_NT_AUTHORITY, SE_DACL_PROTECTED,
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
        let authority = SECURITY_NT_AUTHORITY;
        if AllocateAndInitializeSid(&authority, 1, 18, 0, 0, 0, 0, 0, 0, 0, &mut system) == 0
            || AllocateAndInitializeSid(
                &authority,
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
            return Err(VaultError::AclReadback);
        }
        let mut allocated = vec![system, admin];
        for grant in grants {
            let text = wide(std::ffi::OsStr::new(&grant.sid));
            let mut sid = std::ptr::null_mut();
            if ConvertStringSidToSidW(text.as_ptr(), &mut sid) == 0 {
                release_acl_sids(&allocated);
                return Err(VaultError::AclReadback);
            }
            allocated.push(sid);
        }
        let mut acl = std::ptr::null_mut();
        let mut descriptor = std::ptr::null_mut();
        let name = wide(path.as_os_str());
        let outcome = GetNamedSecurityInfoW(
            name.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut acl,
            std::ptr::null_mut(),
            &mut descriptor,
        );
        let expected = std::iter::once((system, FULL))
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
            .collect::<Vec<_>>();
        let mut control = 0u16;
        let mut revision = 0u32;
        let matches = outcome == ERROR_SUCCESS
            && !acl.is_null()
            && !descriptor.is_null()
            && (*acl).AceCount as usize == expected.len()
            && GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) != 0
            && (control & SE_DACL_PROTECTED) != 0
            && expected
                .iter()
                .enumerate()
                .all(|(index, (wanted_sid, wanted_mask))| {
                    let mut ace = std::ptr::null_mut();
                    GetAce(acl, index as u32, &mut ace) != 0
                        && *(ace as *const u8) == 0
                        && *((ace as *const u8).add(4) as *const u32) == *wanted_mask
                        && EqualSid((ace as *const u8).add(8) as PSID, *wanted_sid) != 0
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

/// Check one captured named-pipe peer token against the stored grants. The
/// service owns the handle for the connection lifetime; callers must never
/// reopen a PID after pipe authentication.
#[cfg(windows)]
pub fn authorize_mount_for_token(
    store: &VaultAccessStore,
    entry_id: &str,
    token: windows_sys::Win32::Foundation::HANDLE,
) -> VaultAuthorizeMountResponse {
    use windows_sys::Win32::Foundation::{CloseHandle, LocalFree};
    use windows_sys::Win32::Security::Authorization::ConvertStringSidToSidW;
    use windows_sys::Win32::Security::{
        CheckTokenMembership, DuplicateToken, SecurityIdentification, PSID,
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
        resolved.authorization_grants.clone()
    };
    // CheckTokenMembership needs TOKEN_DUPLICATE when handed a process
    // primary token so Windows can create the impersonation token it tests.
    let mut membership_token = std::ptr::null_mut();
    if unsafe { DuplicateToken(token, SecurityIdentification, &mut membership_token) } == 0 {
        return denied(VaultMountDenial::NotAuthorized);
    }
    let mut allowed = Vec::new();
    for grant in sids {
        let sid_text = wide(std::ffi::OsStr::new(&grant.sid));
        let mut sid: PSID = std::ptr::null_mut();
        if unsafe { ConvertStringSidToSidW(sid_text.as_ptr(), &mut sid) } != 0 {
            let mut member = 0;
            if unsafe { CheckTokenMembership(membership_token, sid, &mut member) } != 0
                && member != 0
            {
                allowed.push(grant.sid);
            }
            unsafe { LocalFree(sid as _) };
        }
    }
    unsafe { CloseHandle(membership_token) };
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

/// Stable, opaque, NetBIOS-safe service group name. The entry ID never
/// becomes a group name directly, which avoids account-name injection and
/// makes rename/reapply idempotent.
fn managed_group_name(entry_id: &str, access: VaultAccess) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(entry_id.as_bytes());
    let suffix = if access == VaultAccess::Write {
        "W"
    } else {
        "R"
    };
    format!(
        "WC-Vault-{}-{suffix}",
        digest[..8]
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    )
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
    #[derive(Default)]
    struct Groups(Arc<Mutex<HashMap<String, Vec<String>>>>);
    impl LocalGroupReconciler for Groups {
        fn reconcile_exact_members(
            &self,
            group: &str,
            members: &[String],
        ) -> Result<(), VaultError> {
            self.0
                .lock()
                .unwrap()
                .insert(group.to_owned(), members.to_vec());
            Ok(())
        }
        fn snapshot(
            &self,
            plans: &[GroupMembershipPlan],
        ) -> Result<Vec<GroupMembershipSnapshot>, VaultError> {
            let current = self.0.lock().unwrap();
            Ok(plans
                .iter()
                .map(|plan| match current.get(&plan.group) {
                    Some(members) => GroupMembershipSnapshot {
                        group: plan.group.clone(),
                        members: members.clone(),
                        existed: true,
                    },
                    None => GroupMembershipSnapshot {
                        group: plan.group.clone(),
                        members: vec![],
                        existed: false,
                    },
                })
                .collect())
        }
        fn restore(&self, snapshots: &[GroupMembershipSnapshot]) -> Result<(), VaultError> {
            let mut current = self.0.lock().unwrap();
            for snapshot in snapshots {
                if snapshot.existed {
                    current.insert(snapshot.group.clone(), snapshot.members.clone());
                } else {
                    current.remove(&snapshot.group);
                }
            }
            Ok(())
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
                dacl_protected: true,
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
                container_kind: wincmd_shared::vault_access::VaultContainerKind::Standard,
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
    fn store_with_groups(
        files: Arc<Mutex<HashMap<PathBuf, Vec<u8>>>>,
        groups: Groups,
    ) -> VaultAccessStore {
        VaultAccessStore::open_with_groups(
            Box::new(Fs(files)),
            Box::new(Resolver),
            Box::new(Acl),
            Box::new(groups),
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
    fn managed_group_sid_controls_eligibility_and_is_launch_ready() {
        let s = store(Arc::new(Mutex::new(HashMap::new())));
        s.apply(policy(1, 0), 7).unwrap();
        let r = s.authorize_mount(
            "shared",
            &[format!(
                "S-1-test-{}",
                managed_group_name("shared", VaultAccess::Write)
            )],
        );
        assert!(r.allowed);
        assert!(r.launch_ready);
        assert_eq!(r.denial_reason, None);
        let already_logged_on_partner = s.authorize_mount("shared", &["S-1-test-Partner".into()]);
        assert!(
            already_logged_on_partner.allowed,
            "an explicit user grant must not require a fresh Windows logon"
        );
        assert_eq!(already_logged_on_partner.mode, Some(VaultAccess::Write));
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
                .managed_groups
                .iter()
                .filter(|g| g.group == managed_group_name("shared", VaultAccess::Write))
                .count(),
            1
        );
        assert_eq!(
            resolved[0]
                .1
                .managed_groups
                .iter()
                .find(|g| g.group == managed_group_name("shared", VaultAccess::Write))
                .unwrap()
                .access,
            VaultAccess::Write
        );
    }

    #[test]
    fn accepts_a_shared_dual_container_policy() {
        let mut shared_dual = policy(1, 0);
        shared_dual.entries[0].container_kind =
            wincmd_shared::vault_access::VaultContainerKind::Dual;
        assert_eq!(validate_policy(&shared_dual), Ok(()));
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

    #[test]
    fn startup_rejects_a_protected_but_wrong_acl() {
        struct WrongProtectedAcl;
        impl AclApplier for WrongProtectedAcl {
            fn apply_and_verify(&self, _: &VaultAclPlan) -> Result<(), VaultError> {
                Ok(())
            }
            fn snapshot(&self, plan: &VaultAclPlan) -> Result<Vec<AclSnapshot>, VaultError> {
                Ok(vec![AclSnapshot {
                    path: plan.container.clone(),
                    descriptor: vec![],
                    dacl_protected: true,
                }])
            }
            fn restore(&self, _: &[AclSnapshot]) -> Result<(), VaultError> {
                Ok(())
            }
            // Models an inherited/extra ACE or wrong access mask despite the
            // protected control bit: startup must reject it.
            fn verify_exact(&self, _: &VaultAclPlan) -> Result<(), VaultError> {
                Err(VaultError::AclReadback)
            }
        }
        let files = Arc::new(Mutex::new(HashMap::new()));
        let installed = VaultAccessStore::open(
            Box::new(Fs(files.clone())),
            Box::new(Resolver),
            Box::new(Acl),
            PathBuf::from("/policy"),
        );
        installed.apply(policy(1, 0), 7).unwrap();
        let restarted = VaultAccessStore::open(
            Box::new(Fs(files)),
            Box::new(Resolver),
            Box::new(WrongProtectedAcl),
            PathBuf::from("/policy"),
        );
        restarted.load_at_startup();
        assert_eq!(
            restarted.status().validation_state,
            VaultValidationState::Degraded
        );
    }

    #[test]
    fn startup_migrates_legacy_policy_and_keeps_explicit_user_access() {
        let files = Arc::new(Mutex::new(HashMap::new()));
        let installed = store(files.clone());
        installed.apply(policy(1, 0), 7).unwrap();
        let path = PathBuf::from("/policy").join(POLICY_FILE);
        {
            let mut locked = files.lock().unwrap();
            let mut value: serde_json::Value =
                serde_json::from_slice(locked.get(&path).unwrap()).unwrap();
            for entry in value["resolved"].as_array_mut().unwrap() {
                entry
                    .as_object_mut()
                    .unwrap()
                    .remove("authorization_grants");
            }
            locked.insert(path.clone(), serde_json::to_vec(&value).unwrap());
        }

        let restarted = store(files.clone());
        restarted.load_at_startup();
        assert_eq!(
            restarted.status().validation_state,
            VaultValidationState::Current
        );
        assert!(
            restarted
                .authorize_mount("shared", &["S-1-test-Partner".into()])
                .allowed
        );
        let migrated: serde_json::Value =
            serde_json::from_slice(files.lock().unwrap().get(&path).unwrap()).unwrap();
        assert!(migrated["resolved"][0]["authorization_grants"].is_array());
    }

    #[test]
    fn first_apply_creates_exact_groups_and_restart_detects_membership_drift() {
        let files = Arc::new(Mutex::new(HashMap::new()));
        let groups = Groups::default();
        let membership = Arc::clone(&groups.0);
        let store = store_with_groups(files.clone(), groups);
        store.apply(policy(1, 0), 7).unwrap();
        let write = managed_group_name("shared", VaultAccess::Write);
        assert_eq!(
            membership.lock().unwrap().get(&write).unwrap(),
            &vec!["S-1-test-Admin".to_string(), "S-1-test-Partner".to_string()]
        );
        // Re-applying a successive policy produces the same deterministic
        // membership, not a new/nested group.
        store.apply(policy(2, 1), 8).unwrap();
        assert_eq!(membership.lock().unwrap().len(), 1);
        membership.lock().unwrap().get_mut(&write).unwrap().clear();
        let restarted = store_with_groups(files, Groups(membership));
        restarted.load_at_startup();
        assert_eq!(
            restarted.status().validation_state,
            VaultValidationState::Degraded
        );
    }

    #[test]
    fn acl_failure_rolls_back_new_managed_group() {
        struct Fails;
        impl AclApplier for Fails {
            fn apply_and_verify(&self, _: &VaultAclPlan) -> Result<(), VaultError> {
                Err(VaultError::AclApply)
            }
            fn snapshot(&self, plan: &VaultAclPlan) -> Result<Vec<AclSnapshot>, VaultError> {
                Ok(vec![AclSnapshot {
                    path: plan.container.clone(),
                    descriptor: vec![],
                    dacl_protected: true,
                }])
            }
            fn restore(&self, _: &[AclSnapshot]) -> Result<(), VaultError> {
                Ok(())
            }
        }
        let files = Arc::new(Mutex::new(HashMap::new()));
        let groups = Groups::default();
        let membership = Arc::clone(&groups.0);
        let store = VaultAccessStore::open_with_groups(
            Box::new(Fs(files)),
            Box::new(Resolver),
            Box::new(Fails),
            Box::new(groups),
            PathBuf::from("/policy"),
        );
        assert_eq!(store.apply(policy(1, 0), 7), Err(VaultError::AclApply));
        assert!(membership.lock().unwrap().is_empty());
    }
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
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
