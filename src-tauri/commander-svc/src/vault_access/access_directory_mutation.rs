// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;
use wincmd_shared::vault_access::{
    VaultAccessDirectoryUser, VaultAccessGroupInput, VaultAccessGroupResult,
};

impl VaultAccessStore {
    /// The pipe's mount-operation lock covers authorization, membership and persistence.
    pub fn save_access_directory_for_caller<F>(
        &self,
        mut directory: VaultAccessDirectory,
        caller_sid: &str,
        mut before_change: F,
    ) -> Result<(VaultAccessDirectory, Vec<VaultAccessGroupResult>), VaultError>
    where
        F: FnMut() -> Result<(), VaultError>,
    {
        if caller_sid.is_empty() || !valid_access_directory(&directory) {
            return Err(VaultError::Validation);
        }
        let state = self.state.lock().map_err(|_| VaultError::Persistence)?;
        if !state.access_directory_healthy {
            return Err(VaultError::Persistence);
        }
        let current = state.access_directory.clone();
        let active_principals = state
            .active
            .as_ref()
            .into_iter()
            .flat_map(|active| &active.policy.entries)
            .flat_map(|entry| {
                std::iter::once(entry.owner_account.as_str()).chain(
                    entry
                        .grants
                        .iter()
                        .map(|grant| grant.principal_name.as_str()),
                )
            })
            .map(local_name)
            .collect::<HashSet<_>>();
        for old in &current.groups {
            let next = directory
                .groups
                .iter()
                .find(|group| group.id.eq_ignore_ascii_case(&old.id));
            if next != Some(old) && !is_member(&old.member_sids, caller_sid) {
                return Err(VaultError::Forbidden);
            }
            let drops_name =
                next.is_none_or(|group| !group.local_group.eq_ignore_ascii_case(&old.local_group));
            if drops_name
                && state.active.as_ref().is_some_and(|active| {
                    active.policy.entries.iter().any(|entry| {
                        std::iter::once(entry.owner_account.as_str())
                            .chain(
                                entry
                                    .grants
                                    .iter()
                                    .map(|grant| grant.principal_name.as_str()),
                            )
                            .any(|name| {
                                name.rsplit('\\')
                                    .next()
                                    .unwrap_or(name)
                                    .eq_ignore_ascii_case(&old.local_group)
                            })
                    })
                })
            {
                return Err(VaultError::GroupInUse);
            }
        }
        drop(state);

        let mut requests = Vec::new();
        let mut new_names = Vec::new();
        let mut creator_added = false;
        for group in &mut directory.groups {
            let old = current
                .groups
                .iter()
                .find(|old| old.id.eq_ignore_ascii_case(&group.id));
            if old.is_some_and(|old| !is_member(&old.member_sids, caller_sid)) {
                // Unchanged outsider groups are presentation data, not permission to repair Windows membership.
                continue;
            }
            let new_name =
                old.is_none_or(|old| !old.local_group.eq_ignore_ascii_case(&group.local_group));
            if new_name {
                if active_principals.contains(&local_name(&group.local_group)) {
                    return Err(VaultError::GroupInUse);
                }
                if current
                    .groups
                    .iter()
                    .any(|old| old.local_group.eq_ignore_ascii_case(&group.local_group))
                {
                    return Err(VaultError::GroupNameConflict);
                }
                let plan = GroupMembershipPlan {
                    group: group.local_group.clone(),
                    members: vec![],
                    access: VaultAccess::Read,
                };
                let snapshots = self.groups.snapshot(&[plan])?;
                if snapshots.len() != 1 || snapshots[0].existed {
                    return Err(VaultError::GroupNameConflict);
                }
                new_names.push(group.local_group.clone());
            }
            if old.is_none() && !is_member(&group.member_sids, caller_sid) {
                group.member_sids.push(caller_sid.to_owned());
                creator_added = true;
            }
            requests.push(VaultAccessGroupInput {
                local_group: group.local_group.clone(),
                member_sids: group.member_sids.clone(),
            });
        }
        if creator_added
            && !directory
                .users
                .iter()
                .any(|user| user.sid.eq_ignore_ascii_case(caller_sid))
        {
            directory.users.push(VaultAccessDirectoryUser {
                sid: caller_sid.to_owned(),
                username: "Group creator".to_owned(),
                display_name: None,
            });
        }
        if !valid_access_directory(&directory) {
            return Err(VaultError::Validation);
        }
        before_change()?;
        let plans = requests
            .iter()
            .map(|group| GroupMembershipPlan {
                group: group.local_group.clone(),
                members: group.member_sids.clone(),
                access: VaultAccess::Read,
            })
            .collect::<Vec<_>>();
        let before = self.groups.snapshot(&plans)?;
        if before.len() != plans.len() {
            return Err(VaultError::Persistence);
        }
        if new_names.iter().any(|name| {
            !before
                .iter()
                .any(|snapshot| snapshot.group.eq_ignore_ascii_case(name) && !snapshot.existed)
        }) {
            return Err(VaultError::GroupNameConflict);
        }
        let outcome = (|| {
            let results = self.reconcile_access_groups_before_change(&requests, || Ok(()))?;
            if results.iter().any(|result| {
                result.state == wincmd_shared::vault_access::VaultAccessGroupState::Failed
            }) {
                return Err(VaultError::PrincipalResolution(
                    "access group membership".to_owned(),
                ));
            }
            let readback = self.groups.snapshot(&plans)?;
            if readback.len() != plans.len()
                || plans.iter().any(|plan| {
                    !readback.iter().any(|actual| {
                        actual.existed
                            && actual.group.eq_ignore_ascii_case(&plan.group)
                            && same_members(&actual.members, &plan.members)
                    })
                })
            {
                return Err(VaultError::PrincipalResolution(
                    "access group membership".to_owned(),
                ));
            }
            let bytes = serde_json::to_vec(&directory).map_err(|_| VaultError::Persistence)?;
            let mut state = self.state.lock().map_err(|_| VaultError::Persistence)?;
            self.fs
                .atomic_write(&self.access_directory_path, &bytes)
                .map_err(|_| VaultError::Persistence)?;
            state.access_directory = directory.clone();
            state.access_directory_healthy = true;
            Ok((directory, results))
        })();
        if outcome.is_err() {
            self.groups
                .restore(&before)
                .map_err(|_| VaultError::AclReadback)?;
        }
        outcome
    }
}

fn is_member(members: &[String], sid: &str) -> bool {
    members
        .iter()
        .any(|member| member.eq_ignore_ascii_case(sid))
}

fn local_name(name: &str) -> String {
    name.rsplit('\\')
        .next()
        .unwrap_or(name)
        .trim()
        .to_ascii_lowercase()
}

fn same_members(left: &[String], right: &[String]) -> bool {
    left.len() == right.len() && left.iter().all(|sid| is_member(right, sid))
}
