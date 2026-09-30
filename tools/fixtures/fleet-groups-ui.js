// Isolated browser fixture. All service calls are intercepted by its test runner.
import React from 'react';
import { createRoot } from 'react-dom/client';
import '../../src/index.css';
import '../../src/styles/v2-theme.css';
import FleetPanel from '../../src/panels/fleet/index';

window.__groupNotices = [];
window.__groupStallDiscovery = false;
window.__groupDiscoveryCount = 0;
window.__groupService = {
  getCapabilities: async () => ({ can_manage_policy: true }),
  getAccessDirectory: () => window.__readMachineGroups(),
  saveAccessDirectory: value => window.__saveMachineGroups(value),
};
window.__groupBackend = {
  getFleetAccessUsers: async () => {
    window.__groupDiscoveryCount++;
    if (window.__groupStallDiscovery) await new Promise(resolve => { window.__releaseGroupDiscovery = resolve; });
    return { success: true, data: { users: [{ sid: 'S-1-5-21-101', name: 'Alice', isCurrent: true }] } };
  },
};
createRoot(document.getElementById('fixture')).render(React.createElement(FleetPanel));
