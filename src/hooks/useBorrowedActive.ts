// src/hooks/useBorrowedActive.ts
//
// Returns true while Borrowed Mode (panel-lock) is active.
// Derives from persisted settings (lockedPanelIds.length > 0) so edits made
// via the VisibilityTable are reflected immediately without re-toggling.
// The keyword pair typed in GlobalCommandPalette ("hidden-panels-lock" /
// "hidden-panels-unlock") sets a session override that takes priority until
// the user types the opposite keyword or navigates away.

import { useState, useEffect } from "react";
import { useAppState } from "../context/AppContext";

export default function useBorrowedActive(): boolean {
    const { appSettings } = useAppState();
    // null = no keyword typed this session; true/false = explicit keyword override.
    // KT: session override decouples the keyword-toggle UX from the persisted
    // config so editing the visibility table takes effect without re-typing keywords.
    const [sessionActive, setSessionActive] = useState<boolean | null>(null);

    useEffect(() => {
        const onLock = () => setSessionActive(true);
        const onUnlock = () => setSessionActive(false);
        window.addEventListener("hidden-panels-lock", onLock);
        window.addEventListener("hidden-panels-unlock", onUnlock);
        return () => {
            window.removeEventListener("hidden-panels-lock", onLock);
            window.removeEventListener("hidden-panels-unlock", onUnlock);
        };
    }, []);

    // Keyword override wins when present; otherwise derive reactively from settings.
    if (sessionActive !== null) return sessionActive;
    return (appSettings?.app?.lockedPanelIds?.length ?? 0) > 0;
}
