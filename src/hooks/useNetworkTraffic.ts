import { useEffect, useSyncExternalStore } from "react";
import { listen } from "@tauri-apps/api/event";
import { showError } from "../utils/toast";

export interface NetSample {
  upBytesPerSec: number;
  downBytesPerSec: number;
}

interface MetricAlertEvent {
  metric: string;
  label: string;
  value: number;
  unit: string;
  threshold: number;
}

export interface NetworkTrafficSnapshot {
  sample: NetSample;
  upHistory: number[];
  downHistory: number[];
}

export const NETWORK_TRAFFIC_HISTORY = 30;

const EMPTY_SAMPLE: NetSample = { upBytesPerSec: 0, downBytesPerSec: 0 };
let snapshot: NetworkTrafficSnapshot = {
  sample: EMPTY_SAMPLE,
  upHistory: [],
  downHistory: [],
};
const subscribers = new Set<() => void>();
let listenerStarted = false;
let listenerStartPromise: Promise<void> | null = null;
let lastToastAt = 0;

function notify() {
  for (const subscriber of subscribers) subscriber();
}

function pushHistory(history: number[], value: number): number[] {
  const next = [...history, value];
  return next.length > NETWORK_TRAFFIC_HISTORY
    ? next.slice(next.length - NETWORK_TRAFFIC_HISTORY)
    : next;
}

export function recordNetworkTrafficSample(sample: NetSample) {
  snapshot = {
    sample,
    upHistory: pushHistory(snapshot.upHistory, sample.upBytesPerSec),
    downHistory: pushHistory(snapshot.downHistory, sample.downBytesPerSec),
  };
  notify();
}

export function getNetworkTrafficSnapshot(): NetworkTrafficSnapshot {
  return snapshot;
}

function subscribeNetworkTraffic(callback: () => void) {
  subscribers.add(callback);
  return () => {
    subscribers.delete(callback);
  };
}

export function useNetworkTraffic(): NetworkTrafficSnapshot {
  return useSyncExternalStore(
    subscribeNetworkTraffic,
    getNetworkTrafficSnapshot,
    getNetworkTrafficSnapshot,
  );
}

export function startNetworkTrafficListener(): Promise<void> {
  if (listenerStarted) return listenerStartPromise ?? Promise.resolve();

  listenerStarted = true;
  // KT: This listener is app-level, not dashboard-card-owned. The Rust sampler
  // emits every second for speed readouts and metric alerts; unmounting the
  // Dashboard must not stop Upload/Download monitoring or toast delivery.
  listenerStartPromise = Promise.all([
    listen<NetSample>("metrics://network", (event) => {
      recordNetworkTrafficSample(event.payload);
    }),
    listen<MetricAlertEvent>("metrics://metric-alert", (event) => {
      const now = Date.now();
      if (now - lastToastAt < 3000) return;
      lastToastAt = now;
      const { label, value, unit, threshold } = event.payload;
      showError(`${label} hit ${value.toFixed(1)}${unit} (limit ${threshold.toFixed(0)}${unit})`);
    }),
  ]).then(() => undefined).catch((error: unknown) => {
    listenerStarted = false;
    listenerStartPromise = null;
    throw error;
  });

  return listenerStartPromise;
}

export function useNetworkTrafficListener() {
  useEffect(() => {
    void startNetworkTrafficListener().catch(() => undefined);
  }, []);
}

export function resetNetworkTrafficForTests() {
  snapshot = {
    sample: EMPTY_SAMPLE,
    upHistory: [],
    downHistory: [],
  };
  lastToastAt = 0;
  notify();
}
