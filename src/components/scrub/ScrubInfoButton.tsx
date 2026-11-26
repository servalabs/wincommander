import type { ReactNode } from 'react';
import { Icon, Popover } from '@/components/ui/bp';

/** Small (i) button that reveals an explanation; Radix gives it Enter/Space to open and Esc to close. */
export default function ScrubInfoButton({ topic, children }: { topic: string; children: ReactNode }) {
  return (
    <Popover
      position="bottom-start"
      popoverClassName="w-64 text-xs leading-snug"
      content={<div style={{ fontSize: 11, lineHeight: 1.45, color: 'var(--color-text-primary)' }}>{children}</div>}
    >
      <button
        type="button"
        aria-label={`About ${topic}`}
        title={`About ${topic}`}
        style={{
          display: 'inline-flex',
          alignItems: 'center',
          justifyContent: 'center',
          width: 14,
          height: 14,
          padding: 0,
          marginLeft: 4,
          border: 'none',
          borderRadius: '50%',
          background: 'transparent',
          color: 'var(--color-text-muted)',
          cursor: 'pointer',
          verticalAlign: 'middle',
        }}
      >
        <Icon icon="info-sign" size={11} />
      </button>
    </Popover>
  );
}
