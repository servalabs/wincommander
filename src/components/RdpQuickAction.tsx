import { Icon, Button, Classes, Dialog, FormGroup, Intent, InputGroup } from "@/components/ui/bp";
import { useState } from "react";
import { useAppState } from "../context/AppContext";
import useBackend from "../hooks/useBackend";
import { useTheme } from "../context/ThemeContext";
import type { RdpNode } from "../types/settings";
import { showError, showInfo, showSuccess } from "../utils/toast";

export default function RdpQuickAction({ isCollapsed }: { isCollapsed: boolean }) {
  const { appSettings, patchAppSettings } = useAppState();
  const { connectRdp, setRdpCredentials } = useBackend();
  const { theme } = useTheme();
  const [isManageOpen, setIsManageOpen] = useState(false);
  const [isEditing, setIsEditing] = useState<RdpNode | null>(null);

  // Local state for adding/editing a connection
  const [host, setHost] = useState("");
  const [user, setUser] = useState("");
  const [label, setLabel] = useState("");
  const [pass, setPass] = useState("");
  const [loading, setLoading] = useState(false);
  const [isListOpen, setIsListOpen] = useState(false);

  // Safely extract connections from settings, ensuring it's an array
  const nodes = Array.isArray(appSettings?.app?.rdpNodes) ? appSettings!.app!.rdpNodes : [];

  const handleConnect = async (node: RdpNode) => {
    try {
      await connectRdp(node.hostname);
      showInfo(`Connecting to ${node.label}...`);
    } catch (err) {
      showError(`Connection failed: ${err}`);
    }
  };

  const handleSave = async () => {
    if (!host.trim()) return;
    setLoading(true);
    try {
      let updatedNodes = [...nodes];
      const newNode: RdpNode = {
        id: isEditing?.id || (Math.random().toString(36).substring(2, 11)),
        hostname: host.trim(),
        username: user.trim(),
        label: label.trim() || host.trim(),
      };

      if (isEditing) {
        updatedNodes = updatedNodes.map(n => n.id === isEditing.id ? newNode : n);
      } else {
        updatedNodes.push(newNode);
      }

      await patchAppSettings({
        app: {
          rdpNodes: updatedNodes
        }
      });

      if (pass.trim()) {
        await setRdpCredentials(host.trim(), user.trim(), pass.trim());
      }

      showSuccess(isEditing ? "Connection updated" : "Connection added");
      setIsEditing(null);
      setHost(""); setUser(""); setLabel(""); setPass("");
    } catch (err) {
      showError(`Failed to save: ${err}`);
    } finally {
      setLoading(false);
    }
  };

  const handleDelete = async (id: string) => {
    const updated = nodes.filter(n => n.id !== id);
    await patchAppSettings({
      app: {
        rdpNodes: updated
      }
    });
  };

  const startEdit = (node: RdpNode) => {
    setIsEditing(node);
    setHost(node.hostname);
    setUser(node.username);
    setLabel(node.label);
    setPass("");
  };

  if (isCollapsed) {
    return (
      <div className="rdp-quick-action collapsed">
        <Button
          icon="desktop"
          minimal
          onClick={() => setIsManageOpen(true)}
          title="Remote Endpoints"
          aria-label="Manage remote endpoints"
        />
      </div>
    );
  }

  if (nodes.length === 0) {
    return (
      <>
        <div className="rdp-sidebar-container">
          <Button
            minimal
            fill
            small
            icon="plus"
            onClick={() => setIsManageOpen(true)}
            text="ADD ENDPOINT"
            className="node-setup-btn"
            style={{ justifyContent: 'flex-start', color: 'var(--color-text-muted)', fontSize: '11px', fontFamily: 'var(--font-mono)' }}
          />
        </div>
        {/* Same Dialog for empty state */}
        <Dialog isOpen={isManageOpen} onClose={() => setIsManageOpen(false)} title="Remote Connections" icon="desktop" className={`rdp-manage-dialog max-w-[860px] ${theme === "dark" ? Classes.DARK : ""}`} style={{ width: "min(860px, calc(100vw - 48px))" }}>
          <div className="bp5-dialog-body" style={{ padding: '24px' }}>
            <div className="rdp-manage-columns" style={{ display: 'flex', gap: '24px' }}>
              <div className="rdp-nodes-list" style={{ flex: '1 1 340px', minWidth: '320px', borderRight: '1px solid var(--color-border)', paddingRight: '20px', maxHeight: '420px', overflowY: 'auto' }}>
                <h6 className="column-title" style={{ fontSize: '10px', letterSpacing: '1px', opacity: 0.6, color: 'var(--color-text-primary)', marginBottom: '12px', fontWeight: 700 }}>ENDPOINTS</h6>
                <div style={{ padding: '30px 10px', textAlign: 'center', opacity: 0.4 }}>
                  <Icon icon="layout-grid" size={32} />
                  <div style={{ marginTop: '8px', fontSize: '12px' }}>No configured endpoints</div>
                </div>
              </div>
              <div className="rdp-node-form" style={{ flex: '0 0 340px' }}>
                <h6 className="column-title" style={{ fontSize: '10px', letterSpacing: '1px', color: 'var(--color-accent)', marginBottom: '16px', fontWeight: 700 }}>ADD NEW CONNECTION</h6>
                <input type="text" name="username" style={{ display: 'none' }} readOnly tabIndex={-1} aria-hidden="true" />
                <input type="password" name="password" style={{ display: 'none' }} readOnly tabIndex={-1} aria-hidden="true" />
                <FormGroup label="Nickname" labelFor="rdp-ref" style={{ marginBottom: '16px' }}>
                  <InputGroup className="rdp-endpoint-input" id="rdp-ref" autoComplete="off" name="rdp-ref" value={label} onChange={(e) => setLabel(e.target.value)} placeholder="e.g. My Home PC" />
                </FormGroup>
                <FormGroup label="Computer Name or IP" labelFor="rdp-addr" style={{ marginBottom: '16px' }}>
                  <InputGroup className="rdp-endpoint-input" id="rdp-addr" autoComplete="off" name="rdp-addr" value={host} onChange={(e) => setHost(e.target.value)} placeholder="e.g. 192.168.1.5" leftIcon="ip-address" />
                </FormGroup>
                <FormGroup label="Remote User / ID" labelFor="rdp-acct" style={{ marginBottom: '16px' }}>
                  <InputGroup className="rdp-endpoint-input" id="rdp-acct" autoComplete="off" name="rdp-acct" value={user} onChange={(e) => setUser(e.target.value)} placeholder="e.g. administrator" leftIcon="user" />
                </FormGroup>
                <FormGroup label="Login Password" labelFor="rdp-sec" helperText="Stored Securely in Windows Vault" style={{ marginBottom: '24px' }}>
                  <InputGroup className="rdp-endpoint-input" id="rdp-sec" type="password" autoComplete="new-password" name="rdp-sec" value={pass} onChange={(e) => setPass(e.target.value)} placeholder="••••••••" leftIcon="key" />
                </FormGroup>
                <Button intent={Intent.PRIMARY} fill large onClick={handleSave} loading={loading} text="Save Connection" disabled={!host || !user} />
              </div>
            </div>
          </div>
        </Dialog>
      </>
    );
  }


  return (
    <>
      <div className="rdp-sidebar-container">
        <div
          className="node-header"
          style={{ cursor: 'pointer', background: 'none', border: 'none', width: '100%', padding: '6px 8px', display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}
        >
          <button
            type="button"
            className="node-header-toggle"
            aria-expanded={isListOpen}
            aria-controls="rdp-endpoint-list"
            onClick={() => setIsListOpen(!isListOpen)}
            style={{ cursor: 'pointer', background: 'none', border: 'none', padding: 0, display: 'flex', alignItems: 'center', justifyContent: 'space-between', flex: 1, minWidth: 0 }}
          >
            <span className="node-info" style={{ display: 'flex', alignItems: 'center', gap: '6px' }}>
              <Icon icon="desktop" size={13} className="node-icon" />
              <span className="node-label" style={{ fontSize: '11px', fontFamily: 'var(--font-mono)', fontWeight: 700, letterSpacing: '0.5px', color: 'var(--color-text-secondary)' }}>ENDPOINTS</span>
              <span style={{ fontSize: '10px', fontFamily: 'var(--font-mono)', color: 'var(--color-text-muted)', fontWeight: 600 }}>({nodes.length})</span>
            </span>
            <Icon icon={isListOpen ? "chevron-up" : "chevron-down"} size={12} color="var(--color-text-muted)" />
          </button>
          <Button
            icon="cog"
            minimal
            small
            onClick={() => setIsManageOpen(true)}
            className="node-settings-btn"
            aria-label="Manage remote endpoints"
          />
        </div>

        {isListOpen && (
          <div id="rdp-endpoint-list" className="rdp-multi-list" style={{ padding: '2px 0 4px' }}>
            {nodes.map(node => (
              <button
                key={node.id}
                className="rdp-endpoint-item"
                onClick={() => handleConnect(node)}
                title={node.label}
              >
                <Icon icon="dot" size={12} className="endpoint-icon" />
                <span className="endpoint-name">{node.label}</span>
              </button>
            ))}
          </div>
        )}
      </div>

      <Dialog
        isOpen={isManageOpen}
        onClose={() => setIsManageOpen(false)}
        title="Remote Connections"
        icon="desktop"
        className={`rdp-manage-dialog max-w-[860px] ${theme === "dark" ? Classes.DARK : ""}`}
        style={{ width: "min(860px, calc(100vw - 48px))" }}
      >
        <div className="bp5-dialog-body" style={{ padding: '24px' }}>
          <div className="rdp-manage-columns" style={{ display: 'flex', gap: '24px' }}>
            <div className="rdp-nodes-list" style={{ flex: '1 1 340px', minWidth: '320px', borderRight: '1px solid var(--color-border)', paddingRight: '20px', maxHeight: '420px', overflowY: 'auto' }}>
              <h6 className="column-title" style={{ fontSize: '10px', letterSpacing: '1px', opacity: 0.6, color: 'var(--color-text-primary)', marginBottom: '12px', fontWeight: 700 }}>ENDPOINTS</h6>
              <div style={{ display: 'flex', flexDirection: 'column', gap: '8px' }}>
                {nodes.length === 0 && (
                  <div style={{ padding: '30px 10px', textAlign: 'center', opacity: 0.4 }}>
                    <Icon icon="layout-grid" size={32} />
                    <div style={{ marginTop: '8px', fontSize: '12px' }}>No configured endpoints</div>
                  </div>
                )}
                {nodes.map(n => (
                  <div key={n.id} className="node-list-item" style={{
                    display: 'flex', justifyContent: 'space-between', alignItems: 'center', padding: '10px 12px',
                    background: 'var(--color-bg-primary)', border: '1px solid var(--color-border)', borderRadius: '8px',
                    transition: 'all 0.2s'
                  }}>
                    <div className="node-item-info" style={{ display: 'flex', flexDirection: 'column' }}>
                      <span className="node-item-label" style={{ fontSize: '13px', fontWeight: 600 }}>{n.label}</span>
                      <span className="node-item-host" style={{ fontSize: '11px', color: 'var(--color-text-muted)' }}>{n.hostname} • {n.username}</span>
                    </div>
                    <div className="node-item-actions" style={{ display: 'flex', gap: '4px' }}>
                      <Button
                        icon="edit"
                        minimal
                        small
                        className="node-action-btn"
                        title={`Edit ${n.label}`}
                        aria-label={`Edit ${n.label}`}
                        onClick={() => startEdit(n)}
                      />
                      <Button
                        icon="trash"
                        minimal
                        small
                        intent="danger"
                        className="node-action-btn"
                        title={`Delete ${n.label}`}
                        aria-label={`Delete ${n.label}`}
                        onClick={() => handleDelete(n.id)}
                      />
                    </div>
                  </div>
                ))}
              </div>
              <Button icon="plus" minimal fill small style={{ marginTop: '16px', background: 'rgba(255,255,255,0.03)' }} onClick={() => { setIsEditing(null); setHost(""); setUser(""); setLabel(""); setPass(""); }} text="New Connection" />
            </div>

            <div className="rdp-node-form" style={{ flex: '0 0 340px' }}>
              <h6 className="column-title" style={{ fontSize: '10px', letterSpacing: '1px', color: 'var(--color-accent)', marginBottom: '16px', fontWeight: 700 }}>{isEditing ? "EDIT CONNECTION" : "ADD NEW CONNECTION"}</h6>

              {/* Invisible honeypot fields — tricks browser autofill away from real inputs */}
              <input type="text" name="username" style={{ display: 'none' }} readOnly tabIndex={-1} aria-hidden="true" />
              <input type="password" name="password" style={{ display: 'none' }} readOnly tabIndex={-1} aria-hidden="true" />

              <FormGroup label="Nickname (optional)" labelFor="rdp-ref" style={{ marginBottom: '16px' }}>
                <InputGroup className="rdp-endpoint-input" id="rdp-ref" autoComplete="off" name="rdp-ref" value={label} onChange={(e) => setLabel(e.target.value)} placeholder="e.g. My Home PC" />
              </FormGroup>

              <FormGroup label="Computer Name or IP" labelFor="rdp-addr" style={{ marginBottom: '16px' }}>
                <InputGroup className="rdp-endpoint-input" id="rdp-addr" autoComplete="off" name="rdp-addr" value={host} onChange={(e) => setHost(e.target.value)} placeholder="e.g. 192.168.1.5 or WORK-DESKTOP" leftIcon="ip-address" />
              </FormGroup>

              <FormGroup label="Remote User / ID" labelFor="rdp-acct" style={{ marginBottom: '16px' }}>
                <InputGroup className="rdp-endpoint-input" id="rdp-acct" autoComplete="off" name="rdp-acct" value={user} onChange={(e) => setUser(e.target.value)} placeholder="e.g. administrator" leftIcon="user" />
              </FormGroup>

              <FormGroup label="Login Password" labelFor="rdp-sec" helperText="Stored Securely in Windows Vault" style={{ marginBottom: '24px' }}>
                <InputGroup className="rdp-endpoint-input" id="rdp-sec" type="password" autoComplete="new-password" name="rdp-sec" value={pass} onChange={(e) => setPass(e.target.value)} placeholder="••••••••" leftIcon="key" />
              </FormGroup>

              <Button
                intent={Intent.PRIMARY}
                fill
                large
                onClick={handleSave}
                loading={loading}
                text={isEditing ? "Save Changes" : "Save Connection"}
                disabled={!host || !user}
              />
            </div>
          </div>
        </div>
      </Dialog>
    </>
  );
}
