import { useCallback, useEffect, useRef, useState } from "react";
import useBackend from "@/hooks/useBackend";
import type { ActivityArea, ActivityItem, ActivityRecordPage, ActivityRemoveResult, ActivityScan, ActivitySource } from "@/types/generated/activity";
import { ACTIVITY_RECORD_SELECTION_LIMIT, appendActivityRecords, createActivityReview, type ActivityReview } from "./activityReview";

export function useMaintenanceActivity(initialArea: ActivityArea, readOnly = false) {
    const backend = useBackend();
    const backendRef = useRef(backend); backendRef.current = backend;
    const [area, setArea] = useState<ActivityArea>(initialArea);
    const [scan, setScan] = useState<ActivityScan | null>(null);
    const [scanning, setScanning] = useState(false);
    const [error, setError] = useState("");
    const [inspected, setInspected] = useState<{ source: ActivitySource; item: ActivityItem } | null>(null);
    const [page, setPage] = useState<ActivityRecordPage | null>(null);
    const [recordsLoading, setRecordsLoading] = useState(false);
    const [recordsError, setRecordsError] = useState("");
    const [selected, setSelected] = useState<Set<string>>(new Set());
    const [review, setReview] = useState<ActivityReview | null>(null);
    const [removing, setRemoving] = useState(false);
    const [result, setResult] = useState<ActivityRemoveResult | null>(null);
    const [resultLabel, setResultLabel] = useState("");
    const [operationError, setOperationError] = useState("");
    const mounted = useRef(true), scanRequest = useRef(0), recordRequest = useRef(0), nextOffset = useRef(0), mutationPending = useRef(false);

    const resetInspection = useCallback(() => {
        recordRequest.current++; nextOffset.current = 0;
        setInspected(null); setPage(null); setSelected(new Set()); setRecordsLoading(false); setRecordsError(""); setReview(null);
    }, []);

    const scanArea = useCallback(async (requested: ActivityArea, preserveResult = false) => {
        const request = ++scanRequest.current;
        if (mounted.current) { setScanning(true); setError(""); setScan(null); resetInspection(); if (!preserveResult) { setResult(null); setOperationError(""); } }
        try {
            const response = await backendRef.current.activityScan(requested);
            if (mounted.current && request === scanRequest.current) setScan(response);
        } catch (cause) { if (mounted.current && request === scanRequest.current) setError(String(cause)); }
        finally { if (mounted.current && request === scanRequest.current) setScanning(false); }
    }, [resetInspection]);

    const invalidateRequests = useCallback(() => { scanRequest.current++; recordRequest.current++; }, []);
    useEffect(() => { mounted.current = true; return () => { mounted.current = false; invalidateRequests(); }; }, [invalidateRequests]);
    useEffect(() => { if (!mutationPending.current) void scanArea(area); }, [area, scanArea]);

    async function inspect(source: ActivitySource, item: ActivityItem) {
        if (!scan || scanning || mutationPending.current) return;
        resetInspection(); setInspected({ source, item });
        if (item.status !== "ready" || !item.canSelectRecords || item.count <= 0) return;
        const request = ++recordRequest.current;
        setRecordsLoading(true);
        try {
            const response = await backendRef.current.activityRecords(area, scan.scanId, item.id, 0);
            if (mounted.current && request === recordRequest.current) {
                setPage(appendActivityRecords(null, response, scan.scanId, item.id, 0));
                nextOffset.current = response.offset + response.records.length;
            }
        } catch (cause) { if (mounted.current && request === recordRequest.current) setRecordsError(String(cause)); }
        finally { if (mounted.current && request === recordRequest.current) setRecordsLoading(false); }
    }

    async function loadMore() {
        if (!scan || !inspected || !page?.hasMore || recordsLoading || mutationPending.current) return;
        const request = ++recordRequest.current, offset = nextOffset.current;
        setRecordsLoading(true); setRecordsError("");
        try {
            const response = await backendRef.current.activityRecords(area, scan.scanId, inspected.item.id, offset);
            if (mounted.current && request === recordRequest.current) {
                setPage(appendActivityRecords(page, response, scan.scanId, inspected.item.id, offset));
                nextOffset.current = response.offset + response.records.length;
            }
        } catch (cause) { if (mounted.current && request === recordRequest.current) setRecordsError(String(cause)); }
        finally { if (mounted.current && request === recordRequest.current) setRecordsLoading(false); }
    }

    function openReview(source: ActivitySource, item: ActivityItem, allRecords: boolean) {
        if (readOnly || !scan || scanning || mutationPending.current) return;
        try { setReview(createActivityReview(area, scan, source.id, item.id, inspected?.item.id === item.id ? page?.records ?? [] : [], selected, allRecords)); }
        catch (cause) { setError(String(cause)); }
    }

    async function confirmRemoval() {
        if (readOnly || !review || !scan || review.scanId !== scan.scanId || mutationPending.current) return;
        mutationPending.current = true; setRemoving(true); setError(""); setOperationError("");
        const plan = review;
        try {
            const response = await backendRef.current.activityRemove(plan.area, plan.scanId, plan.itemId, plan.recordIds, plan.allRecords);
            if (mounted.current) { setResult(response); setResultLabel(plan.itemLabel); }
        } catch (cause) { if (mounted.current) setOperationError(`Removal could not be confirmed. No retry was attempted. ${String(cause)}`); }
        finally {
            // A failed response can still follow a partial write; discard every old action ID.
            if (mounted.current) resetInspection();
            await scanArea(plan.area, true);
            mutationPending.current = false;
            if (mounted.current) setRemoving(false);
        }
    }

    return { area, readOnly, scan, scanning, error, inspected, page, recordsLoading, recordsError, selected, review, removing, result, resultLabel, operationError,
        refresh: () => { if (!mutationPending.current) void scanArea(area); },
        changeArea: (next: ActivityArea) => { if (!mutationPending.current && next !== area) { resetInspection(); setScan(null); setResult(null); setError(""); setScanning(true); setArea(next); } },
        inspect, loadMore, openReview, confirmRemoval, closeInspection: resetInspection,
        cancelReview: () => { if (!mutationPending.current) setReview(null); },
        toggleRecord: (id: string) => {
            if (readOnly || mutationPending.current) return;
            if (!selected.has(id) && selected.size >= ACTIVITY_RECORD_SELECTION_LIMIT) { setRecordsError(`Select up to ${ACTIVITY_RECORD_SELECTION_LIMIT} individual records at a time.`); return; }
            setSelected(current => { const next = new Set(current); if (next.has(id)) next.delete(id); else next.add(id); return next; });
        },
        clearSelection: () => { setSelected(new Set()); setRecordsError(""); },
        selectLoaded: () => { if (!readOnly && !mutationPending.current && inspected?.item.canSelectRecords) { setRecordsError(""); setSelected(new Set(page?.records.slice(0, ACTIVITY_RECORD_SELECTION_LIMIT).map(record => record.id) ?? [])); } },
    };
}

export type MaintenanceActivity = ReturnType<typeof useMaintenanceActivity>;
