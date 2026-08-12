// src/panels/cleanup/CleanupCategoryGrid.tsx
// Renders ONE usability tier's card grid (Low impact / History & cache /
// Rebuilds apps or connectivity / Data, accounts & recovery) — one instance
// per System Cleanup tier tab (2026-07 tab split). Scan All and the
// needs-cleaning summary belong to SystemCleanupPanel's tab-navigation row;
// this renderer keeps only tab-specific controls and card content. Pure
// renderer — all state/IPC lives in useCleanupScan.
import { useState } from "react";
import { Button, Icon, Popover, Menu, MenuItem } from "@/components/ui/bp";
import CleanupTraceCard from "../../components/cleanup/CleanupTraceCard";
import {
  CLEANUP_USABILITY_TIERS,
  type CleanupCategory,
  type CleanupUsabilityTier,
} from "./cleanupCategories";
import {
  ALL_USERS_KEY,
  MULTI_USER_CLEANUP_IDS,
  useCleanupScan,
} from "./useCleanupScan";
import { packCleanCards } from "./cleanupCardLayout";

interface Props {
  scan: ReturnType<typeof useCleanupScan>;
  tier: CleanupUsabilityTier;
  isInvestigator: boolean;
  schedulesEnabled: boolean;
  onRequestScheduleAccess?: () => void;
  detailOpenerMap: Record<string, (() => void) | undefined>;
  onDriveWipe: () => void;
  onOpenCombinedDetail: (catId: string) => void;
  onOpenOtherDetail: (cat: {
    catId: string;
    label: string;
    icon: string;
    color: string;
  }) => void;
}

export default function CleanupCategoryGrid({
  scan,
  tier,
  isInvestigator,
  schedulesEnabled,
  onRequestScheduleAccess,
  detailOpenerMap,
  onDriveWipe,
  onOpenCombinedDetail,
  onOpenOtherDetail,
}: Props) {
  const {
    cardDataMap,
    orderedScanCategories,
    handleCardLoad,
    handleCardClear,
    clearAllExcludes,
    setClearAllExcludes,
    availableUsers,
    currentUser,
    isAdminUser,
    selectedUser,
    otherUserDataMap,
    otherUserLoading,
    combinedDataMap,
    isViewingAllUsers,
    isViewingCurrentUser,
    canSwitchUsers,
    showAccountPicker,
    accountSelectValue,
    hasMultipleAccountChoices,
    selectedDisplay,
    loadUserView,
    loadAllUsersView,
    handleSwitchUser,
    handleOtherUserClear,
    handleCardClearAllUsers,
    schedulesById,
    scheduleBusyId,
    handleSetSchedule,
    handleClearSchedule,
  } = scan;

  // The low-impact tab alone owns the bulk-clear exclusion picker and account
  // selector. The per-tab Scan All control is in the tab navigation above.
  const isLowImpactTab = tier === "low-impact";
  const tierMeta = CLEANUP_USABILITY_TIERS.find((t) => t.id === tier)!;

  const sysCats = orderedScanCategories.filter(
    (c) => !c.scopeAware && c.usabilityTier === tier,
  );
  const isClean = (cat: CleanupCategory) => {
    const data = cardDataMap[cat.id];
    return !cat.actionOnly && data?.count === 0 && !data.loading && !data.error;
  };
  const orderActiveCards = (categories: CleanupCategory[]) =>
    [...categories].sort((left, right) => {
      const priority = (cat: CleanupCategory) => {
        const data = cardDataMap[cat.id];
        // A completed scan with findings is the actionable state, so it must
        // never be displaced by cards that have not been scanned yet.
        if (data && data.count > 0) return 0;
        if (data?.error) return 1;
        if (data?.loading) return 2;
        return 3;
      };
      return priority(left) - priority(right);
    });
  const renderCard = (cat: CleanupCategory, compact = false) => {
    const d = cardDataMap[cat.id] || {
      count: -1,
      items: [],
      loading: false,
      clearing: false,
    };
    return (
      <CleanupTraceCard
        key={cat.id}
        category={cat}
        count={d.count}
        preview={(d.items || []).slice(0, 3)}
        loading={d.loading}
        clearing={d.clearing}
        error={d.error}
        compact={compact}
        onClear={() =>
          handleCardClear(
            cat,
            cat.id === "unallocatedErase" ? onDriveWipe : undefined,
          )
        }
        clearDisabled={isInvestigator}
        onViewDetails={detailOpenerMap[cat.id]}
        onLoad={() => handleCardLoad(cat)}
        scheduleMinutes={
          cat.schedulable ? (schedulesById[cat.id] ?? null) : undefined
        }
        scheduleBusy={cat.schedulable && scheduleBusyId === cat.id}
        onSetSchedule={
          cat.schedulable && schedulesEnabled
            ? (m: number) =>
                handleSetSchedule(cat.id, m, !!cat.schedulerRunAsSystem)
            : undefined
        }
        onClearSchedule={
          cat.schedulable && schedulesEnabled
            ? () => handleClearSchedule(cat.id)
            : undefined
        }
        onRequestScheduleAccess={
          cat.schedulable && !schedulesEnabled
            ? onRequestScheduleAccess
            : undefined
        }
      />
    );
  };
  const [excludePickerOpen, setExcludePickerOpen] = useState(false);
  // Non-modal Popover/Menu (not a Radix Select) for the account picker: a
  // modal Select's react-remove-scroll global wheel/touch blocker can leak on
  // fast open/close + panel-switch and stick the page scroll (T16).
  const [accountPickerOpen, setAccountPickerOpen] = useState(false);
  // Confirmed empty categories leave the working grid and are packed four at
  // a time. Scanned categories with findings stay ahead of unscanned cards.
  const activeSysCats = orderActiveCards(
    sysCats.filter((cat) => !isClean(cat)),
  );
  const userViewMap = isViewingAllUsers
    ? combinedDataMap
    : isViewingCurrentUser
      ? cardDataMap
      : otherUserDataMap;
  const userCardsLoading = !isViewingCurrentUser && otherUserLoading;
  const userCats = orderedScanCategories.filter(
    (cat) =>
      cat.scopeAware &&
      cat.usabilityTier === tier &&
      (isViewingCurrentUser || MULTI_USER_CLEANUP_IDS.has(cat.id)),
  );
  const userCardData = (cat: CleanupCategory) => {
    const rawData = userViewMap[cat.id];
    return rawData
      ? { ...rawData, loading: rawData.loading || userCardsLoading }
      : { count: -1, items: [], loading: userCardsLoading, clearing: false };
  };
  const isUserClean = (cat: CleanupCategory) => {
    const data = userCardData(cat);
    return !cat.actionOnly && data.count === 0 && !data.loading && !data.error;
  };
  const renderUserCard = (cat: CleanupCategory, compact = false) => {
    const data = userCardData(cat);
    const offerAllUsers = canSwitchUsers;
    return (
      <CleanupTraceCard
        key={cat.id}
        category={cat}
        count={data.count}
        preview={(data.items || []).slice(0, 3)}
        loading={data.loading}
        clearing={data.clearing}
        error={data.error}
        compact={compact}
        onClear={() =>
          isViewingAllUsers
            ? handleCardClearAllUsers(cat)
            : isViewingCurrentUser
              ? handleCardClear(cat)
              : handleOtherUserClear(cat)
        }
        clearDisabled={isInvestigator}
        onClearAllUsers={
          offerAllUsers && !isViewingAllUsers
            ? () => handleCardClearAllUsers(cat)
            : undefined
        }
        clearScopeLabel={
          isViewingAllUsers ? "all users" : selectedDisplay || "this user"
        }
        onViewDetails={
          isViewingAllUsers
            ? () => onOpenCombinedDetail(cat.id)
            : isViewingCurrentUser
              ? detailOpenerMap[cat.id]
              : () =>
                  onOpenOtherDetail({
                    catId: cat.id,
                    label: cat.label,
                    icon: cat.icon,
                    color: cat.color,
                  })
        }
        onLoad={() =>
          isViewingAllUsers
            ? loadAllUsersView()
            : isViewingCurrentUser
              ? handleCardLoad(cat)
              : loadUserView(selectedUser, [cat.id])
        }
        scheduleMinutes={
          isViewingCurrentUser && cat.schedulable
            ? (schedulesById[cat.id] ?? null)
            : undefined
        }
        scheduleBusy={
          isViewingCurrentUser && cat.schedulable && scheduleBusyId === cat.id
        }
        onSetSchedule={
          isViewingCurrentUser && cat.schedulable && schedulesEnabled
            ? (minutes: number) =>
                handleSetSchedule(cat.id, minutes, !!cat.schedulerRunAsSystem)
            : undefined
        }
        onClearSchedule={
          isViewingCurrentUser && cat.schedulable && schedulesEnabled
            ? () => handleClearSchedule(cat.id)
            : undefined
        }
        onRequestScheduleAccess={
          isViewingCurrentUser && cat.schedulable && !schedulesEnabled
            ? onRequestScheduleAccess
            : undefined
        }
      />
    );
  };
  const activeUserCats = [...userCats]
    .filter((cat) => !isUserClean(cat))
    .sort((left, right) => {
      const priority = (cat: CleanupCategory) => {
        const data = userCardData(cat);
        if (data.count > 0) return 0;
        if (data.error) return 1;
        if (data.loading) return 2;
        return 3;
      };
      return priority(left) - priority(right);
    });
  const userCardIds = new Set(userCats.map((cat) => cat.id));
  const cleanCardPacks = packCleanCards(
    orderedScanCategories.flatMap((cat) => {
      if (cat.usabilityTier !== tier) return [];
      if (cat.scopeAware) {
        return userCardIds.has(cat.id) && isUserClean(cat)
          ? [renderUserCard(cat, true)]
          : [];
      }
      return isClean(cat) ? [renderCard(cat, true)] : [];
    }),
  );

  return (
    <div className="flex flex-col gap-3">
      {isLowImpactTab && (
        <>
          {/* The summary counts sit below the tab navigation in
              SystemCleanupPanel. This block keeps only the low-impact bulk
              clear exclusion controls. */}
          <div className="flex flex-col gap-2 mb-6" data-cleanup-summary="true">
            <div
              className="flex items-center gap-4 px-3 py-2.5 rounded-lg flex-wrap"
              style={{
                background: "var(--color-bg-tertiary)",
                border: "1px solid var(--color-border)",
              }}
            >
              <div className="flex items-center gap-2 flex-wrap flex-1">
                <span className="text-[9px] uppercase tracking-widest text-[var(--color-text-muted)] mr-1">
                  Exclude from bulk clear:
                </span>
                {Array.from(clearAllExcludes).map((id) => (
                  <button
                    key={id}
                    type="button"
                    onClick={() =>
                      setClearAllExcludes((prev) => {
                        const next = new Set(prev);
                        next.delete(id);
                        return next;
                      })
                    }
                    title="Remove from exclusions"
                    aria-label={`Remove ${orderedScanCategories.find((c) => c.id === id)?.label ?? id} from bulk-clear exclusions`}
                    style={{
                      fontSize: 9,
                      fontWeight: 700,
                      letterSpacing: "0.5px",
                      padding: "2px 8px",
                      borderRadius: "var(--radius-sm)",
                      border: "1px solid var(--color-border)",
                      background: "var(--color-bg-secondary)",
                      color: "var(--color-text-primary)",
                      cursor: "pointer",
                      textTransform: "uppercase",
                    }}
                  >
                    ✕{" "}
                    {orderedScanCategories.find((c) => c.id === id)?.label ??
                      id}
                  </button>
                ))}
                {/* Exclude candidates span every scan category (Standard + Deep DFIR),
                 * not just low-impact ones — Wi-Fi Profiles and Browser Audit in
                 * particular need to be excludable even though "Clear Low-Impact"
                 * itself only ever touches low-impact categories. */}
                <Popover
                  position="bottom-right"
                  isOpen={excludePickerOpen}
                  onInteraction={setExcludePickerOpen}
                  popoverClassName="cleanup-exclude-popover"
                  content={
                    <div
                      onPointerDown={(e) => e.stopPropagation()}
                      onClick={(e) => e.stopPropagation()}
                    >
                      <Menu
                        className="bp5-small"
                        style={{ maxHeight: 260, overflowY: "auto" }}
                      >
                        {(() => {
                          const available = orderedScanCategories.filter(
                            (c) => !clearAllExcludes.has(c.id),
                          );
                          if (available.length === 0) {
                            return (
                              <MenuItem
                                disabled
                                text="No more categories to exclude"
                              />
                            );
                          }
                          return available.map((c) => (
                            <MenuItem
                              key={c.id}
                              icon={c.icon as any}
                              text={c.label}
                              onClick={() =>
                                setClearAllExcludes((prev) => {
                                  const next = new Set(prev);
                                  next.add(c.id);
                                  setExcludePickerOpen(false);
                                  return next;
                                })
                              }
                            />
                          ));
                        })()}
                      </Menu>
                    </div>
                  }
                  renderTarget={({
                    ref,
                    onClick: popoverClick,
                    isOpen: _isOpen,
                    ...ariaProps
                  }) => (
                    <button
                      {...ariaProps}
                      ref={ref as React.Ref<HTMLButtonElement>}
                      type="button"
                      title="Add another category to exclude"
                      aria-label="Add a category to bulk-clear exclusions"
                      onClick={(e) => {
                        e.preventDefault();
                        e.stopPropagation();
                        (popoverClick as React.MouseEventHandler)?.(e);
                      }}
                      style={{
                        fontSize: 9,
                        fontWeight: 700,
                        letterSpacing: "0.5px",
                        padding: "2px 8px",
                        borderRadius: "var(--radius-sm)",
                        border: "1px dashed var(--color-border)",
                        background: "transparent",
                        color: "var(--color-text-muted)",
                        cursor: "pointer",
                        textTransform: "uppercase",
                      }}
                    >
                      + Add
                    </button>
                  )}
                ></Popover>
              </div>
            </div>
          </div>

          {/* Account row — visible immediately. With one real user it shows
           * the default/current profile as a read-only chip; with multiple
           * real users admins get the dropdown. Per-user cards are grouped
           * with system cards by usability impact below. */}
          {showAccountPicker && (
            <div className="cleanup-account-picker">
              <div className="cleanup-account-picker__label">
                <Icon icon="user" size={12} />
                <span>Account</span>
              </div>
              {hasMultipleAccountChoices ? (
                <Popover
                  position="bottom-left"
                  isOpen={accountPickerOpen}
                  onInteraction={setAccountPickerOpen}
                  popoverClassName="cleanup-account-select__content"
                  content={
                    <div
                      onPointerDown={(e) => e.stopPropagation()}
                      onClick={(e) => e.stopPropagation()}
                    >
                      <Menu
                        className="bp5-small"
                        style={{ maxHeight: 260, overflowY: "auto" }}
                      >
                        {canSwitchUsers && (
                          <MenuItem
                            icon="people"
                            text="All users"
                            label="Combined"
                            active={accountSelectValue === ALL_USERS_KEY}
                            onClick={() => {
                              handleSwitchUser(ALL_USERS_KEY);
                              setAccountPickerOpen(false);
                            }}
                          />
                        )}
                        {availableUsers.map((u) => (
                          <MenuItem
                            key={u.path ?? u.name}
                            icon={u.isCurrent ? "user" : "person"}
                            text={u.displayName ?? u.name}
                            label={u.isCurrent ? "You" : u.name}
                            active={accountSelectValue === u.name}
                            onClick={() => {
                              handleSwitchUser(u.name);
                              setAccountPickerOpen(false);
                            }}
                          />
                        ))}
                      </Menu>
                    </div>
                  }
                  renderTarget={({
                    ref,
                    onClick: popoverClick,
                    isOpen: _isOpen,
                    ...ariaProps
                  }) => (
                    <button
                      {...ariaProps}
                      ref={ref as React.Ref<HTMLButtonElement>}
                      type="button"
                      className="cleanup-account-select cleanup-account-select--trigger"
                      disabled={
                        !canSwitchUsers ||
                        otherUserLoading ||
                        availableUsers.length === 0
                      }
                      onClick={(e) => {
                        e.preventDefault();
                        e.stopPropagation();
                        (popoverClick as React.MouseEventHandler)?.(e);
                      }}
                    >
                      <span>
                        {otherUserLoading
                          ? "Loading account..."
                          : `${selectedDisplay}${isViewingCurrentUser ? " (you)" : ""}`}
                      </span>
                      <Icon icon="chevron-down" size={13} />
                    </button>
                  )}
                ></Popover>
              ) : (
                <div className="cleanup-account-select cleanup-account-select--readonly">
                  <Icon icon="user" size={13} />
                  <span>
                    {availableUsers.length === 0
                      ? "Loading profile..."
                      : `${selectedDisplay || availableUsers[0]?.displayName || availableUsers[0]?.name || "Current user"} (you)`}
                  </span>
                </div>
              )}
              {!isViewingCurrentUser && (
                <>
                  <Button
                    small
                    minimal
                    icon="refresh"
                    text="Rescan"
                    loading={otherUserLoading}
                    disabled={otherUserLoading}
                    onClick={() =>
                      isViewingAllUsers
                        ? loadAllUsersView()
                        : loadUserView(selectedUser)
                    }
                    style={{ fontSize: 10 }}
                  />
                  <Button
                    small
                    minimal
                    icon="cross"
                    text="Back to you"
                    onClick={() => handleSwitchUser(currentUser)}
                    style={{ fontSize: 10 }}
                  />
                </>
              )}
              {!isAdminUser && (
                <div
                  style={{
                    fontSize: 10,
                    color: "var(--color-text-muted)",
                    display: "flex",
                    alignItems: "center",
                    gap: 4,
                  }}
                >
                  <Icon icon="lock" size={11} />
                  <span>
                    Run as Administrator to view or clear other accounts.
                  </span>
                </div>
              )}
            </div>
          )}
        </>
      )}

      {(activeSysCats.length > 0 ||
        activeUserCats.length > 0 ||
        cleanCardPacks.length > 0) && (
        <section className="mt-12 mb-6">
          <div className="flex flex-wrap items-center gap-3 mb-6 py-3">
            {tierMeta.id === "data-accounts-recovery" && (
              <Icon
                icon="warning-sign"
                size={11}
                style={{ color: tierMeta.color }}
              />
            )}
            <span
              className="text-[10px] font-semibold uppercase tracking-wider whitespace-nowrap"
              style={{ color: tierMeta.color }}
            >
              {tierMeta.label}
            </span>
            <div
              className="flex-1 h-px opacity-50"
              style={{ background: tierMeta.color }}
            />
            <span
              className="text-[9px] italic opacity-60 text-right"
              style={{ color: "var(--color-text-muted)" }}
            >
              {tierMeta.description}
            </span>
          </div>
          <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 xl:grid-cols-4">
            {activeSysCats.map((cat) => renderCard(cat))}
            {activeUserCats.map((cat) => renderUserCard(cat))}
            {cleanCardPacks.map((pack, index) => (
              <div
                key={`clean-pack-${index}`}
                className="grid h-[168px] grid-cols-1 grid-rows-4 gap-1"
                data-cleanup-clean-pack="true"
              >
                {pack}
              </div>
            ))}
          </div>
        </section>
      )}
    </div>
  );
}
