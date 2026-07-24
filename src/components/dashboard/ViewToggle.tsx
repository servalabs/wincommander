import { Icon } from "@/components/ui/bp";

type ViewMode = "map" | "risk" | "products";

interface ViewToggleProps {
  viewMode: ViewMode;
  setViewMode: (mode: ViewMode) => void;
  showProductsTab: boolean;
  showRiskMatrix: boolean;
  showMoreProducts: boolean;
}

export default function ViewToggle({ viewMode, setViewMode, showProductsTab, showRiskMatrix, showMoreProducts }: ViewToggleProps) {
  // The Live Map toggle always *could* render, but a single lonely toggle is
  // pointless: if neither the Risk Matrix nor More Products button would show,
  // suppress the whole toggle group. The map still renders as default content;
  // only the toggle chrome is hidden.
  const showProducts = showMoreProducts && showProductsTab;
  if (!showRiskMatrix && !showProducts) return null;

  return (
    <div className="view-toggle-container">
      <button
        className={`view-toggle-btn ${viewMode === "map" ? "active" : ""}`}
        onClick={() => setViewMode("map")}
      >
        <Icon icon="globe" size={14} />
        <span>LIVE MAP</span>
      </button>
      {showRiskMatrix && (
        <button
          className={`view-toggle-btn ${viewMode === "risk" ? "active" : ""}`}
          onClick={() => setViewMode("risk")}
        >
          <Icon icon="shield" size={14} />
          <span>RISK MATRIX</span>
        </button>
      )}
      {showProducts && (
        <button
          className={`view-toggle-btn ${viewMode === "products" ? "active" : ""}`}
          onClick={() => setViewMode("products")}
        >
          <Icon icon="clean" size={14} />
          <span>MORE PRODUCTS</span>
        </button>
      )}
    </div>
  );
}
