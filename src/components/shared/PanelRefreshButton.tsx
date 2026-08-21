// src/components/shared/PanelRefreshButton.tsx
//
// Manual refresh button for panel headers.
// Gives users a way to force-refresh data on demand without relying on polling.

import { Button, Tooltip } from "@/components/ui/bp";
import { useState, useCallback } from "react";

interface PanelRefreshButtonProps {
    /** Async function to call when refresh is clicked */
    onRefresh: () => Promise<void>;
    /** Tooltip text (default: "Refresh data") */
    tooltip?: string;
}

export default function PanelRefreshButton({ onRefresh, tooltip = "Refresh data" }: PanelRefreshButtonProps) {
    const [spinning, setSpinning] = useState(false);

    const handleClick = useCallback(async () => {
        if (spinning) return; // Prevent double-click
        setSpinning(true);
        try {
            await onRefresh();
        } finally {
            // Keep spin animation visible for a beat so user sees feedback
            setTimeout(() => setSpinning(false), 600);
        }
    }, [onRefresh, spinning]);

    return (
        <Tooltip content={tooltip} placement="bottom">
            <Button
                icon="refresh"
                minimal
                small
                onClick={handleClick}
                style={{
                    transition: 'transform 0.3s ease',
                    transform: spinning ? 'rotate(360deg)' : 'none',
                }}
            />
        </Tooltip>
    );
}
