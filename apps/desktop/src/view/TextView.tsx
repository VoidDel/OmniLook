// Text and code: shown plain at once, then coloured when the highlighter's classes arrive. Only
// the lines on screen are drawn (a 4 MB log scrolls as fast as a short file). Firmware records
// (Intel HEX, S-records) are coloured here field by field, with Rust's checks marked.

import { useVirtualizer } from '@tanstack/react-virtual';
import { Fragment, type ReactNode, useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';

import { api, type Shown, type TextPreview } from '../lib/api';
import { formatSize } from '../lib/format';
import { cells, pieces, spansByLine, splitLines } from '../lib/lines';
import { addressSpan, type BadRecord, formatAddress, recordPieces } from '../lib/records';

const LINE_H = 20;

/** `text` with its wide characters and symbols in cells (`.c1`, `.c2`), so columns line up. */
function inCells(text: string): ReactNode {
  const parts = cells(text);
  if (parts.every((p) => typeof p === 'string')) return text;
  return parts.map((p, i) =>
    typeof p === 'string' ? (
      p
    ) : (
      <span key={i} className={`c${p.cols}`}>
        {p.text}
      </span>
    ),
  );
}

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
        if (r.language && !r.records) void api.highlight(shown.id).then((s) => !gone && setSpans(s));
      },
      (e) => !gone && setError(String(e)),
    );
    return () => {
      gone = true;
    };
  }, [shown.id]);

  const lines = useMemo(() => splitLines(text?.text ?? ''), [text]);
  const byLine = useMemo(() => spansByLine(spans, lines.starts), [spans, lines]);
  const records = text?.records ?? null;
  const badByLine = useMemo(() => new Map((records?.bad ?? []).map((b) => [b.line, b])), [records]);

  useEffect(() => {
    if (!text) return;
    const parts = [text.language ?? t('view.plainText')];
    if (records) {
      parts.push(t('records.data', { size: formatSize(records.dataBytes) }));
      const span = addressSpan(records);
      if (span) parts.push(span);
      if (records.rangeCount > 1) parts.push(t('records.segments', { count: records.rangeCount }));
      if (records.entry != null) parts.push(t('records.entry', { address: formatAddress(records.entry) }));
      if (records.header) parts.push(t('records.header', { text: records.header }));
      parts.push(
        records.badCount > 0
          ? t('records.bad', { count: records.badCount, line: records.bad[0].line + 1 })
          : t('records.ok'),
      );
      if (records.format === 'intelHex' && !records.ended && !records.partial) parts.push(t('records.noEnd'));
      if (records.partial) parts.push(t('records.partial'));
    } else {
      parts.push(text.encoding);
    }
    parts.push(t('view.lines', { count: lines.lines.length }));
    if (text.truncated) parts.push(t('view.truncated'));
    onMeta(parts.join(' · '));
  }, [text, records, lines, onMeta, t]);

  const problem = (b: BadRecord) =>
    b.problem === 'checksum'
      ? t('records.checksum', { expected: (b.expected ?? 0).toString(16).toUpperCase().padStart(2, '0') })
      : t(`records.${b.problem}`);

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
        {v.getVirtualItems().map((row) => {
          const line = lines.lines[row.index];
          const bad = badByLine.get(row.index);
          const ps = records
            ? recordPieces(line, records.format, bad, bad && problem(bad))
            : pieces(line, byLine.get(row.index));
          return (
            <div key={row.index} className="tl" style={{ transform: `translateY(${row.start}px)` }}>
              <span className="ln" style={{ width: `${gutter}ch` }}>
                {row.index + 1}
              </span>
              {ps.map((p, i) =>
                p.cls ? (
                  <span key={i} className={`tk${p.cls}`} title={p.title}>
                    {inCells(p.text)}
                  </span>
                ) : (
                  <Fragment key={i}>{inCells(p.text)}</Fragment>
                ),
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}
