import useProcessElevation from "../../hooks/useProcessElevation";

export default function ProcessElevationStatus() {
    const { state, retry } = useProcessElevation();

    const label = {
        checking: "Checking privileges…",
        administrator: "Administrator (elevated)",
        standard: "Standard (not elevated)",
        unknown: "Privileges unavailable",
    }[state];

    return <div className="dgz-tile" aria-label="Current app privileges">
        <div className="dgz-tile-row">
            <div className="dgz-tile-body">
                <div className="dgz-tile-title">Current app privileges</div>
                <div role="status" aria-live="polite">{label}</div>
                <div className="dgz-tile-desc">
                    {state === "unknown" ? "Windows could not confirm this app’s privileges. Try checking again."
                        : "Shows how this running WinCommander window was opened. An administrator account can also open the app without elevation."}
                </div>
            </div>
            {state === "unknown" && <button type="button" className="dgz-autostart-retry" onClick={retry}>Check again</button>}
        </div>
    </div>;
}
