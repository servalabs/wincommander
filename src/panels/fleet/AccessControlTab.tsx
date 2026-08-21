import { useEffect, useMemo, useRef, useState, type Dispatch, type SetStateAction } from "react";
import {
  AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent,
  AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Icon } from "@/components/ui/icon";
import { Input } from "@/components/ui/input";
import useBackend from "@/hooks/useBackend";
import { showError, showSuccess } from "@/utils/toast";
import {
  createAccessGroup, membershipCount, mergeAccessUsers, validateAccessDirectory,
} from "./accessControlPolicy";
import type { FleetAccessDirectory, FleetAccessGroup } from "./accessControlTypes";
import FleetField from "./FleetField";
import FleetInfoPopover from "./FleetInfoPopover";

interface AccessControlTabProps {
  directory: FleetAccessDirectory;
  onChange: Dispatch<SetStateAction<FleetAccessDirectory>>;
  onSave: () => void;
}

export default function AccessControlTab({ directory, onChange, onSave }: AccessControlTabProps) {
  const [selectedGroupId, setSelectedGroupId] = useState(directory.groups[0]?.id ?? "");
  const [groupSearch, setGroupSearch] = useState("");
  const [userSearch, setUserSearch] = useState("");
  const [pendingDelete, setPendingDelete] = useState<FleetAccessGroup>();
  const [discovering, setDiscovering] = useState(false);
  const discoveredOnce = useRef(false);
  const { getFleetAccessUsers } = useBackend();
  const errors = useMemo(() => validateAccessDirectory(directory), [directory]);
  const selectedGroup = directory.groups.find(group => group.id === selectedGroupId);

  useEffect(() => {
    if (selectedGroup || directory.groups.length === 0) return;
    setSelectedGroupId(directory.groups[0].id);
  }, [directory.groups, selectedGroup]);

  const discoverUsers = async (quiet = false) => {
    setDiscovering(true);
    const result = await getFleetAccessUsers();
    setDiscovering(false);
    if (!result.success) {
      if (!quiet) void showError(result.error || "Windows user discovery failed.");
      return;
    }
    const discovered = (result.data?.users ?? []).map(user => ({
      id: user.name.toLocaleLowerCase(),
      username: user.name,
      displayName: user.displayName,
      sid: user.sid,
      isCurrent: user.isCurrent,
    }));
    onChange(current => {
      const users = mergeAccessUsers(current.users, discovered);
      return JSON.stringify(users) === JSON.stringify(current.users) ? current : { ...current, users };
    });
    if (!quiet) void showSuccess(`Found ${discovered.length} Windows user${discovered.length === 1 ? "" : "s"}.`);
  };

  useEffect(() => {
    if (discoveredOnce.current) return;
    discoveredOnce.current = true;
    void discoverUsers(true);
  });

  const addGroup = () => {
    const group = createAccessGroup(directory.groups);
    onChange({ ...directory, groups: [...directory.groups, group] });
    setSelectedGroupId(group.id);
  };

  const updateGroup = (patch: Partial<FleetAccessGroup>) => {
    if (!selectedGroup) return;
    onChange({
      ...directory,
      groups: directory.groups.map(group => group.id === selectedGroup.id ? { ...group, ...patch } : group),
    });
  };

  const toggleUser = (id: string, checked: boolean) => {
    if (!selectedGroup) return;
    updateGroup({
      userIds: checked
        ? [...new Set([...selectedGroup.userIds, id])]
        : selectedGroup.userIds.filter(userId => userId !== id),
    });
  };

  const deleteGroup = () => {
    if (!pendingDelete) return;
    onChange({ ...directory, groups: directory.groups.filter(group => group.id !== pendingDelete.id) });
    setPendingDelete(undefined);
  };

  const visibleGroups = directory.groups.filter(group =>
    `${group.name} ${group.localGroup}`.toLocaleLowerCase().includes(groupSearch.toLocaleLowerCase()));
  const visibleUsers = selectedGroup
    ? directory.users
      .filter(user => `${user.username} ${user.displayName ?? ""}`.toLocaleLowerCase().includes(userSearch.toLocaleLowerCase()))
      .sort((left, right) => {
        const membershipOrder = Number(selectedGroup.userIds.includes(right.id)) - Number(selectedGroup.userIds.includes(left.id));
        return membershipOrder || left.username.localeCompare(right.username, undefined, { sensitivity: "base" });
      })
    : [];

  const save = () => {
    if (errors.length) return void showError(errors[0]);
    onSave();
    void showSuccess("Access groups saved on this administrator workstation.");
  };

  return (
    <div className="fleet-access-layout">
      <Card className="fleet-access-pane">
        <CardHeader className="fleet-access-pane-header">
          <div className="fleet-access-title-row">
            <div><CardTitle>Access groups</CardTitle><CardDescription>{directory.groups.length} group{directory.groups.length === 1 ? "" : "s"}</CardDescription></div>
            <Button size="sm" onClick={addGroup}><Icon icon="plus" />Add group</Button>
          </div>
          <Input aria-label="Search access groups" placeholder="Search groups" value={groupSearch} onChange={event => setGroupSearch(event.target.value)} />
        </CardHeader>
        <CardContent className="fleet-access-pane-content">
          <div className="fleet-access-group-list">
            {visibleGroups.map(group => (
              <button
                type="button"
                key={group.id}
                className={`fleet-access-group-row${group.id === selectedGroupId ? " is-selected" : ""}`}
                onClick={() => setSelectedGroupId(group.id)}
              >
                <span><strong>{group.name}</strong><small>{group.localGroup}</small></span>
                <span className="fleet-count-badge">{group.userIds.length}</span>
              </button>
            ))}
            {visibleGroups.length === 0 && (
              <div className="fleet-access-empty"><Icon icon="people" size={24} /><strong>No groups yet</strong><small>Create the first group, then select its Windows users.</small></div>
            )}
          </div>
        </CardContent>
      </Card>

      <Card className="fleet-access-pane">
        {selectedGroup ? <>
          <CardHeader className="fleet-access-pane-header">
            <div className="fleet-access-title-row">
              <div><CardTitle>Group details</CardTitle><CardDescription>{selectedGroup.userIds.length} selected · checked users stay at the top</CardDescription></div>
              <FleetInfoPopover
                label="About access groups"
                title="How access groups work"
                description="Create the Windows user groups here once, then use them in feature-specific permission tabs."
              >
                <ul>
                  <li>A user can belong to more than one group.</li>
                  <li>Group membership alone does not grant Vault or feature access.</li>
                  <li>Each feature decides how overlapping group settings are resolved.</li>
                  <li>Checked users appear first so membership is easy to review.</li>
                </ul>
              </FleetInfoPopover>
            </div>
          </CardHeader>
          <CardContent className="fleet-access-details">
            <div className="fleet-access-fields">
              <FleetField label="Group name"><Input value={selectedGroup.name} onChange={event => updateGroup({ name: event.target.value })} /></FleetField>
              <FleetField label="Windows group"><Input value={selectedGroup.localGroup} onChange={event => updateGroup({ localGroup: event.target.value })} /></FleetField>
            </div>
            <div className="fleet-access-user-tools">
              <Input aria-label="Search Windows users" placeholder="Search Windows users" value={userSearch} onChange={event => setUserSearch(event.target.value)} />
              <Button size="sm" variant="outline" disabled={discovering} onClick={() => void discoverUsers()}><Icon icon="refresh" />{discovering ? "Checking…" : "Refresh"}</Button>
            </div>
            <div className="fleet-access-user-panel">
              <div className="fleet-access-user-list">
                {visibleUsers.map(user => {
                  const checked = selectedGroup.userIds.includes(user.id);
                  const totalMemberships = membershipCount(directory.groups, user.id);
                  const primary = user.displayName || user.username;
                  const secondary = user.displayName
                    && user.displayName.localeCompare(user.username, undefined, { sensitivity: "accent" }) !== 0
                    ? user.username
                    : user.isCurrent ? "Signed-in user" : "";
                  return (
                    <label className={`fleet-access-user-row${checked ? " is-checked" : ""}`} key={user.id}>
                      <input type="checkbox" checked={checked} onChange={event => toggleUser(user.id, event.target.checked)} />
                      <span className="fleet-access-user-copy"><strong>{primary}</strong><small>{secondary || "\u00a0"}</small></span>
                      <span className="fleet-count-badge" title={`${totalMemberships} group memberships`}>{totalMemberships}</span>
                    </label>
                  );
                })}
                {visibleUsers.length === 0 && <div className="fleet-access-empty"><strong>No matching Windows users</strong><small>Refresh discovery after the account is created in Windows.</small></div>}
              </div>
            </div>
            {errors.length > 0 && <p className="fleet-inline-error">{errors[0]}</p>}
            <div className="fleet-access-actions">
              <Button size="sm" variant="danger" onClick={() => setPendingDelete(selectedGroup)}><Icon icon="trash" />Delete group</Button>
              <Button size="sm" variant="primary" onClick={save}>Save groups</Button>
            </div>
          </CardContent>
        </> : <CardContent className="fleet-access-empty fleet-access-empty-main"><Icon icon="people" size={28} /><strong>Select or create a group</strong><small>All group details and Windows users will stay in this pane.</small></CardContent>}
      </Card>

      <AlertDialog open={pendingDelete !== undefined} onOpenChange={open => { if (!open) setPendingDelete(undefined); }}>
        <AlertDialogContent><AlertDialogHeader><AlertDialogTitle>Delete {pendingDelete?.name}?</AlertDialogTitle><AlertDialogDescription>This removes the group and its Vault assignments. The Windows user accounts are not deleted.</AlertDialogDescription></AlertDialogHeader><AlertDialogFooter><AlertDialogCancel>Back</AlertDialogCancel><AlertDialogAction onClick={deleteGroup}>Delete group</AlertDialogAction></AlertDialogFooter></AlertDialogContent>
      </AlertDialog>
    </div>
  );
}
