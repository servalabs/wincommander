// Durable journal for package-manager work.  The actual winget child process is
// owned by Windows, not this renderer, so a renderer restart must never pretend
// that a previously-running command will resume.  We retain it as interrupted
// instead, while completed and failed work remains useful history.
import { useSyncExternalStore } from "react";

export type PackageActivityKind = "install" | "update";
export type PackageActivityStatus = "queued" | "running" | "completed" | "failed" | "interrupted";

export interface PackageActivity {
  id: string;
  packageId: string;
  label: string;
  kind: PackageActivityKind;
  status: PackageActivityStatus;
  createdAt: number;
  startedAt?: number;
  finishedAt?: number;
  error?: string;
}

const STORAGE_KEY = "wc-package-activity-v1";
const MAX_HISTORY = 100;
const listeners = new Set<() => void>();

function read(): PackageActivity[] {
  try {
    const parsed: unknown = JSON.parse(localStorage.getItem(STORAGE_KEY) || "[]");
    if (!Array.isArray(parsed)) return [];
    return parsed.filter((item): item is PackageActivity =>
      !!item && typeof item === "object" && typeof (item as PackageActivity).id === "string"
    ).slice(0, MAX_HISTORY);
  } catch {
    return [];
  }
}

let activities = typeof window === "undefined" ? [] : read();
// An in-memory executor cannot survive a reload. Mark those entries honestly
// before rendering them; do not re-run package commands without a new click.
let recovered = false;
function recoverInterrupted(): void {
  if (recovered || typeof window === "undefined") return;
  recovered = true;
  let changed = false;
  activities = activities.map(entry => {
    if (entry.status !== "queued" && entry.status !== "running") return entry;
    changed = true;
    return { ...entry, status: "interrupted", finishedAt: Date.now(), error: "WinCommander was closed before this package task finished." };
  });
  if (changed) persist();
}

function persist(): void {
  try { localStorage.setItem(STORAGE_KEY, JSON.stringify(activities.slice(0, MAX_HISTORY))); } catch { /* storage is optional */ }
}
function notify(): void { listeners.forEach(listener => listener()); }
function mutate(fn: (previous: PackageActivity[]) => PackageActivity[]): void {
  recoverInterrupted();
  activities = fn(activities).slice(0, MAX_HISTORY);
  persist();
  notify();
}

export function recordPackageActivity(input: Pick<PackageActivity, "packageId" | "label" | "kind">): string {
  recoverInterrupted();
  const id = `package-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
  mutate(previous => [{ ...input, id, status: "queued", createdAt: Date.now() }, ...previous]);
  return id;
}

export function setPackageActivityStatus(id: string, status: PackageActivityStatus, error?: string): void {
  mutate(previous => previous.map(entry => entry.id !== id ? entry : {
    ...entry,
    status,
    ...(status === "running" ? { startedAt: Date.now() } : {}),
    ...(["completed", "failed", "interrupted"].includes(status) ? { finishedAt: Date.now() } : {}),
    ...(error ? { error } : {}),
  }));
}

export function getPackageActivities(): PackageActivity[] { recoverInterrupted(); return activities; }
export function clearFinishedPackageActivities(): void {
  mutate(previous => previous.filter(entry => entry.status === "queued" || entry.status === "running"));
}
export function subscribePackageActivities(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}
export function usePackageActivities(): PackageActivity[] {
  return useSyncExternalStore(subscribePackageActivities, getPackageActivities, () => []);
}
