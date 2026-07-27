// Suspense fallback used only for the Fleet panel (see App.tsx). Fleet's own
// FleetConnectView renders a purpose-built loading card once its lazy chunk
// has mounted (see FleetConnectView.tsx's `!loaded || !statusChecked` branch);
// this mirrors that same card shape for the brief window BEFORE the chunk has
// even loaded, so a cold first navigation to Fleet doesn't flash the generic
// cross-panel PanelSkeleton first. Reuses FleetConnectView's own CSS classes
// (src/panels/fleet/index.css) rather than duplicating them — presentational
// only, no hooks, no backend calls.
export default function FleetSkeleton() {
  return (
    <div className="fleet-connect-card" aria-busy="true" aria-label="Loading device enrollment status">
      <div className="fleet-connect-header">
        <div className="fleet-connect-title-row">
          <h3 className="fleet-connect-title">Device Enrollment</h3>
        </div>
        <span className="fleet-connect-badge fleet-connect-badge--pending">
          <span className="fleet-dot is-pending" /> Connecting…
        </span>
      </div>
      <div className="fleet-connect-status">
        <dl className="fleet-connect-meta" aria-hidden="true">
          <div className="fleet-meta">
            <dt className="fleet-shimmer fleet-shimmer-label" />
            <dd className="fleet-shimmer fleet-shimmer-value" />
          </div>
          <div className="fleet-meta">
            <dt className="fleet-shimmer fleet-shimmer-label" />
            <dd className="fleet-shimmer fleet-shimmer-value" />
          </div>
        </dl>
      </div>
    </div>
  );
}
