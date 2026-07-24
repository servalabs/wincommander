import { useState } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { Icon, Popover, Menu, MenuItem } from "@/components/ui/bp";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import type { CleanupCategory } from "../../panels/cleanup/cleanupCategories";
import CleanupScheduleControl from "./CleanupScheduleControl";
import "./CleanupTraceCard.css";

interface CleanupTraceCardProps {
  category: CleanupCategory;
  count: number;
  preview: string[];
  loading: boolean;
  clearing: boolean;
  error?: string;
  onClear: () => void;
  onViewDetails?: () => void;
  onLoad?: () => void;
  scheduleMinutes?: number | null;
  scheduleBusy?: boolean;
  onSetSchedule?: (minutes: number) => boolean | Promise<boolean>;
  onClearSchedule?: () => boolean | Promise<boolean>;
  onRequestScheduleAccess?: () => void;
  onClearAllUsers?: () => void;
  clearScopeLabel?: string;
  clearDisabled?: boolean;
  /** Completed scans render empty categories as a single compact row. */
  compact?: boolean;
}

const TRACE_CARD_HEIGHT = '168px';

type BandState = 'has' | 'clean' | 'scanning' | 'not-scanned' | 'action-only';
interface BandDef { bg: string; borderColor: string; labelColor: string; icon: string; label: string }

const BAND: Record<BandState, BandDef> = {
  has:          { bg: 'var(--color-warning-dim)',  borderColor: 'color-mix(in srgb, var(--color-warning) 28%, transparent)', labelColor: 'var(--color-warning)',      icon: 'warning-sign', label: 'Traces found'    },
  clean:        { bg: 'var(--color-success-dim)',  borderColor: 'color-mix(in srgb, var(--color-success) 25%, transparent)', labelColor: 'var(--color-success)',      icon: 'tick-circle',  label: 'Clean'           },
  scanning:     { bg: 'var(--color-bg-secondary)', borderColor: 'var(--color-border)',   labelColor: 'var(--color-info)',         icon: 'refresh',      label: 'Scanning…'       },
  'not-scanned':{ bg: 'var(--color-bg-secondary)', borderColor: 'var(--color-border)',   labelColor: 'var(--color-text-muted)',   icon: 'minus',        label: 'Not scanned'     },
  'action-only':{ bg: 'var(--color-danger-dim)',   borderColor: 'color-mix(in srgb, var(--color-danger) 20%, transparent)',  labelColor: 'var(--color-danger)',       icon: 'flame',        label: 'One-time action' },
};

export default function CleanupTraceCard({
  category, count, preview, loading, clearing, error,
  onClear, onViewDetails, onLoad,
  scheduleMinutes, scheduleBusy, onSetSchedule, onClearSchedule,
  onRequestScheduleAccess,
  onClearAllUsers, clearScopeLabel, clearDisabled,
  compact = false,
}: CleanupTraceCardProps) {
  const [showParticles, setShowParticles] = useState(false);

  const minInterval   = category.minIntervalMinutes ?? 5;
  const showScheduler = (!!onSetSchedule || !!onRequestScheduleAccess) && !!category.schedulable;
  const isEmpty       = count === 0;
  const isActionOnly  = category.actionOnly;
  const hasData       = count > 0;
  const hasClear      = !!category.clearDataKey;
  const isNotLoaded   = count === -1 && !isActionOnly && !loading;
  const hasScanOutput = !isActionOnly && (count >= 0 || !!error);
  const infoText      = category.description;
  const bandState: BandState =
    isActionOnly ? 'action-only' : loading ? 'scanning' : hasData ? 'has' : isEmpty ? 'clean' : 'not-scanned';
  const band = BAND[bandState];
  // Unlike a Popover, Tooltip does not restore focus to a card action that
  // moved during wheel input and therefore cannot pull this panel backward.
  const infoTooltip = (
    <Tooltip>
      <TooltipTrigger asChild>
        <button
          type="button"
          className="grid place-items-center flex-shrink-0 rounded transition-colors"
          style={{ width: 22, height: 22, border: 'none', cursor: 'help' }}
          data-trace-card-action="info"
          title={`About ${category.label}`}
          onClick={e => { e.preventDefault(); e.stopPropagation(); }}
        >
          <Icon icon="info-sign" size={14} color="currentColor" />
        </button>
      </TooltipTrigger>
      <TooltipContent side="bottom" align="end" className="w-[260px] p-2.5 text-[11px] leading-[1.45]">
        <strong className="mb-1 block text-[var(--color-text-primary)]">{category.label}</strong>
        {infoText}
      </TooltipContent>
    </Tooltip>
  );
  const scheduleControl = showScheduler && !isActionOnly ? (
    <CleanupScheduleControl
      minInterval={minInterval}
      scheduleMinutes={scheduleMinutes}
      scheduleBusy={scheduleBusy}
      onSetSchedule={onSetSchedule}
      onClearSchedule={onClearSchedule}
      onRequestScheduleAccess={onRequestScheduleAccess}
    />
  ) : null;
  const rescanButton = !isActionOnly && onLoad && hasScanOutput ? (
    <button
      type="button"
      onClick={e => { e.stopPropagation(); onLoad(); }}
      disabled={loading}
      className="flex items-center justify-center flex-shrink-0 rounded transition-colors disabled:opacity-40"
      data-trace-card-action="rescan"
      style={{ width: 22, height: 22, border: 'none', cursor: loading ? 'not-allowed' : 'pointer' }}
      title="Rescan"
    >
      <Icon icon="refresh" size={14} color="currentColor" />
    </button>
  ) : null;

  // Footer is only rendered when there's something actionable
  const showFooter = isActionOnly || (!loading && hasData && (!!onViewDetails || hasClear));

  const iconBg = `${category.color}1e`;

  // ── CLEAN STATE — scanned, zero traces, nothing to action. ──
  if (bandState === 'clean') {
    return (
      <motion.div
        initial={{ opacity: 0, scale: 0.95 }}
        animate={{ opacity: 1, scale: 1 }}
        transition={{ duration: 0.2 }}
        className={`relative overflow-hidden flex flex-col text-center${compact ? ' cleanup-trace-card--compact' : ''}`}
        data-cleanup-trace-card
        style={{
          height: compact ? 44 : TRACE_CARD_HEIGHT,
          padding: 0,
          /* Clean = good = should recede, not shout. Neutral surface + a small,
             low-key check (owner: the bright green tick drew too much attention). */
          background: 'var(--color-bg-secondary)',
          border: '1px solid var(--color-border)',
          borderRadius: 'var(--r-lg, 10px)',
        }}
      >
        {compact ? (
          <div className="flex h-full items-center gap-2 px-3">
            <Icon icon="tick-circle" size={14} style={{ color: 'var(--color-success)' }} />
            <span className="flex-1 truncate text-left font-bold uppercase" style={{ fontSize: 10, letterSpacing: '0.5px', color: 'var(--color-text-muted)' }} title={category.label}>
              {category.label}
            </span>
            {infoTooltip}
            {rescanButton}
          </div>
        ) : (
          <>
            <div
              className="flex items-center justify-between flex-shrink-0"
              style={{ padding: '4px 8px', background: band.bg, borderBottom: `1px solid ${band.borderColor}` }}
            >
              <span className="flex items-center gap-1.5 font-bold uppercase"
                style={{ fontSize: 8.5, letterSpacing: '0.8px', color: band.labelColor }}>
                <Icon icon={band.icon as any} size={10} />
                {band.label}
              </span>
              <span className="flex items-center gap-0.5">
                {infoTooltip}
                {scheduleControl}
                {rescanButton}
              </span>
            </div>
            <div className="flex-1 flex flex-col items-center justify-center" style={{ padding: '6px 12px' }}>
              <Icon icon="tick-circle" size={26} style={{ color: 'color-mix(in srgb, var(--color-success) 60%, var(--color-text-muted))' }} />
              <div
                className="font-bold uppercase truncate"
                style={{ fontSize: 10, letterSpacing: '0.5px', color: 'var(--color-text-muted)', marginTop: 6, maxWidth: 'calc(100% - 24px)' }}
                title={category.label}
              >
                {category.label}
              </div>
            </div>
          </>
        )}
        {!compact && category.regeneratesNote && (
          <div
            title={category.regeneratesNote}
            className="flex items-center gap-1"
            style={{ fontSize: 8, color: 'var(--color-text-muted)', opacity: 0.6, cursor: 'help', marginTop: 4 }}
          >
            <Icon icon="info-sign" size={8} color="currentColor" />
            <span>Regenerates</span>
          </div>
        )}
      </motion.div>
    );
  }

  return (
    <motion.div
      initial={{ opacity: 0, scale: 0.95 }}
      animate={{ opacity: 1, scale: 1 }}
      transition={{ duration: 0.2 }}
      className="relative overflow-hidden flex flex-col"
      data-cleanup-trace-card
      style={{
        height: TRACE_CARD_HEIGHT,
        background: 'var(--color-bg-secondary)',
        border: isActionOnly ? '1px dashed var(--color-border)' : '1px solid var(--color-border)',
        borderRadius: 'var(--r-lg, 10px)',
      }}
    >

      {/* ── STATUS BAND ── */}
      <div
        className="flex items-center justify-between flex-shrink-0"
        style={{ padding: '5px 12px', background: band.bg, borderBottom: `1px solid ${band.borderColor}` }}
      >
        <span className="flex items-center gap-1.5 font-bold uppercase"
          style={{ fontSize: 9, letterSpacing: '0.9px', color: band.labelColor }}>
          <Icon icon={band.icon as any} size={10} className={loading ? 'animate-spin' : undefined} />
          {/* Owner: the count was buried as plain text inside the 9px label —
              give the numeral its own larger, bold treatment so it's the
              thing the eye lands on, with "traces found" as a small caption. */}
          {bandState === 'has' ? (
            <span className="flex items-baseline gap-1">
              <span className="font-mono font-black" style={{ fontSize: 15, letterSpacing: 0, lineHeight: 1 }}>
                {count}
              </span>
              <span>{count === 1 ? 'trace found' : 'traces found'}</span>
            </span>
          ) : band.label}
        </span>
        <span className="flex items-center gap-0.5">
          {infoTooltip}
          {scheduleControl}
          {rescanButton}
        </span>
      </div>

      {/* ── BODY ── */}
      <div className="flex-1 overflow-hidden flex flex-col" style={{ padding: '9px 12px 6px', gap: 6 }}>

        {/* Title row: icon · title (larger) · optional standalone info */}
        <div className="flex items-center gap-2 flex-shrink-0">
          <div className="flex-shrink-0 flex items-center justify-center"
            style={{ width: 24, height: 24, borderRadius: 4, background: iconBg }}>
            <Icon icon={category.icon as any} size={12} style={{ color: category.color }} />
          </div>

          <div
            className="font-black uppercase truncate flex-1"
            style={{
              fontSize: 13,
              letterSpacing: '0.4px',
              lineHeight: 1.15,
              color: hasData ? category.color : 'var(--color-text-secondary)',
            }}
          >
            {category.label}
          </div>

        </div>

        {/* Preview / state content */}
        <div className="flex flex-col gap-0.5 flex-1 overflow-hidden">
          {loading && (
            <div className="flex flex-col gap-1.5 mt-0.5">
              <div className="wc-scan-bar w-full h-[3px] rounded-full"
                style={{ background: 'var(--color-bg-tertiary)' }}>
                <i style={{ background: category.color }} />
              </div>
              <span className="font-mono uppercase" style={{ fontSize: 8.5, letterSpacing: '0.6px', color: 'var(--color-text-muted)' }}>
                Scanning for traces…
              </span>
            </div>
          )}
          {!loading && hasData && (
            <AnimatePresence>
              <div className="flex flex-col gap-0.5">
                {preview.slice(0, 3).map((item, i) => (
                  <motion.span
                    key={`${category.id}-${i}-${item}`}
                    initial={{ opacity: 1, x: 0 }}
                    exit={{ opacity: 0, x: -16, transition: { duration: 0.25 } }}
                    className="font-mono truncate"
                    style={{ fontSize: 8.5, padding: '2px 6px', borderRadius: 2,
                      background: 'var(--color-bg-tertiary)', color: 'var(--color-text-muted)' }}
                    title={item}
                  >
                    {item}
                  </motion.span>
                ))}
              </div>
            </AnimatePresence>
          )}
          {error && !loading && (
            <div className="font-mono truncate"
              style={{ fontSize: 8.5, padding: '2px 6px', borderRadius: 2,
                background: 'var(--color-danger-dim)', color: 'var(--color-danger)' }}>
              {error}
            </div>
          )}
          {/* Owner: manual-only view-only cards (Process Review) were stuck at
              "Not scanned" with only a tiny unlabeled header icon as the trigger.
              Surface an obvious, labeled Scan CTA in the body instead. */}
          {!loading && !error && isNotLoaded && onLoad && (
            <div className="flex-1 flex flex-col items-center justify-center gap-2" style={{ padding: '4px 0' }}>
              <span className="text-center" style={{ fontSize: 9, color: 'var(--color-text-muted)' }}>
                Not scanned yet
              </span>
              <button
                type="button"
                onClick={e => { e.stopPropagation(); onLoad(); }}
                className="flex items-center gap-1.5 rounded transition-colors duration-150"
                style={{ height: 26, padding: '0 12px', border: '1px solid var(--color-border)',
                  background: 'var(--color-bg-tertiary)', cursor: 'pointer',
                  fontSize: 10, fontWeight: 700, letterSpacing: '0.5px', textTransform: 'uppercase',
                  color: 'var(--color-text-primary)' }}
                onMouseEnter={e => {
                  e.currentTarget.style.color = 'var(--color-accent)';
                  e.currentTarget.style.borderColor = 'var(--color-border-light)';
                  e.currentTarget.style.background = 'var(--color-bg-elevated)';
                }}
                onMouseLeave={e => {
                  e.currentTarget.style.color = 'var(--color-text-primary)';
                  e.currentTarget.style.borderColor = 'var(--color-border)';
                  e.currentTarget.style.background = 'var(--color-bg-tertiary)';
                }}
              >
                <Icon icon="search" size={11} color="currentColor" />
                Scan
              </button>
            </div>
          )}
          {!loading && hasData && category.regeneratesNote && (
            <div
              title={category.regeneratesNote}
              className="flex items-center gap-1 truncate"
              style={{ fontSize: 8, color: 'var(--color-text-muted)', opacity: 0.7, cursor: 'help', marginTop: 1 }}
            >
              <Icon icon="info-sign" size={8} color="currentColor" />
              <span className="truncate">Regenerates after clear</span>
            </div>
          )}
        </div>
      </div>

      {/* ── FOOTER — only when there's something actionable ── */}
      {showFooter && (
        <div
          className="flex items-center gap-1.5 flex-shrink-0"
          style={{ padding: '5px 10px', background: 'var(--color-bg-tertiary)',
            borderTop: '1px solid var(--color-border)' }}
        >
          {/* Details — only when hasData */}
          {!isActionOnly && onViewDetails && hasData && (
            <button
              type="button"
              onClick={e => { e.stopPropagation(); onViewDetails(); }}
              className="flex items-center gap-1 rounded flex-1 justify-center transition-colors duration-150"
              style={{ height: 24, padding: '0 8px', border: '1px solid var(--color-border)',
                background: 'transparent', cursor: 'pointer',
                fontSize: 9, fontWeight: 700, letterSpacing: '0.5px', textTransform: 'uppercase',
                color: 'var(--color-text-muted)' }}
              onMouseEnter={e => {
                e.currentTarget.style.color = 'var(--color-text-primary)';
                e.currentTarget.style.borderColor = 'var(--color-border-light)';
                e.currentTarget.style.background = 'var(--color-bg-elevated)';
              }}
              onMouseLeave={e => {
                e.currentTarget.style.color = 'var(--color-text-muted)';
                e.currentTarget.style.borderColor = 'var(--color-border)';
                e.currentTarget.style.background = 'transparent';
              }}
            >
              <Icon icon="eye-open" size={10} color="currentColor" />
              Details
            </button>
          )}

          {/* Clear — only when hasData */}
          {!isActionOnly && hasClear && hasData && (
            onClearAllUsers ? (
              <Popover
                minimal placement="bottom-end"
                disabled={clearing || clearDisabled}
                content={
                  <Menu>
                    <MenuItem icon="trash" text={`Clear for ${clearScopeLabel ?? 'this user'}`}
                      disabled={clearDisabled} onClick={onClear} />
                    <MenuItem icon="people" text="Clear for all users" intent="danger"
                      disabled={clearDisabled} onClick={onClearAllUsers} />
                  </Menu>
                }
              >
                <ClearButton clearing={clearing} disabled={!!clearDisabled} isMenu onClear={onClear} />
              </Popover>
            ) : (
              <ClearButton
                clearing={clearing} disabled={!!clearDisabled}
                onClear={e => { e.stopPropagation(); onClear(); }}
              />
            )
          )}

          {/* Run — action-only full width */}
          {isActionOnly && (
            <button
              type="button"
              disabled={clearing || clearDisabled}
              onClick={e => { e.stopPropagation(); onClear(); }}
              className="flex items-center justify-center gap-1 w-full rounded transition-colors duration-150
                disabled:opacity-40 disabled:cursor-not-allowed"
              style={{ height: 24, border: '1px solid var(--color-border)', background: 'transparent',
                cursor: 'pointer', fontSize: 9, fontWeight: 700, letterSpacing: '0.5px',
                textTransform: 'uppercase', color: 'var(--color-text-muted)' }}
              onMouseEnter={e => {
                e.currentTarget.style.color = 'var(--color-text-primary)';
                e.currentTarget.style.borderColor = 'var(--color-border-light)';
              }}
              onMouseLeave={e => {
                e.currentTarget.style.color = 'var(--color-text-muted)';
                e.currentTarget.style.borderColor = 'var(--color-border)';
              }}
            >
              <Icon icon="play" size={10} color="currentColor" />
              {clearing ? 'Running…' : 'Run'}
            </button>
          )}
        </div>
      )}

      {/* Particle burst */}
      <AnimatePresence>
        {showParticles && (
          <>
            {[...Array(8)].map((_, i) => (
              <motion.div key={`p-${i}`}
                initial={{ opacity: 1, scale: 1, x: 0, y: 0 }}
                animate={{ opacity: 0, scale: 0.3, x: (i % 2 === 0 ? 1 : -1) * (20 + i * 8), y: -25 - i * 4 }}
                transition={{ duration: 0.6, delay: i * 0.04, ease: "easeOut" }}
                onAnimationComplete={() => { if (i === 7) setShowParticles(false); }}
                className="absolute rounded-full pointer-events-none"
                style={{ width: 6, height: 6, backgroundColor: category.color,
                  left: `${15 + i * 9}%`, top: '50%' }}
              />
            ))}
            <motion.div
              initial={{ opacity: 0 }} animate={{ opacity: [0, 0.3, 0] }} transition={{ duration: 0.5 }}
              className="absolute inset-0 rounded-lg pointer-events-none"
              style={{ backgroundColor: category.color }}
            />
          </>
        )}
      </AnimatePresence>
    </motion.div>
  );
}

// ── ClearButton — styled button with red hover, reused for single and menu trigger ──
function ClearButton({
  clearing, disabled, onClear, isMenu,
}: {
  clearing: boolean;
  disabled: boolean;
  onClear?: (e: React.MouseEvent<HTMLButtonElement>) => void;
  isMenu?: boolean;
}) {
  return (
    <button
      type="button"
      disabled={clearing || disabled}
      onClick={onClear}
      className="flex items-center justify-center gap-1 flex-1 rounded transition-colors duration-150
        disabled:opacity-40 disabled:cursor-not-allowed"
      style={{ height: 24, padding: '0 8px', border: '1px solid var(--color-border)',
        background: 'transparent', cursor: 'pointer',
        fontSize: 9, fontWeight: 700, letterSpacing: '0.5px', textTransform: 'uppercase',
        color: 'var(--color-text-muted)' }}
      onMouseEnter={e => {
        e.currentTarget.style.color = 'var(--color-danger)';
        e.currentTarget.style.borderColor = 'color-mix(in srgb, var(--color-danger) 45%, transparent)';
        e.currentTarget.style.background = 'var(--color-danger-dim)';
      }}
      onMouseLeave={e => {
        e.currentTarget.style.color = 'var(--color-text-muted)';
        e.currentTarget.style.borderColor = 'var(--color-border)';
        e.currentTarget.style.background = 'transparent';
      }}
    >
      <Icon icon="trash" size={10} color="currentColor" />
      {clearing ? 'Clearing…' : 'Clear'}
      {isMenu && <Icon icon="caret-down" size={10} color="currentColor" />}
    </button>
  );
}
