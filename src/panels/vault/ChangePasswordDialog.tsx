import { useCallback, useReducer, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { Button, Callout, Dialog, FormGroup, Icon, InputGroup } from "@/components/ui/bp";
import VaultOperationNotice from "@/components/shared/VaultOperationNotice";
import useBackend from "../../hooks/useBackend";
import { useTheme } from "../../context/ThemeContext";
import { newDiagnosticOperationId, recordDiagnostic } from "../../lib/diagnostics";
import { showSuccess } from "../../utils/toast";
import {
  PASSWORD_CHANGE_BUSY,
  PASSWORD_CHANGE_WARNING,
  PASSWORD_PROBE_BUSY,
  readProbeHeader,
  type HeaderCredentials,
} from "@/lib/vaultPasswordChange";
import {
  DECOY_QUESTION,
  passwordChangeProblem,
  passwordChangeReducer,
  passwordChangeStage,
  startPasswordChange,
} from "@/lib/vaultPasswordChangeFlow";

type Part = "first" | "decoy";
type PasswordField = "oldPassword" | "newPassword" | "confirmPassword";

interface ChangePasswordDialogProps {
  isOpen: boolean;
  onClose: () => void;
  /** Prefilled from the vault row that opened the dialog; the person can still pick another file. */
  initialPath?: string;
}

export default function ChangePasswordDialog({ isOpen, onClose, initialPath = "" }: ChangePasswordDialogProps) {
  const { theme } = useTheme();
  const { probeVolumePassword, changeVolumePassword } = useBackend();
  const [flow, dispatch] = useReducer(passwordChangeReducer, initialPath, startPasswordChange);
  const [showPasswords, setShowPasswords] = useState(false);
  const [showExtras, setShowExtras] = useState(false);
  const { step, busy } = flow;
  const stage = passwordChangeStage(step);
  const problem = passwordChangeProblem(flow);

  const edit = useCallback((part: Part, field: keyof HeaderCredentials, value: string) => {
    dispatch({ type: "edit", part, field, value });
  }, []);

  const close = useCallback(() => {
    if (busy) return;
    dispatch({ type: "reset" });
    setShowExtras(false);
    onClose();
  }, [busy, onClose]);

  const browse = useCallback(async (target: "volume" | { part: Part; field: "oldKeyfile" | "newKeyfile" }) => {
    try {
      const selected = await open(target === "volume"
        ? { multiple: false, filters: [{ name: "Container File", extensions: ["hc", "tc", "*"] }] }
        : { multiple: false });
      if (typeof selected !== "string") return;
      if (target === "volume") dispatch({ type: "path", volumePath: selected });
      else edit(target.part, target.field, selected);
    } catch {
      dispatch({ type: "notice", failure: "The file picker could not be opened. Type the path instead." });
    }
  }, [edit]);

  const submit = useCallback(async () => {
    if (busy || problem || !stage) return;
    const operationId = newDiagnosticOperationId("vault");
    const action = stage === "probe" ? "probe_password" : "change_password";
    const { volumePath, first, decoy, header, provenReal } = flow;
    dispatch({ type: "send" });
    setShowExtras(false);
    try {
      if (stage === "probe") {
        const result = await probeVolumePassword(volumePath, first);
        const opened = result.success ? readProbeHeader(result.data) : null;
        if (!opened) throw new Error(result.error || "");
        dispatch({ type: "probed", header: opened });
      } else {
        const result = await changeVolumePassword(volumePath, stage === "decoy" ? "outer" : header ?? "outer", stage === "decoy" ? decoy : first, stage === "decoy" ? provenReal : null);
        if (!result.success || result.data?.status !== "password_changed") throw new Error(result.error || "");
        dispatch({ type: "changed" });
        showSuccess(stage === "decoy" ? "Decoy password changed." : "Vault password changed.", undefined, { kind: "notification", operationId });
      }
      recordDiagnostic({ operationId, feature: "vault", action, stage: "service_receipt",
        lifecycle: "verified", outcome: "succeeded", severity: "info", retryability: "never",
        suggestedNextAction: "none", privacyClass: "local_sensitive" });
    } catch (error) {
      dispatch({ type: "failed", error });
      recordDiagnostic({ operationId, feature: "vault", action, stage: "service_receipt",
        lifecycle: "verified", outcome: "failed", errorCode: "VLT.PASSWORD.CHANGE_FAILED", severity: "warn",
        retryability: "manual", suggestedNextAction: "review_status", privacyClass: "local_sensitive" });
    }
  }, [busy, changeVolumePassword, flow, probeVolumePassword, problem, stage]);

  const passwordField = (part: Part, field: PasswordField, label: string, withToggle = false) => (
    <FormGroup key={`${part}-${field}`} label={label} labelFor={`change-${part}-${field}`}>
      <InputGroup id={`change-${part}-${field}`} type={showPasswords ? "text" : "password"} value={flow[part][field]}
        autoComplete="off" onChange={event => edit(part, field, event.target.value)}
        rightElement={withToggle ? (
          <Button icon={showPasswords ? "eye-off" : "eye-open"} minimal
            aria-label={showPasswords ? "Hide passwords" : "Show passwords"}
            onClick={() => setShowPasswords(!showPasswords)} />
        ) : undefined} />
    </FormGroup>
  );

  const keyfileField = (part: Part, field: "oldKeyfile" | "newKeyfile", label: string) => (
    <FormGroup key={`${part}-${field}`} label={label} labelFor={`change-${part}-${field}`}>
      <InputGroup id={`change-${part}-${field}`} placeholder="Path to keyfile" value={flow[part][field]} autoComplete="off"
        onChange={event => edit(part, field, event.target.value)}
        rightElement={<Button icon="folder-open" minimal aria-label={`Browse for ${label.toLowerCase()}`} onClick={() => void browse({ part, field })} />} />
    </FormGroup>
  );

  const pimField = (part: Part, field: "oldPim" | "newPim", label: string) => (
    <FormGroup key={`${part}-${field}`} label={label} labelFor={`change-${part}-${field}`} helperText="Leave blank unless the vault uses a custom PIM.">
      <InputGroup id={`change-${part}-${field}`} type="number" min={1} max={2147468} placeholder="Default" value={flow[part][field]}
        autoComplete="off" onChange={event => edit(part, field, event.target.value)} />
    </FormGroup>
  );

  const fields = step === "current" ? [passwordField("first", "oldPassword", "Current password", true)]
    : step === "new" ? [
      passwordField("first", "newPassword", "New password", true),
      passwordField("first", "confirmPassword", "Confirm new password"),
    ] : step === "decoy" ? [
      passwordField("decoy", "oldPassword", "Current decoy password", true),
      passwordField("decoy", "newPassword", "New decoy password"),
      passwordField("decoy", "confirmPassword", "Confirm new decoy password"),
    ] : [];

  const extras = step === "current" ? [pimField("first", "oldPim", "PIM (optional)"), keyfileField("first", "oldKeyfile", "Keyfile (optional)")]
    : step === "new" ? [pimField("first", "newPim", "New PIM (optional)"), keyfileField("first", "newKeyfile", "New keyfile (optional)")]
      : step === "decoy" ? [
        pimField("decoy", "oldPim", "Current decoy PIM (optional)"),
        keyfileField("decoy", "oldKeyfile", "Current decoy keyfile (optional)"),
        pimField("decoy", "newPim", "New decoy PIM (optional)"),
        keyfileField("decoy", "newKeyfile", "New decoy keyfile (optional)"),
      ] : [];

  const primaryText = step === "current" ? "CONTINUE" : step === "decoy" ? "CHANGE DECOY PASSWORD" : "CHANGE PASSWORD";

  return (
    <Dialog isOpen={isOpen} onClose={close} title="Change Vault Password"
      className={`mount-dialog vault-file-dialog ${theme === "light" ? "light" : ""}`}>
      <div className="wc-dialog-body">
        {stage && <Callout intent="warning" icon="warning-sign" className="mb-4">{PASSWORD_CHANGE_WARNING}</Callout>}
        <VaultOperationNotice tone="info" message={busy ? (stage === "probe" ? PASSWORD_PROBE_BUSY : PASSWORD_CHANGE_BUSY) : ""} />
        <VaultOperationNotice tone="success" message={flow.success} />
        <VaultOperationNotice message={flow.failure} />
        {step === "decoy-ask" && <p className="vault-card-status-line" role="status">{DECOY_QUESTION}</p>}
        {stage && (
          <fieldset disabled={busy} style={{ border: 0, padding: 0, margin: 0, minWidth: 0 }}>
            <FormGroup label="Vault file" labelFor="change-volume-path" helperText="The vault must be dismounted.">
              <InputGroup id="change-volume-path" placeholder="/path/to/volume.hc" value={flow.volumePath} autoComplete="off"
                disabled={step !== "current"} onChange={event => dispatch({ type: "path", volumePath: event.target.value })}
                rightElement={step === "current"
                  ? <Button icon="folder-open" minimal aria-label="Browse for the vault file" onClick={() => void browse("volume")} />
                  : undefined} />
            </FormGroup>
            {fields}
            <Button minimal small icon={showExtras ? "chevron-down" : "chevron-right"}
              text="Advanced: PIM and keyfile" onClick={() => setShowExtras(!showExtras)} aria-expanded={showExtras} />
            {showExtras && <div className="mount-options-grid" aria-label="PIM and keyfile options">{extras}</div>}
            {problem && flow.volumePath && !busy && <p className="vault-card-status-line" role="status"><Icon icon="info-sign" size={12} /> {problem}</p>}
          </fieldset>
        )}
      </div>
      <div className="mount-dialog-footer">
        {step === "decoy-ask" ? <>
          <Button icon="cross" text="NO" onClick={() => dispatch({ type: "decoy-no" })} minimal className="modal-cancel-btn" />
          <Button icon="key" text="YES" onClick={() => dispatch({ type: "decoy-yes" })} className="modal-primary-btn" />
        </> : <>
          <Button icon="cross" text={stage ? "CANCEL" : "DONE"} disabled={busy} onClick={close} minimal className="modal-cancel-btn" />
          {stage && <Button icon={step === "current" ? "arrow-right" : "key"} text={primaryText} onClick={() => void submit()} loading={busy}
            disabled={Boolean(problem) || busy} className="modal-primary-btn" />}
        </>}
      </div>
    </Dialog>
  );
}
