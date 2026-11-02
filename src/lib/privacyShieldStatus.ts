import type { PrivacyShieldMode } from "./privacyShieldMode";

export interface VerifiedShieldStatus {
  running: boolean;
  activeMode?: PrivacyShieldMode;
  phase: "running" | "stopped";
}

export function verifiedShieldStatus(value: unknown): VerifiedShieldStatus | null {
  if (!value || typeof value !== "object") return null;
  const status = value as Record<string, unknown>;
  if ((status.phase !== "running" || status.running !== true)
    && (status.phase !== "stopped" || status.running !== false)) return null;
  return {
    running: status.running as boolean,
    phase: status.phase as VerifiedShieldStatus["phase"],
    activeMode: status.running && (status.activeMode === "blur_notify" || status.activeMode === "notify_only")
      ? status.activeMode : undefined,
  };
}

export function shouldProbeShieldCamera(lastProbeAt: number, now: number, force: boolean): boolean {
  return force || lastProbeAt === 0 || now - lastProbeAt >= 30_000;
}

// A verified lifecycle event supersedes every status request already in flight.
export class ShieldStatusGate {
  private revision = 0;
  private reading: number | null = null;
  private queued = false;
  private acting = false;
  private actionRevision: number | null = null;
  private actionReadsAllowed = false;
  private active = true;

  get actionPending(): boolean { return this.acting; }
  get isActive(): boolean { return this.active; }

  queueRead(): void { this.queued = true; }

  beginAction(): number | null {
    if (this.acting || !this.active) return null;
    this.acting = true;
    this.actionReadsAllowed = false;
    this.actionRevision = ++this.revision;
    return this.actionRevision;
  }

  canApplyAction(revision: number): boolean {
    return this.active && this.actionRevision === revision && this.revision === revision;
  }

  allowActionStatus(revision: number): void {
    if (this.actionRevision === revision) this.actionReadsAllowed = true;
  }

  endAction(revision: number): boolean {
    if (this.actionRevision !== revision) return false;
    this.acting = false;
    this.actionRevision = null;
    this.actionReadsAllowed = false;
    return this.active;
  }

  setActive(active: boolean): void {
    this.active = active;
    this.revision++;
  }

  beginRead(): number | null {
    if (!this.active) return null;
    if ((this.acting && !this.actionReadsAllowed) || this.reading !== null) {
      if (this.acting || this.reading !== this.revision) this.queued = true;
      return null;
    }
    this.queued = false;
    this.reading = this.revision;
    return this.reading;
  }

  canApply(revision: number): boolean {
    return this.active && (!this.acting || this.actionReadsAllowed) && revision === this.revision;
  }

  finishRead(revision: number): boolean {
    if (this.reading !== revision) return false;
    this.reading = null;
    if (!this.active || (this.acting && !this.actionReadsAllowed) || !this.queued) return false;
    this.queued = false;
    return true;
  }

  acceptEvent(value: unknown): VerifiedShieldStatus | null {
    if (!this.active) return null;
    const status = verifiedShieldStatus(value);
    if (!status) return null;
    this.revision++;
    this.queued = true;
    return status;
  }
}
