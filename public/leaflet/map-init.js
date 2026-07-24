// Leaflet map initializer for the Metadata Scrubber GPS preview.
//
// Loaded as an external, same-origin ('self') script inside the map iframe so
// it satisfies the app CSP `script-src 'self'` in packaged builds — an inline
// <script> would be blocked there (no 'unsafe-inline'). Marker/center data is
// passed in via a non-executable <script type="application/json" id="map-data">
// block, which CSP treats as data, not script.
(function () {
  var dataEl = document.getElementById("map-data");
  if (!dataEl || typeof L === "undefined") return;

  var data;
  try {
    data = JSON.parse(dataEl.textContent || "{}");
  } catch (e) {
    return;
  }

  var map = L.map("m", { zoomControl: true });
  L.tileLayer("https://{s}.tile.openstreetmap.org/{z}/{x}/{y}.png", {
    attribution: "© OpenStreetMap",
    maxZoom: 18,
  }).addTo(map);

  (data.markers || []).forEach(function (mk) {
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
      .addTo(map);
  });

  if (data.center) {
    map.setView([data.center.lat, data.center.lon], 13);
  } else if (data.bounds && data.bounds.length) {
    map.fitBounds(data.bounds, { padding: [30, 30] });
  }
})();
