import { useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import type { FleetAccessDirectory } from "./accessControlTypes";
import { resolveVaultPrincipalOption, vaultPrincipalSelectOptions } from "./vaultAccessUiState";

const MANUAL_OPTION_VALUE = "__manual__";

interface VaultPrincipalPickerProps {
  value: string;
  directory: FleetAccessDirectory;
  onChange: (principalName: string) => void;
  ariaLabel: string;
}

/**
 * Emits a bare local principal name — localGroup for a group, username for a
 * user — the same convention `resolveLegacyPrincipal` uses. The service
 * resolves it with LookupAccountNameW(NULL, ...); a MACHINE\ prefix is never
 * synthesised here, and this component never calls out to discover one.
 */
export default function VaultPrincipalPicker({ value, directory, onChange, ariaLabel }: VaultPrincipalPickerProps) {
  const [manualEntry, setManualEntry] = useState(false);

  if (manualEntry) {
    return <>
      <Input aria-label={ariaLabel} value={value} placeholder="DOMAIN\Partner or Administrators" onChange={event => onChange(event.target.value)} />
      <Button variant="ghost" size="sm" onClick={() => setManualEntry(false)}>Choose from directory instead</Button>
    </>;
  }

  const options = vaultPrincipalSelectOptions(value, directory);
  const current = resolveVaultPrincipalOption(value, options);
  const groups = options.filter(option => option.kind === "group");
  const users = options.filter(option => option.kind === "user");
  const unverified = options.filter(option => option.kind === "unverified");

  return (
    <select
      aria-label={ariaLabel}
      value={value.trim() ? current.principalName : ""}
      onChange={event => {
        if (event.target.value === MANUAL_OPTION_VALUE) { setManualEntry(true); return; }
        onChange(event.target.value);
      }}
    >
      <option value="" disabled>Select a Windows user or group…</option>
      {unverified.length > 0 && <optgroup label="Unverified — not in the current directory">
        {unverified.map(option => <option key={option.principalName} value={option.principalName}>{option.label}</option>)}
      </optgroup>}
      {groups.length > 0 && <optgroup label="Access groups">
        {groups.map(option => <option key={option.principalName} value={option.principalName}>{option.label}</option>)}
      </optgroup>}
      {users.length > 0 && <optgroup label="Windows users">
        {users.map(option => <option key={option.principalName} value={option.principalName}>{option.label}</option>)}
      </optgroup>}
      <option value={MANUAL_OPTION_VALUE}>Enter manually…</option>
    </select>
  );
}
