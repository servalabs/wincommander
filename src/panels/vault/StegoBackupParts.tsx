// src/panels/vault/StegoBackupParts.tsx
//
// Presentational pieces for StegoBackupSection — no state, no backend. Split
// out to keep the section file inside the 300-line limit.

import type { ReactNode } from "react";
import { Button, Callout, Icon, ProgressBar, Spinner, Tooltip } from "@/components/ui/bp";
import EmptyState from "../../components/shared/EmptyState";
import { formatBytes, type CapacityPlan } from "../../lib/stegoBackup";
import type { StegoFailure, StegoField, StegoIssue } from "../../lib/stegoBackupValidation";

export const INFO = {
  what: "Steganographic backup: WinCommander builds an encrypted VeraCrypt volume and appends it to a normal video file. The result still plays in any player, so a copy on a phone or a cloud drive looks like an ordinary clip — but the hidden volume only opens with your password.",
  size: "The usable space inside the hidden volume. It is fixed at creation — to store more later you have to build a new one.",
  carrier: "The payload rides along inside the file, so a large hidden volume in a tiny clip produces a video whose file size makes no sense for its length. Roughly three times the payload keeps the file size believable.",
  password: "This password is the only key. There is no reset, no recovery file and no support route back in — lose it and the backup is unreadable forever.",
  restore: "Pulling the container out is a plain byte copy, so it needs no password. You enter the password later, when you mount the recovered container from the Volumes list above.",
};

export function InfoDot({ content }: { content: string }) {
  return (
    <Tooltip content={content}>
      <Icon icon="info-sign" size={11} className="stego-info-dot" />
    </Tooltip>
  );
}

export function IssueLine({
  issues,
  field,
  tone = "error",
}: {
  issues: StegoIssue[];
  field: StegoField;
  tone?: "error" | "warn";
}) {
  const issue = issues.find((candidate) => candidate.field === field);
  if (!issue) return null;
  return <span className={`stego-issue stego-issue--${tone}`}>{issue.message}</span>;
}

export function FilePick({
  label,
  value,
  onPick,
  onClear,
  disabled,
}: {
  label: string;
  value: string;
  onPick: () => void;
  onClear: () => void;
  disabled?: boolean;
}) {
  const name = value.replace(/\//g, "\\").split("\\").pop() ?? "";
  return (
    <div className="stego-pick">
      <Button minimal small icon="folder-open" onClick={onPick} disabled={disabled}>
        {label}
      </Button>
      {value ? (
        <>
          <span className="stego-pick__name" title={value}>
            {name}
          </span>
          <span className="stego-pick__dir" title={value}>
            {value.slice(0, Math.max(0, value.length - name.length))}
          </span>
          <Button minimal small icon="cross" aria-label={`Clear ${label}`} onClick={onClear} disabled={disabled} />
        </>
      ) : (
        <span className="stego-pick__empty">Nothing chosen</span>
      )}
    </div>
  );
}

export function CapacityPanel({
  plan,
  freeBytes,
  loading,
  hasOutput,
  freeShare,
}: {
  plan: CapacityPlan;
  freeBytes: number | null;
  loading: boolean;
  hasOutput: boolean;
  freeShare: number | null;
}) {
  return (
    <div className="stego-capacity">
      <div className="stego-capacity__row">
        <span>Hidden volume</span>
        <b>{formatBytes(plan.containerBytes)}</b>
      </div>
      <div className="stego-capacity__row">
        <span>
          Carrier should be <InfoDot content={INFO.carrier} />
        </span>
        <b>{formatBytes(plan.minimumCarrierBytes)} or larger</b>
      </div>
      <div className="stego-capacity__row">
        <span>Output video</span>
        <b>carrier + {formatBytes(plan.containerBytes)}</b>
      </div>
      <div className="stego-capacity__row">
        <span>Room at the destination</span>
        {loading ? (
          <b className="stego-capacity__muted">
            <Spinner size={11} /> checking…
          </b>
        ) : freeBytes != null ? (
          <b>{formatBytes(freeBytes)} free</b>
        ) : (
          <b className="stego-capacity__muted">{hasOutput ? "unknown" : "pick an output file"}</b>
        )}
      </div>
      {freeShare != null && (
        <ProgressBar
          value={Math.min(freeShare, 1)}
          intent={freeShare > 1 ? "danger" : freeShare > 0.8 ? "warning" : "primary"}
        />
      )}
    </div>
  );
}

export function CapacityEmpty() {
  return <EmptyState compact title="Choose a carrier video to see how the sizes work out." />;
}

export function BusyBar({ label }: { label: string }) {
  return (
    <div className="stego-busy">
      <ProgressBar intent="primary" />
      <span>{label}</span>
    </div>
  );
}

export function FailureCallout({ failure }: { failure: StegoFailure }) {
  return (
    <Callout intent="danger" title={failure.headline}>
      <p className="stego-outcome__hint">{failure.hint}</p>
      <details className="stego-details">
        <summary>Engine message</summary>
        <pre>{failure.raw}</pre>
      </details>
    </Callout>
  );
}

export function SuccessCallout({
  title,
  path,
  children,
  onReveal,
}: {
  title: string;
  path: string;
  children: ReactNode;
  onReveal: () => void;
}) {
  return (
    <Callout intent="success" title={title}>
      <p className="stego-outcome__hint">{children}</p>
      <code className="stego-outcome__path">{path}</code>
      <Button small minimal icon="folder-open" onClick={onReveal}>
        Open containing folder
      </Button>
    </Callout>
  );
}
