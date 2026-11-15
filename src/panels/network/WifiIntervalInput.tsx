import { useEffect, useId, useRef, useState } from 'react';
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
  const errorId = useId();
  const [draft, setDraft] = useState(String(value));
  const [invalid, setInvalid] = useState(false);
  const editing = useRef(false);

  useEffect(() => {
    if (!editing.current) setDraft(String(value));
  }, [value]);

  const commit = () => {
    editing.current = false;
    const next = /^\d+$/.test(draft) ? Number(draft) : NaN;
    if (!Number.isSafeInteger(next) || next < min || next > max) {
      setInvalid(true);
      return;
    }
    setInvalid(false);
    setDraft(String(value));
    if (next !== value) onCommit(next);
  };

  return <label style={{ maxWidth: 155, fontSize: 12 }}>
    {label}
    <InputGroup type="number" min={min} max={max} value={draft} disabled={disabled} aria-invalid={invalid} aria-describedby={invalid ? errorId : undefined}
      onFocus={() => { editing.current = true; }}
      onChange={(event) => { setDraft(event.target.value); setInvalid(false); }}
      onBlur={commit}
      onKeyDown={(event) => { if (event.key === 'Enter' && !event.nativeEvent.isComposing) event.currentTarget.blur(); }} />
    {invalid && <span id={errorId} role="alert">Enter {min}–{max} seconds.</span>}
  </label>;
}
