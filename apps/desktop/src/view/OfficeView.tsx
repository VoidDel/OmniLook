// Word, Excel and PowerPoint without Office (docs/development-plan.md §5.1, M3): their text,
// read in Rust (crates/omnilook-core/src/office.rs) and drawn here as plain elements. Not laid
// out as Office would; enough to see what is in the file.

import { type ReactNode, useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';

import { api, type OfficeBlock, type OfficePreview, type OfficeRun, type Shown } from '../lib/api';

function Runs({ runs }: { runs: OfficeRun[] }) {
  return (
    <>
      {runs.map((r, i) => {
        let node: ReactNode = r.text;
        if (r.italic) node = <em>{node}</em>;
        if (r.bold) node = <strong>{node}</strong>;
        return <span key={i}>{node}</span>;
      })}
    </>
  );
}

function Table({ rows, heads }: { rows: string[][]; heads?: boolean }) {
  const width = rows.reduce((m, r) => Math.max(m, r.length), 0);
  const letter = (i: number) => {
    let s = '';
    for (let n = i + 1; n > 0; n = Math.floor((n - 1) / 26)) s = String.fromCharCode(65 + ((n - 1) % 26)) + s;
    return s;
  };
  return (
    <table>
      {heads && (
        <thead>
          <tr>
            <th />
            {Array.from({ length: width }, (_, i) => (
              <th key={i}>{letter(i)}</th>
            ))}
          </tr>
        </thead>
      )}
      <tbody>
        {rows.map((r, i) => (
          <tr key={i}>
            {heads && <th>{i + 1}</th>}
            {Array.from({ length: width }, (_, j) => (
              <td key={j}>{r[j] ?? ''}</td>
            ))}
          </tr>
        ))}
      </tbody>
    </table>
  );
}

/** Paragraphs; consecutive list items share one list. */
function Word({ blocks }: { blocks: OfficeBlock[] }) {
  const out: ReactNode[] = [];
  let list: ReactNode[] = [];
  const flush = () => {
    if (list.length) out.push(<ul key={`l${out.length}`}>{list}</ul>);
    list = [];
  };
  blocks.forEach((b, i) => {
    if (b.kind === 'para' && b.list) {
      list.push(
        <li key={i}>
          <Runs runs={b.runs} />
        </li>,
      );
      return;
    }
    flush();
    if (b.kind === 'table') out.push(<Table key={i} rows={b.rows} />);
    else if (b.level > 0) {
      const H = `h${Math.min(b.level, 6)}` as 'h1';
      out.push(
        <H key={i}>
          <Runs runs={b.runs} />
        </H>,
      );
    } else
      out.push(
        <p key={i}>
          <Runs runs={b.runs} />
        </p>,
      );
  });
  flush();
  return <article className="look-md look-doc">{out}</article>;
}

export function OfficeView({ shown, onMeta }: { shown: Shown; onMeta: (meta: string) => void }) {
  const { t } = useTranslation();
  const [doc, setDoc] = useState<OfficePreview | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [sheet, setSheet] = useState(0);

  useEffect(() => {
    let gone = false;
    api.readOffice(shown.id).then(
      (r) => !gone && setDoc(r),
      (e) => !gone && setError(String(e)),
    );
    return () => {
      gone = true;
    };
  }, [shown.id]);

  useEffect(() => {
    if (!doc) return;
    const parts = [t('office.simple')];
    if (doc.type === 'excel') parts.push(t('office.sheets', { count: doc.sheets.length }));
    if (doc.type === 'powerPoint') parts.push(t('office.slides', { count: doc.slides.length }));
    if ((doc.type !== 'excel' && doc.truncated) || (doc.type === 'excel' && doc.sheets[sheet]?.truncated)) parts.push(t('view.truncated'));
    onMeta(parts.join(' · '));
  }, [doc, sheet, onMeta, t]);

  if (error) return <div className="look-note">{error}</div>;
  if (!doc) return <div className="look-note">{t('view.loading')}</div>;
  if (doc.type === 'word')
    return (
      <div className="look-md-scroll">
        <Word blocks={doc.blocks} />
      </div>
    );
  if (doc.type === 'powerPoint')
    return (
      <div className="look-md-scroll">
        <div className="look-slides">
          {doc.slides.map((s, i) => (
            <section key={i} className="look-slide">
              <span className="no">{i + 1}</span>
              {s.length ? s.map((p, j) => (j === 0 ? <h3 key={j}>{p}</h3> : <p key={j}>{p}</p>)) : <p className="dim">{t('office.emptySlide')}</p>}
            </section>
          ))}
        </div>
      </div>
    );
  const current = doc.sheets[sheet];
  return (
    <div className="look-sheet">
      <div className="look-sheet-grid">{current ? <Table rows={current.rows} heads /> : null}</div>
      {doc.sheets.length > 1 && (
        <div className="look-sheet-tabs" role="tablist">
          {doc.sheets.map((s, i) => (
            <button key={i} type="button" role="tab" aria-selected={i === sheet} onClick={() => setSheet(i)}>
              {s.name}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
