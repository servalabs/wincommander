// src/panels/productivity/DateNav.tsx
//
// Day picker for the Productivity panel — ActivityWatch keeps months of
// local history but the old embedded-webview panel only ever showed today.
// Plain native <input type="date"> (not the bp.tsx InputGroup shim: its
// min/max props are typed for numeric inputs, not date strings).

import { Button } from "@/components/ui/bp";

interface DateNavProps {
  date: Date;
  onChange: (date: Date) => void;
}

function toInputValue(date: Date): string {
  const y = date.getFullYear();
  const m = String(date.getMonth() + 1).padStart(2, "0");
  const d = String(date.getDate()).padStart(2, "0");
  return `${y}-${m}-${d}`;
}

function startOfDay(date: Date): Date {
  const next = new Date(date);
  next.setHours(0, 0, 0, 0);
  return next;
}

function addDays(date: Date, delta: number): Date {
  const next = startOfDay(date);
  next.setDate(next.getDate() + delta);
  return next;
}

export default function DateNav({ date, onChange }: DateNavProps) {
  const today = startOfDay(new Date());
  const isToday = toInputValue(date) === toInputValue(today);

  return (
    <div className="productivity-date-nav">
      <Button icon="chevron-left" minimal small aria-label="Previous day" onClick={() => onChange(addDays(date, -1))} />
      <input
        type="date"
        className="productivity-date-input"
        value={toInputValue(date)}
        max={toInputValue(today)}
        aria-label="Select date"
        onChange={(event) => {
          const [y, m, d] = event.target.value.split("-").map(Number);
          if (!y || !m || !d) return;
          onChange(new Date(y, m - 1, d));
        }}
      />
      <Button icon="chevron-right" minimal small disabled={isToday} aria-label="Next day" onClick={() => onChange(addDays(date, 1))} />
      {!isToday && (
        <Button minimal small onClick={() => onChange(today)}>Today</Button>
      )}
    </div>
  );
}
