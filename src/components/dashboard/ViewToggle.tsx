import { Icon } from "@/components/ui/bp";

type ViewMode = "map" | "risk" | "products";

interface ViewToggleProps {
  viewMode: ViewMode;
  setViewMode: (mode: ViewMode) => void;
  showRiskMatrix: boolean;
  showMoreProducts: boolean;
}

export default function ViewToggle({ viewMode, setViewMode, showRiskMatrix, showMoreProducts }: ViewToggleProps) {
  // Live Map is the dashboard default, so it does not need its own selector.
  // Selecting the active tag again returns to the default map view.
  if (!showRiskMatrix && !showMoreProducts) return null;

  return (
    <div className="view-toggle-container" role="toolbar" aria-label="Dashboard views">
      {showRiskMatrix && (
        <button
          className={`view-toggle-btn ${viewMode === "risk" ? "active" : ""}`}
          aria-pressed={viewMode === "risk"}
          onClick={() => setViewMode(viewMode === "risk" ? "map" : "risk")}
        >
          <Icon icon="shield" size={14} />
          <span>RISK MATRIX</span>
        </button>
      )}
      {showMoreProducts && (
        <button
          className={`view-toggle-btn ${viewMode === "products" ? "active" : ""}`}
          aria-pressed={viewMode === "products"}
          onClick={() => setViewMode(viewMode === "products" ? "map" : "products")}
        >
          <Icon icon="clean" size={14} />
          <span>MORE PRODUCTS</span>
        </button>
      )}
    </div>
  );
}
