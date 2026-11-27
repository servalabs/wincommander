import { describe, expect, test } from 'bun:test';
import { renderToStaticMarkup } from 'react-dom/server';
import UndoReplaceBar, { undoExpiryLabel } from './UndoReplaceBar';

const undo = { jobId: '19a-0123', files: 3, bytes: 3 * 1024 * 1024, expiresMs: 0 };
const never = () => Promise.reject(new Error('not called'));

describe('UndoReplaceBar', () => {
  test('offers Undo with the kept originals and how long they stay', () => {
    const now = Date.now();
    const html = renderToStaticMarkup(<UndoReplaceBar undo={{ ...undo, expiresMs: now + 23.6 * 3_600_000 }} onUndo={never} />);
    expect(html).toContain('3 originals');
    expect(html).toContain('Undo is available for about 24 more hours');
    expect(html).toContain('Undo');
  });

  test('reports what was put back and what was left alone', () => {
    const html = renderToStaticMarkup(
      <UndoReplaceBar
        undo={undo}
        onUndo={never}
        initialResult={{
          restored: ['C:\\a.jpg', 'C:\\b.jpg'],
          skipped: [{ path: 'C:\\c.docx', reason: 'the file was changed after it was cleaned, so it was left as it is', keptAt: 'C:\\undo\\0.orig' }],
        }}
      />,
    );
    expect(html).toContain('2 of 3 originals put back');
    expect(html).toContain('changed after it was cleaned');
    expect(html).toContain('C:\\undo\\0.orig');
  });

  test('expiry wording', () => {
    expect(undoExpiryLabel(1_000, 2_000)).toBe('Undo has expired');
    expect(undoExpiryLabel(2_000 + 1_800_000 - 1, 2_000)).toBe('Undo expires within the hour');
    expect(undoExpiryLabel(2_000 + 3_600_000, 2_000)).toBe('Undo is available for about 1 more hour');
  });
});
