// Text and code: shown plain at once, then coloured when the highlighter's classes arrive. Only
// the lines on screen are drawn (a 4 MB log scrolls as fast as a short file).

import { useVirtualizer } from '@tanstack/react-virtual';
import { useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';

import { api, type Shown, type TextPreview } from '../lib/api';
import { pieces, spansByLine, splitLines } from '../lib/lines';

const LINE_H = 20;

export function TextView({ shown, onMeta }: { shown: Shown; onMeta: (meta: string) => void }) {
  const { t } = useTranslation();
  const [text, setText] = useState<TextPreview | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [spans, setSpans] = useState<number[]>([]);
  const scroller = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let gone = false;
    api.readText(shown.id).then(
      (r) => {
        if (gone) return;
        setText(r);
        // Colours come second: the text is readable meanwhile.
        if (r.language) void api.highlight(shown.id).then((s) => !gone && setSpans(s));
      },
      (e) => !gone && setError(String(e)),
    );
    return () => {
      gone = true;
    };
  }, [shown.id]);

  const lines = useMemo(() => splitLines(text?.text ?? ''), [text]);
  const byLine = useMemo(() => spansByLine(spans, lines.starts), [spans, lines]);

  useEffect(() => {
    if (!text) return;
    const parts = [text.language ?? t('view.plainText'), text.encoding, t('view.lines', { count: lines.lines.length })];
    if (text.truncated) parts.push(t('view.truncated'));
    onMeta(parts.join(' · '));
  }, [text, lines, onMeta, t]);

  // The virtualizer's functions change identity every render; the rows are not memoized.
  // eslint-disable-next-line react-hooks/incompatible-library
  const v = useVirtualizer({
    count: text ? lines.lines.length : 0,
    getScrollElement: () => scroller.current,
    estimateSize: () => LINE_H,
    overscan: 20,
  });

  if (error) return <div className="look-note">{error}</div>;
  const gutter = String(lines.lines.length).length;
  return (
    <div className="look-text" ref={scroller} tabIndex={0}>
      <div style={{ height: v.getTotalSize(), width: `calc(${lines.columns + gutter + 3}ch + 32px)`, position: 'relative' }}>
        {v.getVirtualItems().map((row) => (
          <div key={row.index} className="tl" style={{ transform: `translateY(${row.start}px)` }}>
            <span className="ln" style={{ width: `${gutter}ch` }}>
              {row.index + 1}
            </span>
            {pieces(lines.lines[row.index], byLine.get(row.index)).map((p, i) =>
              p.cls ? (
                <span key={i} className={`tk${p.cls}`}>
                  {p.text}
                </span>
              ) : (
                p.text
              ),
            )}
          </div>
        ))}
      </div>
    </div>
  );
}
