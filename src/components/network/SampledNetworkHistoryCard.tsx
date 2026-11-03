import { Button } from "@/components/ui/bp";
import { showError, showSuccess } from "../../utils/toast";
import { formatSampledNetworkHistory, type SampledNetworkHistory } from "./sampledNetworkHistory";

interface Props {
  history: SampledNetworkHistory;
  paused: boolean;
  onStart: () => void;
  onStop: () => void;
  onClear: () => void;
}

export default function SampledNetworkHistoryCard({ history, paused, onStart, onStop, onClear }: Props) {
  const copy = async () => {
    try { await navigator.clipboard.writeText(formatSampledNetworkHistory(history, new Date(), paused)); showSuccess("Sampled endpoint report copied."); }
    catch { showError("Could not copy the sampled endpoint report."); }
  };
  return <details className="active-ports-history">
    <summary>Sampled endpoint report</summary>
    <div className="active-ports-history__content">
      <p>Record the local and remote endpoints Windows shows while this view refreshes. Recording starts only when you choose it.</p>
      <p>Short connections between scans can be missed. Listening and UDP entries are not necessarily traffic; the report does not establish who initiated a connection. It includes all returned rows, even those hidden by the view filters.</p>
      <p>Data stays in memory, is limited to 1,000 observation rows, and is cleared when this view closes. No traffic contents are collected.</p>
      <p role="status">{history.running ? paused ? "Recording paused with the view." : "Recording samples." : "Recording off."} {history.count} rows retained; {history.scans} scans; {history.failedScans} failed; {history.cappedScans} capped; {history.evicted} older rows discarded.</p>
      <div className="active-ports-history__actions">
        {history.running ? <Button small onClick={onStop}>Stop recording</Button> : <Button small onClick={onStart}>Start recording</Button>}
        <Button small disabled={history.scans === 0 && history.failedScans === 0} onClick={() => void copy()}>Copy session report</Button>
        <Button small disabled={!history.startedAt} onClick={onClear}>Clear and stop</Button>
      </div>
    </div>
  </details>;
}
