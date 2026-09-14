import { VAULT_ACCESS_PRESETS, type VaultAccessPreset } from "./vaultAccessPresets";
import VaultAccessInfo from "./VaultAccessInfo";
import "./VaultAccessPatternPicker.css";

interface VaultAccessPatternPickerProps {
  value: VaultAccessPreset;
  onChange: (preset: Exclude<VaultAccessPreset, "custom">) => void;
}

const PATTERN_ORDER: Exclude<VaultAccessPreset, "custom">[] = ["private", "shared-read", "shared-write"];

export default function VaultAccessPatternPicker({ value, onChange }: VaultAccessPatternPickerProps) {
  return (
    <fieldset className="vault-access-pattern-picker">
      <legend>Access pattern</legend>
      <div className="vault-access-pattern-picker__options">
        {PATTERN_ORDER.map(pattern => {
          const definition = VAULT_ACCESS_PRESETS[pattern];
          return <div className={`vault-access-pattern-picker__choice${value === pattern ? " is-selected" : ""}`} key={pattern}>
            <button
              type="button"
              className="vault-access-pattern-picker__option"
              aria-pressed={value === pattern}
              onClick={() => onChange(pattern)}
            >
              <strong>{definition.label}</strong>
            </button>
            <VaultAccessInfo label={`About ${definition.label.toLowerCase()}`}>{definition.description}</VaultAccessInfo>
          </div>;
        })}
      </div>
      {value === "custom" && <div className="vault-access-pattern-picker__custom" role="status">
        <strong>Custom access</strong>
        <span>Mixed access levels. Review each row before saving.</span>
      </div>}
    </fieldset>
  );
}
