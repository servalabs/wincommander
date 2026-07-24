import './PanelSkeleton.css';

interface PanelSkeletonProps {
  rows?: number;
  cards?: number;
}

export default function PanelSkeleton({ rows = 3, cards = 2 }: PanelSkeletonProps) {
  return (
    <div className="panel-skeleton" aria-busy="true" aria-label="Loading panel">
      {/* Panel header shimmer */}
      <div className="ps-header">
        <div className="ps-shimmer ps-title" />
        <div className="ps-shimmer ps-badge" />
      </div>

      {/* Section cards */}
      {Array.from({ length: cards }).map((_, ci) => (
        <div key={ci} className="ps-card">
          <div className="ps-card-header">
            <div className="ps-shimmer ps-card-title" />
            <div className="ps-shimmer ps-card-count" />
          </div>
          <div className="ps-card-body">
            {Array.from({ length: rows }).map((_, ri) => (
              <div key={ri} className="ps-row">
                <div className="ps-shimmer ps-row-label" />
                <div className="ps-shimmer ps-row-toggle" />
              </div>
            ))}
          </div>
        </div>
      ))}
    </div>
  );
}
