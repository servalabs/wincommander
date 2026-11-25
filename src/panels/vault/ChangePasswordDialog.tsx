import { useCallback, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { Button, Callout, Dialog, FormGroup, Icon, InputGroup } from "@/components/ui/bp";
import VaultOperationNotice from "@/components/shared/VaultOperationNotice";
import useBackend from "../../hooks/useBackend";
import { useTheme } from "../../context/ThemeContext";
import { newDiagnosticOperationId, recordDiagnostic } from "../../lib/diagnostics";
import { showSuccess } from "../../utils/toast";
import {
  EMPTY_PASSWORD_CHANGE_FORM,
  PASSWORD_CHANGE_BUSY,
  PASSWORD_CHANGE_WARNING,
  passwordChangeFormProblem,
  vaultPasswordChangeErrorMessage,
  type VaultPasswordChangeForm,
} from "@/lib/vaultPasswordChange";

type FormField = keyof VaultPasswordChangeForm;

interface ChoiceProps<T extends string> {
  label: string;
  value: T;
  options: ReadonlyArray<{ value: T; text: string }>;
  onChange: (value: T) => void;
}

function Choice<T extends string>({ label, value, options, onChange }: ChoiceProps<T>) {
  return (
    <div className="mount-type-selector mb-4" role="radiogroup" aria-label={label}>
      {options.map(option => (
        <button key={option.value} type="button" role="radio" aria-checked={value === option.value}
          className={`mount-type-btn${value === option.value ? " is-active" : ""}`}
          onClick={() => onChange(option.value)}>
          {value === option.value && <span className="vault-active-dot" aria-hidden />}
          <span>{option.text}</span>
        </button>
      ))}
    </div>
  );
}

const VAULT_KIND_OPTIONS = [
  { value: "standard", text: "One password" },
  { value: "decoy", text: "Decoy vault (two passwords)" },
] as const;

const WHICH_OPTIONS = [
  { value: "decoy", text: "Decoy password" },
  { value: "real", text: "Real password" },
] as const;

export default function ChangePasswordDialog({ isOpen, onClose }: { isOpen: boolean; onClose: () => void }) {
  const { theme } = useTheme();
  const { changeVolumePassword } = useBackend();
  const [form, setForm] = useState<VaultPasswordChangeForm>(EMPTY_PASSWORD_CHANGE_FORM);
  const [showPasswords, setShowPasswords] = useState(false);
  const [showExtras, setShowExtras] = useState(false);
  const [busy, setBusy] = useState(false);
  const [failure, setFailure] = useState("");
  const [changed, setChanged] = useState(false);
  const problem = passwordChangeFormProblem(form);

  const update = useCallback((field: FormField, value: string) => {
    setForm(current => ({ ...current, [field]: value }));
    setFailure("");
    setChanged(false);
  }, []);

  const close = useCallback(() => {
    if (busy) return;
    setForm(EMPTY_PASSWORD_CHANGE_FORM);
    setFailure("");
    setChanged(false);
    setShowExtras(false);
    onClose();
  }, [busy, onClose]);

  const browse = useCallback(async (field: "volumePath" | "oldKeyfile" | "newKeyfile") => {
    try {
      const selected = await open(field === "volumePath"
        ? { multiple: false, filters: [{ name: "Container File", extensions: ["hc", "tc", "*"] }] }
        : { multiple: false });
      if (typeof selected === "string") update(field, selected);
    } catch {
      setFailure("The file picker could not be opened. Type the path instead.");
    }
  }, [update]);

  const submit = useCallback(async () => {
    if (busy || problem) return;
    const operationId = newDiagnosticOperationId("vault");
    setBusy(true);
    setFailure("");
    const request = form;
    // Passwords leave component state as soon as the request is sent.
    setForm(current => ({ ...current, oldPassword: "", newPassword: "", confirmPassword: "" }));
    try {
      const result = await changeVolumePassword(request);
      if (!result.success || result.data?.status !== "password_changed") throw new Error(result.error || "");
      setChanged(true);
      showSuccess("Vault password changed.", undefined, { kind: "notification", operationId });
      recordDiagnostic({ operationId, feature: "vault", action: "change_password", stage: "service_receipt",
        lifecycle: "verified", outcome: "succeeded", severity: "info", retryability: "never",
        suggestedNextAction: "none", privacyClass: "local_sensitive" });
    } catch (error) {
      setFailure(vaultPasswordChangeErrorMessage(error));
      recordDiagnostic({ operationId, feature: "vault", action: "change_password", stage: "service_receipt",
        lifecycle: "verified", outcome: "failed", errorCode: "VLT.PASSWORD.CHANGE_FAILED", severity: "warn",
        retryability: "manual", suggestedNextAction: "review_status", privacyClass: "local_sensitive" });
    } finally {
      setBusy(false);
    }
  }, [busy, changeVolumePassword, form, problem]);

  const passwordField = (field: "oldPassword" | "newPassword" | "confirmPassword", label: string) => (
    <FormGroup label={label} labelFor={`change-${field}`}>
      <InputGroup id={`change-${field}`} type={showPasswords ? "text" : "password"} value={form[field]}
        autoComplete="off" onChange={event => update(field, event.target.value)}
        rightElement={field === "oldPassword" ? (
          <Button icon={showPasswords ? "eye-off" : "eye-open"} minimal
            aria-label={showPasswords ? "Hide passwords" : "Show passwords"}
            onClick={() => setShowPasswords(!showPasswords)} />
        ) : undefined} />
    </FormGroup>
  );

  const keyfileField = (field: "oldKeyfile" | "newKeyfile", label: string) => (
    <FormGroup label={label} labelFor={`change-${field}`}>
      <InputGroup id={`change-${field}`} placeholder="Path to keyfile" value={form[field]} autoComplete="off"
        onChange={event => update(field, event.target.value)}
        rightElement={<Button icon="folder-open" minimal aria-label={`Browse for ${label.toLowerCase()}`} onClick={() => void browse(field)} />} />
    </FormGroup>
  );

  const pimField = (field: "oldPim" | "newPim", label: string) => (
    <FormGroup label={label} labelFor={`change-${field}`} helperText="Leave blank unless the vault uses a custom PIM.">
      <InputGroup id={`change-${field}`} type="number" min={1} max={2147468} placeholder="Default" value={form[field]}
        autoComplete="off" onChange={event => update(field, event.target.value)} />
    </FormGroup>
  );

  return (
    <Dialog isOpen={isOpen} onClose={close} title="Change Vault Password"
      className={`mount-dialog ${theme === "light" ? "light" : ""}`}>
      <div className="wc-dialog-body">
        <Callout intent="warning" icon="warning-sign" className="mb-4">{PASSWORD_CHANGE_WARNING}</Callout>
        <VaultOperationNotice tone="info" message={busy ? PASSWORD_CHANGE_BUSY : ""} />
        <VaultOperationNotice message={failure} />
        <VaultOperationNotice tone="success" message={changed ? "The password was changed. Use the new password the next time you mount this vault." : ""} />
        <fieldset disabled={busy} style={{ border: 0, padding: 0, margin: 0, minWidth: 0 }}>
          <FormGroup label="Vault file" labelFor="change-volume-path" helperText="The vault must be dismounted.">
            <InputGroup id="change-volume-path" placeholder="/path/to/volume.hc" value={form.volumePath} autoComplete="off"
              onChange={event => update("volumePath", event.target.value)}
              rightElement={<Button icon="folder-open" minimal aria-label="Browse for the vault file" onClick={() => void browse("volumePath")} />} />
          </FormGroup>

          <FormGroup label="Vault type">
            <Choice label="Vault type" value={form.vaultKind} options={VAULT_KIND_OPTIONS}
              onChange={value => update("vaultKind", value)} />
          </FormGroup>
          {form.vaultKind === "decoy" && (
            <FormGroup label="Which password" helperText="Only this password changes. The other one keeps working as before.">
              <Choice label="Which password" value={form.which} options={WHICH_OPTIONS}
                onChange={value => update("which", value)} />
            </FormGroup>
          )}

          {passwordField("oldPassword", "Current password")}
          {passwordField("newPassword", "New password")}
          {passwordField("confirmPassword", "Type the new password again")}

          <Button minimal small icon={showExtras ? "chevron-down" : "chevron-right"}
            text="PIM and keyfile options" onClick={() => setShowExtras(!showExtras)} aria-expanded={showExtras} />
          {showExtras && (
            <div className="mount-options-grid" aria-label="PIM and keyfile options">
              {pimField("oldPim", "Current PIM (optional)")}
              {pimField("newPim", "New PIM (optional)")}
              {keyfileField("oldKeyfile", "Current keyfile (optional)")}
              {keyfileField("newKeyfile", "New keyfile (optional)")}
            </div>
          )}
          {problem && form.volumePath && <p className="vault-card-status-line" role="status"><Icon icon="info-sign" size={12} /> {problem}</p>}
        </fieldset>
      </div>
      <div className="mount-dialog-footer">
        <Button icon="cross" text={changed ? "DONE" : "CANCEL"} disabled={busy} onClick={close} minimal className="modal-cancel-btn" />
        <Button icon="key" text="CHANGE PASSWORD" onClick={() => void submit()} loading={busy}
          disabled={Boolean(problem) || busy} className="modal-primary-btn" />
      </div>
    </Dialog>
  );
}
