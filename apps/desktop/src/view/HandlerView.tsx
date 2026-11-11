// A system preview handler (docs/development-plan.md §5.2): the one Explorer's preview pane uses
// for Word, Excel, PowerPoint, fonts and the like. It draws in a native window that Rust lays over
// this element; the element only holds its place, says "loading", and keeps the window on it as
// the preview window is resized.

import { FileIcon } from 'omni-ui';
import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';

import { api, type HostArea, type Shown } from '../lib/api';
import { formatSize } from '../lib/format';

/** `rgb(r, g, b)` as a Windows COLORREF (0x00BBGGRR). */
function colorref(css: string): number | null {
  const m = css.match(/rgba?\((\d+),\s*(\d+),\s*(\d+)/);
  return m ? Number(m[1]) | (Number(m[2]) << 8) | (Number(m[3]) << 16) : null;
}

type State = 'loading' | 'shown' | 'none' | 'failed' | 'timeout';

export function HandlerView({ shown, onMeta }: { shown: Shown; onMeta: (meta: string) => void }) {
  const { t } = useTranslation();
  const box = useRef<HTMLDivElement>(null);
  const [state, setState] = useState<State>('loading');

  useEffect(() => {
    const el = box.current;
    if (!el) return;
    let gone = false;
    const area = (): HostArea => {
      const r = el.getBoundingClientRect();
      const k = window.devicePixelRatio || 1;
      return { x: Math.round(r.left * k), y: Math.round(r.top * k), w: Math.round(r.width * k), h: Math.round(r.height * k) };
    };
    const style = getComputedStyle(el);
    void api.hostShow(shown.id, area(), colorref(style.backgroundColor), colorref(style.color)).then(
      () => {
        if (gone) return;
        setState('shown');
        onMeta(t('view.handler'));
      },
      (e: unknown) => {
        if (gone) return;
        const msg = String(e);
        setState(msg.includes('no preview handler') ? 'none' : msg.includes('timeout') ? 'timeout' : 'failed');
      },
    );
    const resized = new ResizeObserver(() => void api.hostArea(area()));
    resized.observe(el);
    return () => {
      gone = true;
      resized.disconnect();
      void api.hostUnload();
    };
  }, [shown.id, onMeta, t]);

  return (
    <div ref={box} className="look-host">
      {state === 'loading' && <div className="look-note">{t('view.loading')}</div>}
      {(state === 'none' || state === 'failed' || state === 'timeout') && (
        <div className="look-fallback">
          <FileIcon name={shown.name} isDir={false} big />
          <b>{shown.name}</b>
          <span>{formatSize(shown.size)}</span>
          <span className="look-fallback-note">
            {state === 'none' ? t('view.noHandler') : state === 'timeout' ? t('view.handlerTimeout') : t('view.handlerFailed')}
          </span>
          <button type="button" className="btn primary" onClick={() => void api.openDefault()}>
            {t('view.openDefault')}
          </button>
        </div>
      )}
    </div>
  );
}
