import { useSyncExternalStore } from "react";
import {
  appendMaintenanceOperation, MAINTENANCE_HISTORY_KEY, parseMaintenanceHistory,
  type MaintenanceOperation, type MaintenanceOperationInput,
} from "../lib/maintenanceHistory";

let entries: MaintenanceOperation[] = [];
let initialized = false;
let persistent = true;
const listeners = new Set<() => void>();

function initialize() {
  if (initialized || typeof window === "undefined") return;
  initialized = true;
  try {
    entries = parseMaintenanceHistory(window.localStorage.getItem(MAINTENANCE_HISTORY_KEY));
  } catch {
    persistent = false;
  }
}

function publish() {
  try {
    window.localStorage.setItem(MAINTENANCE_HISTORY_KEY, JSON.stringify(entries));
    persistent = true;
  } catch {
    persistent = false;
  }
  listeners.forEach(listener => listener());
}

// Display history follows returned operation results; it isn't an authorization or evidence receipt.
export function recordMaintenanceOperation(input: MaintenanceOperationInput) {
  initialize();
  const entry: MaintenanceOperation = {
    ...input,
    title: input.title.trim().slice(0, 120),
    detail: input.detail?.trim().slice(0, 240),
    id: crypto.randomUUID(),
    recordedAt: new Date().toISOString(),
  };
  entries = appendMaintenanceOperation(entries, entry);
  publish();
}

function onStorage(event: StorageEvent) {
  if (event.key !== MAINTENANCE_HISTORY_KEY && event.key !== null) return;
  entries = parseMaintenanceHistory(event.newValue);
  listeners.forEach(listener => listener());
}

function subscribe(listener: () => void) {
  initialize();
  if (!listeners.size) window.addEventListener("storage", onStorage);
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
    if (!listeners.size) window.removeEventListener("storage", onStorage);
  };
}

function getSnapshot() { initialize(); return entries; }

export function useMaintenanceHistory() {
  const operations = useSyncExternalStore(subscribe, getSnapshot, getSnapshot);
  return {
    operations,
    isPersistent: persistent,
    clearHistory: () => { entries = []; publish(); },
  };
}
