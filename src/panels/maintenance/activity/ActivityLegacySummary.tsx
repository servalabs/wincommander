import { useState } from "react";
import { Button, Icon } from "@/components/ui/bp";
import useBackend, { type BrowserFootprintBrowser } from "@/hooks/useBackend";

export default function ActivityLegacySummary() {
    const { getBrowserFootprints } = useBackend();
    const [sources, setSources] = useState<BrowserFootprintBrowser[] | null>(null);
    const [loading, setLoading] = useState(false);
    const [error, setError] = useState("");
    async function inspect() {
        if (loading) return;
        setLoading(true); setError("");
        try {
            const response = await getBrowserFootprints();
            if (response.success && response.data) setSources(Array.isArray(response.data.browsers) ? response.data.browsers : []);
            else setError(response.error ?? "Browser summary unavailable.");
        } catch (cause) { setError(String(cause)); }
        finally { setLoading(false); }
    }
    return <section className="activity-legacy" aria-label="Read-only browser summary"><div><Icon icon="eye-open" size={20} /><strong>Browser summary</strong></div>
        <p>The existing browser viewer can show discovered artifact types for the Windows user running WinCommander. It does not provide individual records or clearing controls.</p>
        <Button small outlined loading={loading} disabled={loading} onClick={() => void inspect()}>{sources ? "Refresh summary" : "Show available browser summary"}</Button>
        {error && <p role="alert" className="activity-error">{error}</p>}
        {sources?.map((source, index) => <details key={`${source.browser}-${source.profilePath}-${index}`}><summary>{source.browser} · {source.artifacts.length} artifact types</summary><p className="activity-profile-location">{source.profilePath || "Location unavailable"}</p><ul>{source.artifacts.map((artifact, artifactIndex) => <li key={`${artifact.name}-${artifactIndex}`}>{artifact.name}</li>)}</ul></details>)}
        {sources && !sources.length && <p>No browser artifacts were returned by the existing viewer.</p>}
    </section>;
}
