import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

export type ProcessElevationState = "checking" | "administrator" | "standard" | "unknown";

export default function useProcessElevation() {
    const [state, setState] = useState<ProcessElevationState>("checking");
    const [attempt, setAttempt] = useState(0);

    useEffect(() => {
        let active = true;
        void invoke<unknown>("is_current_process_elevated").then(elevated => {
            if (active) setState(elevated === true ? "administrator" : elevated === false ? "standard" : "unknown");
        }).catch(() => { if (active) setState("unknown"); });
        return () => { active = false; };
    }, [attempt]);

    const retry = useCallback(() => {
        setState("checking");
        setAttempt(value => value + 1);
    }, []);

    return { state, retry };
}
