// src/hooks/useSovereigntyScore.ts
//
// ═══════════════════════════════════════════════════════════════════════
// REGISTRY-DRIVEN SCORE — Privacy Score + Cleanup Score
// ═══════════════════════════════════════════════════════════════════════
//
// BEFORE: 90+ lines of hardcoded signal checks with values that conflicted
//         with the registry, the docs, and each other.
// AFTER:  One loop over ALL_TOGGLES. Score values live in the registry,
//         this hook just sums them. Zero manual wiring.
//
// Privacy Score  (100 pts total) — all users
//   4 categories: telemetry, surface, hardening, capabilities
//   ALL categories scored uniformly from the registry via privacyScore/privacyScoreCategory.
//   Capability toggles use checkedWhen: "Deny" for string comparison against settings.json.
//
// Cleanup Score (100 pts max) — cleanup mode only
//   3 categories: traces, memory, behavior
//   Only persistent toggle states. Never one-shot cleaners.

import { useEffect, useRef } from "react";
import { useAppState } from "../context/AppContext";
import { getByPath } from "../types/toggles";
import { ALL_TOGGLES } from "../registry";
import { playSound } from "../utils/sound";

export interface CategoryScore {
    score: number;
    max: number;
    label: string;
}

export interface SovereigntyBreakdown {
    telemetry: CategoryScore;
    surface: CategoryScore;
    hardening: CategoryScore;
    capabilities: CategoryScore;
}

export interface CleanupBreakdown {
    traces: CategoryScore;
    memory: CategoryScore;
    behavior: CategoryScore;
}

export interface SovereigntyScore {
    total: number;
    isArmed: boolean;
    color: string;
    breakdown: SovereigntyBreakdown;
    cleanupTotal: number;
    cleanupEnabled: boolean;
    cleanupBreakdown: CleanupBreakdown;
}

const THRESHOLDS = [30, 50, 70, 90];
const MAX_PROTECTION_SCORE = 95;

function getColor(score: number): string {
    if (score >= 90) return "var(--color-success)";
    if (score >= 60) return "var(--color-accent)";
    if (score >= 30) return "var(--color-warning)";
    return "var(--color-danger)";
}

// ── Pre-compute max values from registry (static, never changes) ────
const privacyMaxByCategory: Record<string, number> = { telemetry: 0, surface: 0, hardening: 0, capabilities: 0 };
const cleanupMaxByCategory: Record<string, number> = { traces: 0, memory: 0, behavior: 0 };

for (const t of ALL_TOGGLES) {
    if (t.privacyScore && t.privacyScoreCategory) {
        privacyMaxByCategory[t.privacyScoreCategory] = (privacyMaxByCategory[t.privacyScoreCategory] ?? 0) + t.privacyScore;
    }
    if (t.cleanupScore && t.cleanupScoreCategory) {
        cleanupMaxByCategory[t.cleanupScoreCategory] = (cleanupMaxByCategory[t.cleanupScoreCategory] ?? 0) + t.cleanupScore;
    }
}

/** Check if a toggle is active, respecting checkedWhen for capability toggles. */
function isToggleActive(appSettings: unknown, t: { currentPath: string; checkedWhen?: string }): boolean {
    const raw = getByPath(appSettings, t.currentPath);
    if (t.checkedWhen !== undefined) return raw === t.checkedWhen;
    return Boolean(raw);
}

export function useSovereigntyScore(): SovereigntyScore {
    const {
        appSettings,
        loading,
    } = useAppState();

    const lastThresholdRef = useRef<number>(-1);
    const isInitialLoad = useRef(true);

    const isLoading = loading?.hardening || loading?.privacy || loading?.network || loading?.dashboard;

    // ── Registry-driven Privacy Score ───────────────────────────────────
    // One loop: for each toggle with privacyScore, check its currentPath
    // in appSettings. If active → add points. Capabilities use checkedWhen
    // to compare "Deny" strings. No manual signal list.
    const privacyByCategory: Record<string, number> = { telemetry: 0, surface: 0, hardening: 0, capabilities: 0 };

    for (const t of ALL_TOGGLES) {
        if (!t.privacyScore || !t.privacyScoreCategory) continue;
        if (isToggleActive(appSettings, t)) {
            privacyByCategory[t.privacyScoreCategory] = (privacyByCategory[t.privacyScoreCategory] ?? 0) + t.privacyScore;
        }
    }

    const score_telemetry = privacyByCategory.telemetry;
    const score_surface = privacyByCategory.surface;
    const score_hardening = privacyByCategory.hardening;
    const score_caps = privacyByCategory.capabilities;

    const raw = score_telemetry + score_surface + score_hardening + score_caps;
    // KT: Keep protection score shy of 100 by product rule.
    const total = Math.min(MAX_PROTECTION_SCORE, Math.max(0, raw));
    const isArmed = total >= 80;

    // ── Registry-driven Cleanup Score ──────────────────────────────────
    const cleanupEnabled = appSettings?.app?.privacyCleanEnabled ?? false;
    const cleanupByCategory: Record<string, number> = { traces: 0, memory: 0, behavior: 0 };

    for (const t of ALL_TOGGLES) {
        if (!t.cleanupScore || !t.cleanupScoreCategory) continue;
        if (isToggleActive(appSettings, t)) {
            cleanupByCategory[t.cleanupScoreCategory] = (cleanupByCategory[t.cleanupScoreCategory] ?? 0) + t.cleanupScore;
        }
    }

    const cleanupTotal = Math.min(100, Math.max(0,
        cleanupByCategory.traces + cleanupByCategory.memory + cleanupByCategory.behavior
    ));

    // Threshold sound — fires once per upward crossing, silent on cold load
    useEffect(() => {
        if (isLoading) return;
        if (isInitialLoad.current) {
            const crossed = THRESHOLDS.filter(t => total >= t);
            lastThresholdRef.current = crossed.length > 0 ? crossed[crossed.length - 1] : -1;
            isInitialLoad.current = false;
            return;
        }
        const crossed = THRESHOLDS.filter(t => total >= t);
        const highest = crossed.length > 0 ? crossed[crossed.length - 1] : -1;
        if (highest > lastThresholdRef.current) {
            lastThresholdRef.current = highest;
            playSound("threshold");
        } else if (highest < lastThresholdRef.current) {
            lastThresholdRef.current = highest;
        }
    }, [total, isLoading]);

    return {
        total,
        isArmed,
        color: getColor(total),
        breakdown: {
            telemetry:    { score: score_telemetry, max: privacyMaxByCategory.telemetry, label: "Core Telemetry" },
            surface:      { score: score_surface,   max: privacyMaxByCategory.surface,   label: "Surface Tracking" },
            hardening:    { score: score_hardening,  max: privacyMaxByCategory.hardening, label: "Active Hardening" },
            capabilities: { score: score_caps,       max: privacyMaxByCategory.capabilities, label: "App Capabilities" },
        },
        cleanupTotal,
        cleanupEnabled,
        cleanupBreakdown: {
            traces:   { score: cleanupByCategory.traces,   max: cleanupMaxByCategory.traces,   label: "Execution Traces" },
            memory:   { score: cleanupByCategory.memory,   max: cleanupMaxByCategory.memory,   label: "Memory Artifacts" },
            behavior: { score: cleanupByCategory.behavior, max: cleanupMaxByCategory.behavior, label: "Activity Trail" },
        },
    };
}
