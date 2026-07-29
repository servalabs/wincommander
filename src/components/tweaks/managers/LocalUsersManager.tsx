import { useEffect, useState, useCallback, useMemo, useRef } from "react";
import { Button, Tag, Spinner, InputGroup, Switch, Icon, type Intent } from "@/components/ui/bp";
import SectionCard from "../../shared/SectionCard";
import { executeBackendCommand } from "../../../hooks/useBackend";
import { showSuccess, showError } from "../../../utils/toast";
import { AnimatedList, AnimatedTableRow, staggerDelay } from "../../shared/AnimatedList";
import { filterAndOrderLocalUsers, type LocalLoginUser } from "./localUsersManagerUtils";
import "./SystemManagers.css";

function normalizeLocalUsers(data: unknown): LocalLoginUser[] {
    if (!data) return [];
    if (Array.isArray(data)) return data as LocalLoginUser[];
    if (typeof data === "object") return [data as LocalLoginUser];
    return [];
}

function disabledReason(user: LocalLoginUser): string | null {
    if (user.hiddenFromLogin) return null;
    if (user.currentUser) return "The currently signed-in account cannot be hidden here.";
    if (user.builtIn) return "Built-in Windows accounts cannot be hidden here.";
    return null;
}

// Single row-status color, checked in this order so a hidden-but-enabled
// user doesn't show two tags fighting for attention (a green "Enabled" next
// to a blue "Hidden"): hidden beats current-user beats enabled beats offline.
function userStatus(user: LocalLoginUser): { intent: Intent; label: string } {
    if (user.hiddenFromLogin) return { intent: "danger", label: "Hidden" };
    if (user.currentUser) return { intent: "success", label: "Current" };
    if (user.enabled) return { intent: "success", label: "Active" };
    return { intent: "none", label: "Disabled" };
}

export default function LocalUsersManager({ embedded = false, scanKey = 0 }: { embedded?: boolean; scanKey?: number }) {
    const [isOpen, setIsOpen] = useState(false);
    const [rows, setRows] = useState<LocalLoginUser[]>([]);
    const [loading, setLoading] = useState(false);
    const [pending, setPending] = useState<Set<string>>(new Set());
    const [filter, setFilter] = useState("");
    const embeddedScanKey = useRef<number | undefined>(undefined);

    const refresh = useCallback(async () => {
        setLoading(true);
        const res = await executeBackendCommand<LocalLoginUser[] | LocalLoginUser>("Get-LocalLoginUsers");
        setLoading(false);
        if (res.success) {
            setRows(normalizeLocalUsers(res.data));
        } else {
            showError(res.error || "Failed to load local users");
        }
    }, []);

    useEffect(() => {
        if (embedded) {
            if (embeddedScanKey.current !== scanKey) {
                embeddedScanKey.current = scanKey;
                void refresh();
            }
            return;
        }
        if (isOpen && rows.length === 0 && !loading) void refresh();
    }, [embedded, isOpen, loading, refresh, rows.length, scanKey]);

    const handleToggle = useCallback(async (user: LocalLoginUser, hidden: boolean) => {
        const reason = hidden ? disabledReason(user) : null;
        if (reason) {
            showError(reason);
            return;
        }

        setPending(prev => new Set(prev).add(user.name));
        const res = await executeBackendCommand<LocalLoginUser>("Set-LocalLoginUserHidden", {
            Name: user.name,
            Hidden: hidden,
        });
        setPending(prev => {
            const next = new Set(prev);
            next.delete(user.name);
            return next;
        });

        if (res.success && res.data) {
            setRows(prev => prev.map(row => row.name === user.name ? res.data as LocalLoginUser : row));
            showSuccess(`${user.name} is now ${hidden ? "hidden from" : "visible on"} the Windows login screen.`);
        } else {
            showError(res.error || `Failed to update ${user.name}`);
            refresh();
        }
    }, [refresh]);

    const filtered = useMemo(
        () => filterAndOrderLocalUsers(rows, filter),
        [rows, filter],
    );

    const hiddenCount = useMemo(
        () => rows.filter(row => row.hiddenFromLogin).length,
        [rows],
    );

    const headerRight = (
        <>
            <span className="system-manager-caption">Local Windows accounts and welcome-screen visibility.</span>
            <div className="system-manager-actions">
                <Tag minimal>{`${rows.length} users`}</Tag>
                <Tag minimal intent={hiddenCount > 0 ? "primary" : "none"}>{`${hiddenCount} hidden`}</Tag>
                {!embedded && <Button minimal icon="refresh" onClick={(e) => { e.stopPropagation(); refresh(); }} loading={loading} />}
            </div>
        </>
    );

    const body = (
        <>
            <InputGroup
                placeholder="Search local users..."
                leftIcon="search"
                value={filter}
                onChange={e => setFilter(e.currentTarget.value)}
                className="system-manager-filter"
            />
            <AnimatedList className="system-manager-list system-manager-list--local-users">
                {loading && rows.length === 0 && <Spinner size={20} />}
                {!loading && filtered.length === 0 && (
                    <div className="system-manager-empty">No local users match.</div>
                )}
                {filtered.map((user, idx) => {
                    const isPending = pending.has(user.name);
                    const reason = disabledReason(user);
                    const canToggle = !isPending && !reason;
                    const status = userStatus(user);
                    return (
                        <AnimatedTableRow
                            key={user.sid || user.name}
                            layoutId={user.sid || user.name}
                            entranceDelay={staggerDelay(idx)}
                        >
                            <div className={`system-manager-row system-manager-row--local-user${user.enabled ? "" : " is-disabled"}`}>
                                <div className="system-manager-identity">
                                    <div className="system-manager-identity__title">
                                        <Icon icon={user.builtIn ? "shield" : "user"} size={13} />
                                        <span className="system-manager-identity__name">{user.name}</span>
                                        {user.fullName && <span className="system-manager-identity__muted">{user.fullName}</span>}
                                    </div>
                                    {(user.description || user.sid) && (
                                        <div className="system-manager-identity__detail">
                                            {user.description || user.sid}
                                        </div>
                                    )}
                                </div>
                                <Tag minimal intent={status.intent}>
                                    {status.label}
                                </Tag>
                                <div className="system-manager-local-user__toggle" title={reason || undefined}>
                                    {user.builtIn && (
                                        <Tag minimal intent="none">
                                            Built-in
                                        </Tag>
                                    )}
                                    <Switch
                                        checked={user.hiddenFromLogin}
                                        disabled={!canToggle}
                                        onChange={e => handleToggle(user, e.currentTarget.checked)}
                                        label="Hide from login"
                                    />
                                </div>
                            </div>
                        </AnimatedTableRow>
                    );
                })}
            </AnimatedList>
        </>
    );

    if (embedded) {
        return (
            <div className="system-manager-embedded">
                <div className="system-manager-embedded__meta">{headerRight}</div>
                {body}
            </div>
        );
    }

    return (
        <SectionCard
            title="Local Users"
            icon="people"
            collapsible
            isOpen={isOpen}
            onToggle={() => setIsOpen(v => !v)}
            headerRight={headerRight}
        >
            {body}
        </SectionCard>
    );
}
