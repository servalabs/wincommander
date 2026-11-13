import { useEffect, useRef, useState } from 'react';
import { InputGroup } from '@/components/ui/bp';

type Props = {
  label: string;
  value: number;
  min: number;
  max: number;
  disabled: boolean;
  onCommit: (value: number) => void;
};

export default function WifiIntervalInput({ label, value, min, max, disabled, onCommit }: Props) {
  const [draft, setDraft] = useState(String(value));
  const [invalid, setInvalid] = useState(false);
  const editing = useRef(false);

  useEffect(() => {
    if (!editing.current) setDraft(String(value));
  }, [value]);

  const commit = () => {
    editing.current = false;
    const next = /^\d+$/.test(draft) ? Number(draft) : NaN;
    setDraft(String(value));
    if (!Number.isSafeInteger(next) || next < min || next > max) {
      setInvalid(true);
      return;
    }
    setInvalid(false);
    if (next !== value) onCommit(next);
  };

  return <label style={{ maxWidth: 155, fontSize: 12 }}>
    {label}
    <InputGroup type="number" min={min} max={max} value={draft} disabled={disabled}
      onFocus={() => { editing.current = true; }}
      onChange={(event) => { setDraft(event.target.value); setInvalid(false); }}
      onBlur={commit}
      onKeyDown={(event) => { if (event.key === 'Enter') event.currentTarget.blur(); }} />
    {invalid && <span role="alert">Enter {min}–{max} seconds.</span>}
  </label>;
}
