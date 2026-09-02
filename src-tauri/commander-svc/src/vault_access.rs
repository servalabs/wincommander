// SPDX-License-Identifier: AGPL-3.0-or-later
//! SYSTEM-owned vault-access policy.  The request is only intent; resolved
//! principals, file identity, and ACL plans never cross the pipe boundary.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use wincmd_shared::vault_access::{
    PersonalVaultRecord, VAULT_ACCESS_SCHEMA_VERSION, VaultAccess, VaultAccessEntry,
    VaultAccessPolicy, VaultAuthorizeMountResponse, VaultEntryResult, VaultEntryStatus,
    VaultMountDenial, VaultPolicyStatus, VaultPresentation, VaultValidationState,
};

const POLICY_FILE: &str = "vault-access-v1.json";
const ACTIVE_MOUNTS_FILE: &str = "vault-active-mounts-v1.json";
const PERSONAL_VAULTS_FILE: &str = "vault-personal-v1.json";
const MAX_ENTRIES: usize = 64;
const MAX_GRANTS: usize = 32;

// ── `svc.vault.reconcile_access_groups` bounds (Task B) ──────────────────
// Mirrors the MAX_ENTRIES / MAX_GRANTS style above: bounded batch and
// per-group member counts so an admin-authored request can't be used to
// exhaust the service.
const MAX_RECONCILE_GROUPS: usize = 64;
const MAX_RECONCILE_GROUP_MEMBERS: usize = 512;
const MAX_GROUP_NAME_LEN: usize = 64;
/// Case-insensitive prefix reserved for deterministic service-owned groups
/// (see [`managed_group_name`]). An admin-authored group must never collide
/// with it: `reconcile_access_groups` only ever creates-if-absent and syncs
/// membership, so a colliding name would let an admin request silently
/// absorb (and later have its membership overwritten by) a policy-owned
/// group, or vice versa.
const RESERVED_GROUP_PREFIX: &str = "wc-vault-";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VaultError {
    Validation,
    VersionConflict,
    /// The rejected principal or Windows local-group name, exactly as the
    /// admin supplied it (a grant/owner account name, or a managed/admin
    /// local-group name). Never a SID, container path, or ACL/SDDL detail —
    /// this payload is echoed straight into an `ErrorReply.message` by
    /// `pipe.rs`'s `vault_error_message`, and only the admin-typed name is
    /// in scope for that (see this crate's vault privacy boundary).
    PrincipalResolution(String),
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

fn personal_key(path: &str) -> String {
    path.trim().replace('/', "\\").to_ascii_lowercase()
}

fn valid_personal_record(record: &PersonalVaultRecord) -> bool {
    record.scope == VaultPresentation::PerUser
        && !record.owner_sid.is_empty()
        && !record.container_identity.is_empty()
        && Path::new(&record.container_path).is_absolute()
        && record.created_by_session != 0
}

fn valid_pending_personal(pending: &PendingPersonalVault) -> bool {
    !pending.owner_sid.is_empty()
        && Path::new(&pending.container_path).is_absolute()
        && pending.created_by_session != 0
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
    /// Personal containers inherit from arbitrary user-selected parents.  Only
    /// the container file is hardened; changing the parent would alter an
    /// unrelated folder such as D:\ for every account.
    fn apply_container_and_verify(
        &self,
        _container: &Path,
        _grants: &[ResolvedGrant],
    ) -> Result<(), VaultError> {
        Err(VaultError::AclApply)
    }
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
    personal: HashMap<String, PersonalVaultRecord>,
    personal_pending: HashMap<String, PendingPersonalVault>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PendingPersonalVault {
    container_path: String,
    owner_sid: String,
    created_by_session: u32,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PersonalVaultRegistry {
    #[serde(default)]
    records: Vec<PersonalVaultRecord>,
    #[serde(default)]
    pending: Vec<PendingPersonalVault>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PersonalRegistrationReservation {
    New,
    Pending,
}

pub struct VaultAccessStore {
    fs: Box<dyn VaultFs>,
    principals: Box<dyn PrincipalResolver>,
    acls: Box<dyn AclApplier>,
    groups: Box<dyn LocalGroupReconciler>,
    path: PathBuf,
    personal_path: PathBuf,
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
            personal_path: dir.join(PERSONAL_VAULTS_FILE),
            state: Mutex::new(State {
                active: None,
                status: empty_status(),
                personal: HashMap::new(),
                personal_pending: HashMap::new(),
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
        drop(state);
        if let Ok(bytes) = self.fs.read(&self.personal_path) {
            let registry = serde_json::from_slice::<PersonalVaultRegistry>(&bytes).or_else(|_| {
                serde_json::from_slice::<Vec<PersonalVaultRecord>>(&bytes).map(|records| {
                    PersonalVaultRegistry {
                        records,
                        pending: vec![],
                    }
                })
            });
            if let Ok(registry) = registry {
                let mut state = self
                    .state
                    .lock()
                    .unwrap_or_else(|poison| poison.into_inner());
                state.personal = registry
                    .records
                    .into_iter()
                    .filter(valid_personal_record)
                    .map(|record| (personal_key(&record.container_path), record))
                    .collect();
                state.personal_pending = registry
                    .pending
                    .into_iter()
                    .filter(valid_pending_personal)
                    .filter(|pending| {
                        !state
                            .personal
                            .contains_key(&personal_key(&pending.container_path))
                    })
                    .map(|pending| (personal_key(&pending.container_path), pending))
                    .collect();
            }
        }
    }

    /// Reserves durable service-owned registration before the caller-session
    /// engine is allowed to create the file. This prevents a later ACL or
    /// persistence failure from leaving an untracked container behind.
    pub fn begin_personal_registration(
        &self,
        container_path: &str,
        owner_sid: &str,
        created_by_session: u32,
    ) -> Result<PersonalRegistrationReservation, VaultError> {
        if owner_sid.is_empty()
            || created_by_session == 0
            || !Path::new(container_path).is_absolute()
        {
            return Err(VaultError::Validation);
        }
        let pending = PendingPersonalVault {
            container_path: PathBuf::from(container_path).to_string_lossy().into_owned(),
            owner_sid: owner_sid.to_string(),
            created_by_session,
        };
        let mut state = self.state.lock().map_err(|_| VaultError::Persistence)?;
        let key = personal_key(&pending.container_path);
        if state.personal.contains_key(&key) {
            return Err(VaultError::Validation);
        }
        if let Some(existing) = state.personal_pending.get(&key) {
            return if existing.owner_sid == pending.owner_sid
                && existing.created_by_session == pending.created_by_session
            {
                Ok(PersonalRegistrationReservation::Pending)
            } else {
                Err(VaultError::Validation)
            };
        }
        state.personal_pending.insert(key.clone(), pending);
        if self.persist_personal(&state.personal, &state.personal_pending).is_err() {
            state.personal_pending.remove(&key);
            return Err(VaultError::Persistence);
        }
        Ok(PersonalRegistrationReservation::New)
    }

    /// Verifies the newly created container, hardens its file DACL, then
    /// promotes its pre-existing pending record in one durable write.
    pub fn complete_personal_registration(
        &self,
        container_path: &str,
        owner_sid: &str,
        created_by_session: u32,
    ) -> Result<PersonalVaultRecord, VaultError> {
        let key = personal_key(container_path);
        let mut state = self.state.lock().map_err(|_| VaultError::Persistence)?;
        let pending = state
            .personal_pending
            .get(&key)
            .cloned()
            .ok_or(VaultError::Validation)?;
        if pending.owner_sid != owner_sid || pending.created_by_session != created_by_session {
            return Err(VaultError::Validation);
        }
        let container = PathBuf::from(&pending.container_path);
        let identity = self.fs.stable_file_identity(&container)?;
        self.acls.apply_container_and_verify(
            &container,
            &[ResolvedGrant {
                sid: owner_sid.to_string(),
                access: VaultAccess::Write,
            }],
        )?;
        let record = PersonalVaultRecord {
            container_path: pending.container_path.clone(),
            container_identity: identity,
            owner_sid: pending.owner_sid.clone(),
            scope: VaultPresentation::PerUser,
            created_by_session: pending.created_by_session,
        };
        state.personal_pending.remove(&key);
        state.personal.insert(key.clone(), record.clone());
        if self.persist_personal(&state.personal, &state.personal_pending).is_err() {
            state.personal.remove(&key);
            state.personal_pending.insert(key, pending);
            return Err(VaultError::Persistence);
        }
        Ok(record)
    }

    pub fn cancel_personal_registration(
        &self,
        container_path: &str,
        owner_sid: &str,
        created_by_session: u32,
    ) {
        let key = personal_key(container_path);
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        let Some(pending) = state.personal_pending.get(&key) else {
            return;
        };
        if pending.owner_sid != owner_sid || pending.created_by_session != created_by_session {
            return;
        }
        let pending = state.personal_pending.remove(&key);
        if self.persist_personal(&state.personal, &state.personal_pending).is_err() {
            if let Some(pending) = pending {
                state.personal_pending.insert(key, pending);
            }
        }
    }

    /// Returns only a caller-owned, identity-stable personal container.  The
    /// service never accepts an owner SID or scope supplied by the renderer.
    pub fn personal_for_owner(
        &self,
        container_path: &str,
        caller_sid: &str,
    ) -> Result<Option<PersonalVaultRecord>, VaultError> {
        let record = self
            .state
            .lock()
            .map_err(|_| VaultError::Persistence)?
            .personal
            .get(&personal_key(container_path))
            .cloned();
        let Some(record) = record else {
            return Ok(None);
        };
        if record.owner_sid != caller_sid || record.scope != VaultPresentation::PerUser {
            return Ok(None);
        }
        let identity = self
            .fs
            .stable_file_identity(Path::new(&record.container_path))?;
        Ok((identity == record.container_identity).then_some(record))
    }

    /// Builds a non-durable personal record for a container that predates the
    /// personal registry. The caller is not trusted by this method: the pipe
    /// permits use only after the native engine has accepted that caller's
    /// existing credential, then promotes the same identity to a durable
    /// record before returning a successful mount response.
    pub fn legacy_personal_record(
        &self,
        container_path: &str,
        owner_sid: &str,
        created_by_session: u32,
    ) -> Result<PersonalVaultRecord, VaultError> {
        if container_path.is_empty()
            || owner_sid.is_empty()
            || created_by_session == 0
            || !Path::new(container_path).is_absolute()
        {
            return Err(VaultError::Validation);
        }
        Ok(PersonalVaultRecord {
            container_path: container_path.to_owned(),
            container_identity: self.fs.stable_file_identity(Path::new(container_path))?,
            owner_sid: owner_sid.to_owned(),
            scope: VaultPresentation::PerUser,
            created_by_session,
        })
    }

    /// Gives a legacy container's interactive caller a temporary, exact write
    /// DACL while its pre-existing credential is verified by the native
    /// engine. The original ACL snapshot is restored on every failed mount;
    /// a successful mount is immediately promoted through
    /// [`complete_personal_registration`], which replaces this temporary ACL
    /// with the normal durable personal-vault ACL.
    pub fn prepare_legacy_personal_mount(
        &self,
        record: &PersonalVaultRecord,
    ) -> Result<Vec<AclSnapshot>, VaultError> {
        let container = PathBuf::from(&record.container_path);
        let parent = container.parent().ok_or(VaultError::Validation)?.to_path_buf();
        let plan = VaultAclPlan {
            parent,
            container: container.clone(),
            grants: vec![ResolvedGrant {
                sid: record.owner_sid.clone(),
                access: VaultAccess::Write,
            }],
            authorization_grants: Vec::new(),
            managed_groups: Vec::new(),
        };
        let snapshots = self.acls.snapshot(&plan)?;
        if self
            .acls
            .apply_container_and_verify(&container, &plan.grants)
            .is_err()
        {
            let _ = self.acls.restore(&snapshots);
            return Err(VaultError::AclApply);
        }
        Ok(snapshots)
    }

    pub fn restore_legacy_personal_mount(&self, snapshots: &[AclSnapshot]) {
        let _ = self.acls.restore(snapshots);
    }

    fn persist_personal(
        &self,
        records: &HashMap<String, PersonalVaultRecord>,
        pending: &HashMap<String, PendingPersonalVault>,
    ) -> Result<(), VaultError> {
        let bytes = serde_json::to_vec(&PersonalVaultRegistry {
            records: records.values().cloned().collect(),
            pending: pending.values().cloned().collect(),
        })
        .map_err(|_| VaultError::Persistence)?;
        self.fs
            .atomic_write(&self.personal_path, &bytes)
            .map_err(|_| VaultError::Persistence)
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
            // The specific drifted group (if any) makes a better admin-facing
            // name than the entry id; fall back to the entry id only in the
            // defensive length-mismatch case where no single group snapshot
            // can be blamed.
            let drifted_group = group_snapshots
                .iter()
                .find(|snapshot| {
                    plan.managed_groups
                        .iter()
                        .find(|plan| plan.group == snapshot.group)
                        .map(|plan| plan.members.as_slice() == snapshot.members.as_slice())
                        != Some(true)
                })
                .map(|snapshot| snapshot.group.clone());
            if group_snapshots.len() != plan.managed_groups.len() || drifted_group.is_some() {
                let name = drifted_group.unwrap_or_else(|| entry.id.clone());
                return Err(VaultError::PrincipalResolution(name));
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
                // A grant-set mismatch spans potentially many principals, so
                // the entry id (not any single SID) is the admin-facing name.
                return Err(VaultError::PrincipalResolution(entry.id.clone()));
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
                return Err(VaultError::PrincipalResolution(entry.id.clone()));
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

    /// Backs `svc.vault.reconcile_access_groups` (Privileged / SYSTEM-Admin
    /// only — see `pipe.rs`'s `classify_verb`/`authorize` gate). Creates
    /// each admin-authored Windows local group if absent and sets its
    /// membership to EXACTLY the supplied SIDs, reusing the same
    /// [`LocalGroupReconciler`] seam the policy-apply path uses for its own
    /// deterministic `WC-Vault-*` groups. A pre-existing group is never
    /// deleted or renamed — only created-if-absent and membership-synced —
    /// and a group whose name collides with the reserved `WC-Vault-*`
    /// prefix is rejected rather than silently reconciled, so an admin
    /// request can never absorb or corrupt a policy-owned group.
    ///
    /// One group's failure never aborts the batch: it is reported as that
    /// group's own `Failed` result. The only whole-request rejection is an
    /// oversized batch (`Err(VaultError::Validation)`), mirroring
    /// `validate_policy`'s `MAX_ENTRIES` check.
    pub fn reconcile_access_groups(
        &self,
        groups: &[wincmd_shared::vault_access::VaultAccessGroupInput],
    ) -> Result<Vec<wincmd_shared::vault_access::VaultAccessGroupResult>, VaultError> {
        if groups.len() > MAX_RECONCILE_GROUPS {
            return Err(VaultError::Validation);
        }
        Ok(groups
            .iter()
            .map(|group| self.reconcile_one_access_group(group))
            .collect())
    }

    fn reconcile_one_access_group(
        &self,
        group: &wincmd_shared::vault_access::VaultAccessGroupInput,
    ) -> wincmd_shared::vault_access::VaultAccessGroupResult {
        use wincmd_shared::vault_access::{VaultAccessGroupResult, VaultAccessGroupState};

        if !valid_admin_group_name(&group.local_group) {
            return VaultAccessGroupResult {
                local_group: group.local_group.clone(),
                state: VaultAccessGroupState::Failed,
                error: Some("invalid local group name".to_string()),
            };
        }
        if group.member_sids.len() > MAX_RECONCILE_GROUP_MEMBERS {
            return VaultAccessGroupResult {
                local_group: group.local_group.clone(),
                state: VaultAccessGroupState::Failed,
                error: Some("too many members".to_string()),
            };
        }

        let mut members = group.member_sids.clone();
        members.sort();
        members.dedup();

        // A before-snapshot is the only way to tell "just created" and
        // "already exact" apart from "membership changed" — reconciliation
        // alone only proves the post-state is exact.
        let plan = GroupMembershipPlan {
            group: group.local_group.clone(),
            members: members.clone(),
            // `access` is unused by the snapshot/reconcile seam below (it
            // only matters for the vault-policy managed-group path); this
            // admin-group batch has no read/write concept of its own.
            access: VaultAccess::Read,
        };
        let before = match self.groups.snapshot(std::slice::from_ref(&plan)) {
            Ok(snapshots) => snapshots.into_iter().next(),
            Err(error) => {
                return VaultAccessGroupResult {
                    local_group: group.local_group.clone(),
                    state: VaultAccessGroupState::Failed,
                    error: Some(group_error_reason(error)),
                };
            }
        };

        if let Err(error) = self
            .groups
            .reconcile_exact_members(&group.local_group, &members)
        {
            return VaultAccessGroupResult {
                local_group: group.local_group.clone(),
                state: VaultAccessGroupState::Failed,
                error: Some(group_error_reason(error)),
            };
        }

        let state = match before {
            Some(snapshot) if !snapshot.existed => VaultAccessGroupState::Created,
            Some(mut snapshot) => {
                snapshot.members.sort();
                if snapshot.members == members {
                    VaultAccessGroupState::Unchanged
                } else {
                    VaultAccessGroupState::Updated
                }
            }
            // Defensive: `snapshot()` returns one entry per plan by
            // contract, so this should be unreachable in practice.
            None => VaultAccessGroupState::Updated,
        };

        VaultAccessGroupResult {
            local_group: group.local_group.clone(),
            state,
            error: None,
        }
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
            MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
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
            BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
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
    let original_name = name.to_string();
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
        return Err(VaultError::PrincipalResolution(original_name));
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
        return Err(VaultError::PrincipalResolution(original_name));
    }
    sid_to_string(sid.as_mut_ptr() as PSID)
        .map(|sid| (sid, use_type))
        .ok_or(VaultError::PrincipalResolution(original_name))
}

/// `NetLocalGroupAdd` reports an existing *local* group as
/// `ERROR_ALIAS_EXISTS`; `NERR_GroupExists` is what the *global* group API
/// returns. Both mean "already there", which is success for us: the caller
/// owns the group — a deterministic `WC-Vault-*` name or an
/// administrator-authored access group — and its membership is exactly
/// reconciled immediately afterwards.
///
/// Accepting only `NERR_GroupExists` made every reconcile after the very
/// first one fail and leave membership silently unchanged, which unit tests
/// could not catch because they run against a fake reconciler. Kept as a
/// pure function so that regression stays covered.
const ERROR_ALIAS_EXISTS: u32 = 1379;
const NERR_GROUP_EXISTS: u32 = 2223;

fn local_group_add_status_is_ok(status: u32) -> bool {
    status == 0 || status == ERROR_ALIAS_EXISTS || status == NERR_GROUP_EXISTS
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
        let current = local_group_members(group)?
            .ok_or_else(|| VaultError::PrincipalResolution(group.to_string()))?;
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
            return Err(VaultError::PrincipalResolution(group.to_string()));
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
        LOCALGROUP_INFO_1, NetLocalGroupAdd,
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
    if local_group_add_status_is_ok(status) {
        Ok(())
    } else {
        Err(VaultError::PrincipalResolution(group.to_string()))
    }
}

#[cfg(windows)]
fn local_group_members(
    group: &str,
) -> Result<Option<std::collections::HashSet<String>>, VaultError> {
    use windows_sys::Win32::NetworkManagement::NetManagement::{
        LOCALGROUP_MEMBERS_INFO_0, MAX_PREFERRED_LENGTH, NetApiBufferFree, NetLocalGroupGetMembers,
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
        return Err(VaultError::PrincipalResolution(group.to_string()));
    }
    let mut members = std::collections::HashSet::new();
    if !buffer.is_null() {
        let rows = unsafe {
            std::slice::from_raw_parts(buffer as *const LOCALGROUP_MEMBERS_INFO_0, read as usize)
        };
        for row in rows {
            members.insert(
                sid_to_string(row.lgrmi0_sid)
                    .ok_or_else(|| VaultError::PrincipalResolution(group.to_string()))?,
            );
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
        Err(VaultError::PrincipalResolution(group.to_string()))
    }
}

#[cfg(windows)]
fn set_local_group_members(group: &str, sids: &[String], add: bool) -> Result<(), VaultError> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::NetworkManagement::NetManagement::{
        LOCALGROUP_MEMBERS_INFO_0, NetLocalGroupAddMembers, NetLocalGroupDelMembers,
    };
    use windows_sys::Win32::Security::Authorization::ConvertStringSidToSidW;
    let name = wide(std::ffi::OsStr::new(group));
    let mut allocated = Vec::with_capacity(sids.len());
    for value in sids {
        let text = wide(std::ffi::OsStr::new(value));
        let mut sid = std::ptr::null_mut();
        // A malformed member SID string is never surfaced itself (it is
        // caller/service-derived, not admin-typed); attribute the failure to
        // the group being reconciled instead.
        if unsafe { ConvertStringSidToSidW(text.as_ptr(), &mut sid) } == 0 {
            return Err(VaultError::PrincipalResolution(group.to_string()));
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
        Err(VaultError::PrincipalResolution(group.to_string()))
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
    fn apply_container_and_verify(
        &self,
        container: &Path,
        grants: &[ResolvedGrant],
    ) -> Result<(), VaultError> {
        apply_one_acl(container, grants)?;
        verify_one_acl(container, grants)
    }
    fn verify_exact(&self, plan: &VaultAclPlan) -> Result<(), VaultError> {
        verify_one_acl(&plan.parent, &plan.grants)?;
        verify_one_acl(&plan.container, &plan.grants)
    }
}

#[cfg(windows)]
fn verify_one_acl(path: &Path, grants: &[ResolvedGrant]) -> Result<(), VaultError> {
    use windows_sys::Win32::Foundation::{ERROR_SUCCESS, LocalFree};
    use windows_sys::Win32::Security::Authorization::{
        ConvertStringSidToSidW, GetNamedSecurityInfoW, SE_FILE_OBJECT,
    };
    use windows_sys::Win32::Security::{
        AllocateAndInitializeSid, DACL_SECURITY_INFORMATION, EqualSid, GetAce,
        GetSecurityDescriptorControl, PSID, SE_DACL_PROTECTED, SECURITY_NT_AUTHORITY,
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
    use windows_sys::Win32::Foundation::{ERROR_SUCCESS, LocalFree};
    use windows_sys::Win32::Security::Authorization::{GetNamedSecurityInfoW, SE_FILE_OBJECT};
    use windows_sys::Win32::Security::{
        DACL_SECURITY_INFORMATION, GetSecurityDescriptorControl, GetSecurityDescriptorLength,
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
        DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR,
        SetFileSecurityW, UNPROTECTED_DACL_SECURITY_INFORMATION,
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
    use windows_sys::Win32::Foundation::{ERROR_SUCCESS, LocalFree};
    use windows_sys::Win32::Security::Authorization::{
        ConvertStringSidToSidW, GetNamedSecurityInfoW, SE_FILE_OBJECT, SetNamedSecurityInfoW,
    };
    use windows_sys::Win32::Security::{
        ACL, ACL_REVISION, AddAccessAllowedAce, AllocateAndInitializeSid,
        DACL_SECURITY_INFORMATION, EqualSid, GetAce, GetSecurityDescriptorControl, InitializeAcl,
        PROTECTED_DACL_SECURITY_INFORMATION, PSID, SE_DACL_PROTECTED, SECURITY_NT_AUTHORITY,
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
        CheckTokenMembership, DuplicateToken, PSID, SecurityIdentification,
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

/// An admin-authored `svc.vault.reconcile_access_groups` group name must be
/// non-empty, bounded, and must never collide with the reserved
/// `WC-Vault-*` prefix [`managed_group_name`] derives — see
/// [`RESERVED_GROUP_PREFIX`]'s doc comment for why that collision matters.
fn valid_admin_group_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_GROUP_NAME_LEN
        && !name.to_ascii_lowercase().starts_with(RESERVED_GROUP_PREFIX)
}

/// A short, content-free reason for one group's `svc.vault
/// .reconcile_access_groups` failure. `PrincipalResolution`'s payload here
/// is always the admin-supplied `local_group` name (never a member SID),
/// so it is safe to include — same privacy boundary as `pipe.rs`'s
/// `vault_error_message`.
fn group_error_reason(error: VaultError) -> String {
    match error {
        VaultError::PrincipalResolution(name) => {
            format!("windows local group operation failed for '{name}'")
        }
        _ => "local group reconciliation failed".to_string(),
    }
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

    struct PersonalFs {
        files: Arc<Mutex<HashMap<PathBuf, Vec<u8>>>>,
        fail_next_write: Arc<AtomicBool>,
    }
    impl VaultFs for PersonalFs {
        fn read(&self, path: &Path) -> std::io::Result<Vec<u8>> {
            self.files
                .lock()
                .unwrap()
                .get(path)
                .cloned()
                .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))
        }
        fn atomic_write(&self, path: &Path, bytes: &[u8]) -> std::io::Result<()> {
            if self.fail_next_write.swap(false, Ordering::SeqCst) {
                return Err(std::io::Error::from(std::io::ErrorKind::Other));
            }
            self.files.lock().unwrap().insert(path.into(), bytes.into());
            Ok(())
        }
        fn stable_file_identity(&self, _: &Path) -> Result<String, VaultError> {
            Ok("v:1:i:2".into())
        }
        fn validate_dedicated_parent(&self, _: &Path, _: &Path) -> Result<(), VaultError> {
            Ok(())
        }
    }
    struct PersonalAcl(Arc<AtomicBool>);
    impl AclApplier for PersonalAcl {
        fn apply_and_verify(&self, _: &VaultAclPlan) -> Result<(), VaultError> {
            Ok(())
        }
        fn snapshot(&self, _: &VaultAclPlan) -> Result<Vec<AclSnapshot>, VaultError> {
            Ok(vec![])
        }
        fn restore(&self, _: &[AclSnapshot]) -> Result<(), VaultError> {
            Ok(())
        }
        fn apply_container_and_verify(
            &self,
            _: &Path,
            _: &[ResolvedGrant],
        ) -> Result<(), VaultError> {
            if self.0.load(Ordering::SeqCst) {
                Err(VaultError::AclApply)
            } else {
                Ok(())
            }
        }
    }
    fn personal_store(
        files: Arc<Mutex<HashMap<PathBuf, Vec<u8>>>>,
        fail_next_write: Arc<AtomicBool>,
        fail_acl: Arc<AtomicBool>,
    ) -> VaultAccessStore {
        VaultAccessStore::open(
            Box::new(PersonalFs {
                files,
                fail_next_write,
            }),
            Box::new(Resolver),
            Box::new(PersonalAcl(fail_acl)),
            PathBuf::from("/policy"),
        )
    }
    #[test]
    fn applies_atomically_after_acl_verification() {
        let files = Arc::new(Mutex::new(HashMap::new()));
        let s = store(files.clone());
        assert_eq!(s.apply(policy(1, 0), 7).unwrap().version, 1);
        assert!(
            files
                .lock()
                .unwrap()
                .contains_key(&PathBuf::from("/policy").join(POLICY_FILE))
        );
    }

    #[test]
    fn personal_acl_failure_stays_pending_and_recovers_after_service_restart() {
        let files = Arc::new(Mutex::new(HashMap::new()));
        let fail_write = Arc::new(AtomicBool::new(false));
        let fail_acl = Arc::new(AtomicBool::new(true));
        let store = personal_store(files.clone(), fail_write.clone(), fail_acl.clone());
        store
            .begin_personal_registration("C:\\vaults\\personal.hc", "S-1-5-21-owner", 7)
            .unwrap();
        assert_eq!(
            store.complete_personal_registration(
                "C:\\vaults\\personal.hc",
                "S-1-5-21-owner",
                7,
            ),
            Err(VaultError::AclApply)
        );
        assert_eq!(
            store
                .personal_for_owner("C:\\vaults\\personal.hc", "S-1-5-21-owner")
                .unwrap(),
            None
        );

        fail_acl.store(false, Ordering::SeqCst);
        let restarted = personal_store(files, fail_write, fail_acl);
        restarted.load_at_startup();
        assert_eq!(
            restarted
                .begin_personal_registration("C:\\vaults\\personal.hc", "S-1-5-21-owner", 7)
                .unwrap(),
            PersonalRegistrationReservation::Pending
        );
        assert!(restarted
            .complete_personal_registration("C:\\vaults\\personal.hc", "S-1-5-21-owner", 7)
            .is_ok());
    }

    #[test]
    fn personal_owner_record_write_failure_keeps_a_recoverable_pending_record() {
        let files = Arc::new(Mutex::new(HashMap::new()));
        let fail_write = Arc::new(AtomicBool::new(false));
        let fail_acl = Arc::new(AtomicBool::new(false));
        let store = personal_store(files.clone(), fail_write.clone(), fail_acl.clone());
        store
            .begin_personal_registration("C:\\vaults\\personal.hc", "S-1-5-21-owner", 7)
            .unwrap();
        fail_write.store(true, Ordering::SeqCst);
        assert_eq!(
            store.complete_personal_registration(
                "C:\\vaults\\personal.hc",
                "S-1-5-21-owner",
                7,
            ),
            Err(VaultError::Persistence)
        );

        let restarted = personal_store(files, fail_write, fail_acl);
        restarted.load_at_startup();
        assert_eq!(
            restarted
                .begin_personal_registration("C:\\vaults\\personal.hc", "S-1-5-21-owner", 7)
                .unwrap(),
            PersonalRegistrationReservation::Pending
        );
        assert!(restarted
            .complete_personal_registration("C:\\vaults\\personal.hc", "S-1-5-21-owner", 7)
            .is_ok());
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

    // ── Task A: PrincipalResolution carries the rejected principal name ───

    #[test]
    fn principal_resolution_error_carries_the_rejected_principal_name() {
        struct RejectingResolver;
        impl PrincipalResolver for RejectingResolver {
            fn resolve_sid(&self, name: &str) -> Result<String, VaultError> {
                Err(VaultError::PrincipalResolution(name.to_string()))
            }
        }
        let s = VaultAccessStore::open(
            Box::new(Fs(Arc::new(Mutex::new(HashMap::new())))),
            Box::new(RejectingResolver),
            Box::new(Acl),
            PathBuf::from("/policy"),
        );
        // `policy()`'s owner_account is "Admin" and is resolved first.
        assert_eq!(
            s.apply(policy(1, 0), 7),
            Err(VaultError::PrincipalResolution("Admin".to_string()))
        );
    }

    #[test]
    fn principal_resolution_error_names_the_rejected_grant_not_just_the_owner() {
        struct RejectingResolver;
        impl PrincipalResolver for RejectingResolver {
            fn resolve_sid(&self, name: &str) -> Result<String, VaultError> {
                if name == "Partner" {
                    Err(VaultError::PrincipalResolution(name.to_string()))
                } else {
                    Ok(format!("S-1-test-{name}"))
                }
            }
        }
        let s = VaultAccessStore::open(
            Box::new(Fs(Arc::new(Mutex::new(HashMap::new())))),
            Box::new(RejectingResolver),
            Box::new(Acl),
            PathBuf::from("/policy"),
        );
        assert_eq!(
            s.apply(policy(1, 0), 7),
            Err(VaultError::PrincipalResolution("Partner".to_string()))
        );
    }

    // ── Task B: svc.vault.reconcile_access_groups ──────────────────────────

    use wincmd_shared::vault_access::{VaultAccessGroupInput, VaultAccessGroupState};

    #[test]
    fn reconcile_access_groups_creates_then_reports_unchanged_then_updates_exactly() {
        let groups = Groups::default();
        let membership = Arc::clone(&groups.0);
        let s = store_with_groups(Arc::new(Mutex::new(HashMap::new())), groups);

        // First call: the group does not exist yet -> Created, and the
        // supplied members land sorted+deduped exactly as given.
        let created = s
            .reconcile_access_groups(&[VaultAccessGroupInput {
                local_group: "WC_Sales".into(),
                member_sids: vec![
                    "S-1-5-21-2".into(),
                    "S-1-5-21-1".into(),
                    "S-1-5-21-1".into(),
                ],
            }])
            .unwrap();
        assert_eq!(created.len(), 1);
        assert_eq!(created[0].local_group, "WC_Sales");
        assert_eq!(created[0].state, VaultAccessGroupState::Created);
        assert_eq!(created[0].error, None);
        assert_eq!(
            membership.lock().unwrap().get("WC_Sales").unwrap(),
            &vec!["S-1-5-21-1".to_string(), "S-1-5-21-2".to_string()]
        );

        // Same members again -> Unchanged, exact membership untouched.
        let unchanged = s
            .reconcile_access_groups(&[VaultAccessGroupInput {
                local_group: "WC_Sales".into(),
                member_sids: vec!["S-1-5-21-1".into(), "S-1-5-21-2".into()],
            }])
            .unwrap();
        assert_eq!(unchanged[0].state, VaultAccessGroupState::Unchanged);

        // A different member set -> Updated, and membership becomes EXACTLY
        // the new set (the old member is gone, not merely appended to).
        let updated = s
            .reconcile_access_groups(&[VaultAccessGroupInput {
                local_group: "WC_Sales".into(),
                member_sids: vec!["S-1-5-21-3".into()],
            }])
            .unwrap();
        assert_eq!(updated[0].state, VaultAccessGroupState::Updated);
        assert_eq!(
            membership.lock().unwrap().get("WC_Sales").unwrap(),
            &vec!["S-1-5-21-3".to_string()]
        );
    }

    #[test]
    /// Regression: reconciling an existing group must not be treated as a
    /// failure. `NetLocalGroupAdd` returns ERROR_ALIAS_EXISTS (not
    /// NERR_GroupExists) for a local group that is already there, and
    /// rejecting it made every save after the first one silently leave
    /// Windows group membership unchanged. Found only by running against
    /// real Windows — the fake reconciler these tests use cannot reach it.
    #[test]
    fn an_already_existing_local_group_counts_as_created_ok() {
        assert!(local_group_add_status_is_ok(0), "fresh create must succeed");
        assert!(
            local_group_add_status_is_ok(ERROR_ALIAS_EXISTS),
            "an existing local group must be accepted, not reported as failed",
        );
        assert!(
            local_group_add_status_is_ok(NERR_GROUP_EXISTS),
            "the global-group 'already exists' code must stay accepted too",
        );
        assert!(!local_group_add_status_is_ok(5), "access denied must fail");
    }

    #[test]
    fn reconcile_access_groups_rejects_reserved_prefix_and_shape_violations() {
        let s = store_with_groups(Arc::new(Mutex::new(HashMap::new())), Groups::default());
        let results = s
            .reconcile_access_groups(&[
                // Collides with the service-owned WC-Vault-* namespace.
                VaultAccessGroupInput {
                    local_group: "WC-Vault-deadbeef-W".into(),
                    member_sids: vec![],
                },
                // Same collision check must be case-insensitive.
                VaultAccessGroupInput {
                    local_group: "wc-vault-deadbeef-w".into(),
                    member_sids: vec![],
                },
                VaultAccessGroupInput {
                    local_group: "".into(),
                    member_sids: vec![],
                },
                VaultAccessGroupInput {
                    local_group: "x".repeat(MAX_GROUP_NAME_LEN + 1),
                    member_sids: vec![],
                },
            ])
            .unwrap();
        assert!(
            results
                .iter()
                .all(|r| r.state == VaultAccessGroupState::Failed && r.error.is_some())
        );
    }

    #[test]
    fn reconcile_access_groups_reports_per_group_failure_without_aborting_batch() {
        struct FailsForOneGroup {
            fail_for: String,
        }
        impl LocalGroupReconciler for FailsForOneGroup {
            fn reconcile_exact_members(&self, group: &str, _: &[String]) -> Result<(), VaultError> {
                if group == self.fail_for {
                    Err(VaultError::PrincipalResolution(group.to_string()))
                } else {
                    Ok(())
                }
            }
            fn snapshot(
                &self,
                plans: &[GroupMembershipPlan],
            ) -> Result<Vec<GroupMembershipSnapshot>, VaultError> {
                Ok(plans
                    .iter()
                    .map(|plan| GroupMembershipSnapshot {
                        group: plan.group.clone(),
                        members: vec![],
                        existed: false,
                    })
                    .collect())
            }
            fn restore(&self, _: &[GroupMembershipSnapshot]) -> Result<(), VaultError> {
                Ok(())
            }
        }
        let s = VaultAccessStore::open_with_groups(
            Box::new(Fs(Arc::new(Mutex::new(HashMap::new())))),
            Box::new(Resolver),
            Box::new(Acl),
            Box::new(FailsForOneGroup {
                fail_for: "WC_Bad".into(),
            }),
            PathBuf::from("/policy"),
        );
        let results = s
            .reconcile_access_groups(&[
                VaultAccessGroupInput {
                    local_group: "WC_Sales".into(),
                    member_sids: vec!["S-1-5-21-1".into()],
                },
                VaultAccessGroupInput {
                    local_group: "WC_Bad".into(),
                    member_sids: vec!["S-1-5-21-2".into()],
                },
                VaultAccessGroupInput {
                    local_group: "WC_Support".into(),
                    member_sids: vec!["S-1-5-21-3".into()],
                },
            ])
            .unwrap();
        assert_eq!(results.len(), 3);
        assert_eq!(results[0].state, VaultAccessGroupState::Created);
        assert_eq!(results[0].error, None);
        assert_eq!(results[1].state, VaultAccessGroupState::Failed);
        assert!(results[1].error.is_some());
        // The batch continues past the failing group.
        assert_eq!(results[2].state, VaultAccessGroupState::Created);
        assert_eq!(results[2].error, None);
    }

    #[test]
    fn reconcile_access_groups_rejects_an_oversized_batch() {
        let s = store_with_groups(Arc::new(Mutex::new(HashMap::new())), Groups::default());
        let groups: Vec<_> = (0..=MAX_RECONCILE_GROUPS)
            .map(|i| VaultAccessGroupInput {
                local_group: format!("WC_Group{i}"),
                member_sids: vec![],
            })
            .collect();
        assert_eq!(
            s.reconcile_access_groups(&groups),
            Err(VaultError::Validation)
        );
    }

    #[test]
    fn reconcile_access_groups_never_deletes_or_renames_a_pre_existing_group() {
        // Seed a group as if an admin had already created it out-of-band,
        // then reconcile it with a shrunk member set. The group must still
        // be present afterward (create-if-absent + exact-membership only,
        // never delete/rename).
        let groups = Groups::default();
        groups
            .0
            .lock()
            .unwrap()
            .insert("WC_Existing".to_string(), vec!["S-1-5-21-9".to_string()]);
        let membership = Arc::clone(&groups.0);
        let s = store_with_groups(Arc::new(Mutex::new(HashMap::new())), groups);
        let results = s
            .reconcile_access_groups(&[VaultAccessGroupInput {
                local_group: "WC_Existing".into(),
                member_sids: vec![],
            }])
            .unwrap();
        assert_eq!(results[0].state, VaultAccessGroupState::Updated);
        let locked = membership.lock().unwrap();
        assert!(
            locked.contains_key("WC_Existing"),
            "group must not be deleted"
        );
        assert_eq!(locked.get("WC_Existing").unwrap(), &Vec::<String>::new());
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
        fn resolve_sid(&self, name: &str) -> Result<String, VaultError> {
            Err(VaultError::PrincipalResolution(name.to_string()))
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
