import { VAULT_ACCESS_PRESETS, type VaultAccessPreset } from "./vaultAccessPresets";
import "./VaultAccessPatternPicker.css";

interface VaultAccessPatternPickerProps {
  value: VaultAccessPreset;
  onChange: (preset: Exclude<VaultAccessPreset, "custom">) => void;
}

const PATTERN_ORDER: Exclude<VaultAccessPreset, "custom">[] = ["private", "shared-read", "shared-write"];

export default function VaultAccessPatternPicker({ value, onChange }: VaultAccessPatternPickerProps) {
  return (
    <fieldset className="vault-access-pattern-picker">
      <legend>2. Choose who can use this vault</legend>
      <p>Pick the closest everyday outcome. You can change it before saving.</p>
      <div className="vault-access-pattern-picker__options">
        {PATTERN_ORDER.map(pattern => {
          const definition = VAULT_ACCESS_PRESETS[pattern];
          return <button
            type="button"
            key={pattern}
            className={`vault-access-pattern-picker__option${value === pattern ? " is-selected" : ""}`}
            aria-pressed={value === pattern}
            onClick={() => onChange(pattern)}
          >
            <strong>{definition.label}</strong>
            <span>{definition.description}</span>
          </button>;
        })}
        {value === "custom" && <div className="vault-access-pattern-picker__custom" role="status">
          <strong>Custom access</strong>
          <span>This saved vault has different levels for different people. Its current permissions have not been changed.</span>
        </div>}
      </div>
    </fieldset>
  );
}
