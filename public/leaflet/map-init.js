// Leaflet map initializer for the Metadata Scrubber GPS preview.
//
// Loaded as an external, same-origin ('self') script inside the map iframe so
// it satisfies the app CSP `script-src 'self'` in packaged builds — an inline
// <script> would be blocked there (no 'unsafe-inline'). Marker/center data is
// passed in via a non-executable <script type="application/json" id="map-data">
// block, which CSP treats as data, not script.
(function () {
  if (typeof L === "undefined") return;

  var map = L.map("m", { zoomControl: true }).setView([20, 0], 2);
  var markerLayer = L.layerGroup().addTo(map);
  L.tileLayer("https://{s}.tile.openstreetmap.org/{z}/{x}/{y}.png", {
    attribution: "© OpenStreetMap",
    maxZoom: 18,
  }).addTo(map);

  function isPoint(point) {
    return point && typeof point.lat === "number" && isFinite(point.lat) &&
      typeof point.lon === "number" && isFinite(point.lon) &&
      Math.abs(point.lat) <= 90 && Math.abs(point.lon) <= 180;
  }

  function render(data) {
    if (!data || !Array.isArray(data.markers)) return;
    markerLayer.clearLayers();
    var points = [];
    data.markers.forEach(function (mk) {
      if (!isPoint(mk)) return;
      points.push([mk.lat, mk.lon]);
      L.circleMarker([mk.lat, mk.lon], {
        radius: mk.radius,
        fillColor: "#ef4444",
        color: "#0a0f12",
        weight: 2,
        opacity: 1,
        fillOpacity: 0.85,
      })
        .bindTooltip(mk.tooltip, { direction: "top", sticky: true, opacity: 0.95 })
        .bindPopup(mk.popup)
        .addTo(markerLayer);
    });

    if (isPoint(data.center)) {
      map.setView([data.center.lat, data.center.lon], 13);
    } else if (points.length > 1) {
      map.fitBounds(points, { padding: [30, 30] });
    } else if (points.length === 1) {
      map.setView(points[0], 13);
    }
  }

  window.addEventListener("message", function (event) {
    if (event.source !== window.parent || !event.data ||
      event.data.type !== "wincommander:metadata-gps-map") return;
    render(event.data.mapData);
  });
})();
