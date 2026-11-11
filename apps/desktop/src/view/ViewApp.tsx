// The preview window (docs/development-plan.md §4.3, §5): a title bar with the file's name and
// its actions, the preview, and a status line. Shown without taking the focus; once clicked, the
// hotkey or Escape closes it and the arrow keys move to the next file, as in Explorer.

import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { FileIcon, Icon, Segmented } from 'omni-ui';
import { useCallback, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';

import { api, type Shown } from '../lib/api';
import { formatDate, formatSize } from '../lib/format';
import { nextFrame } from '../lib/frame';
import { HAS_SOURCE, Preview } from './Preview';

/** Rendered or source, kept for this viewer between previews (browser storage, best effort). */
const SOURCE_KEY = 'omnilook.source';
function loadSource(): boolean {
  try {
    return localStorage.getItem(SOURCE_KEY) === '1';
  } catch {
    return false;
  }
}

export function ViewApp() {
  const { t, i18n } = useTranslation();
  const [shown, setShown] = useState<Shown | null>(null);
  // What the preview says about the file (dimensions, encoding, counts), for the status line.
  const [meta, setMeta] = useState('');
  const onMeta = useCallback((m: string) => setMeta(m), []);
  const firstPaint = useRef(false);
  const [source, setSourceState] = useState(loadSource);
  const setSource = (on: boolean) => {
    setSourceState(on);
    setMeta('');
    try {
      localStorage.setItem(SOURCE_KEY, on ? '1' : '0');
    } catch {
      /* not kept */
    }
  };

  // Listen first, then ask for the first file: a file sent before the listener is up is lost.
  useEffect(() => {
    let off: (() => void) | undefined;
    let gone = false;
    void listen<Shown>('show', (e) => {
      setMeta('');
      setShown(e.payload);
    }).then((u) => {
      if (gone) return u();
      off = u;
      void api.viewReady().then((s) => s && setShown(s));
    });
    return () => {
      gone = true;
      off?.();
    };
  }, []);

  // The window shows once the first file is drawn.
  useEffect(() => {
    if (!shown || firstPaint.current) return;
    firstPaint.current = true;
    nextFrame(() => nextFrame(() => void api.viewShow()));
  }, [shown]);

  useEffect(() => {
    if (shown) document.title = `${shown.title ?? shown.name} - Omni Look`;
  }, [shown]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement;
      if (e.key === 'Escape' || (e.key === ' ' && e.ctrlKey)) {
        e.preventDefault();
        void api.viewClose();
      } else if ((e.key === 'ArrowLeft' || e.key === 'ArrowRight') && !target.closest('video, audio, input')) {
        e.preventDefault();
        void api.viewNav(e.key === 'ArrowLeft' ? -1 : 1);
      } else if (e.key === 'Enter' && e.ctrlKey) {
        void api.openDefault();
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);

  const name = shown ? (shown.title ?? shown.name) : '';
  return (
    <div className="look">
      <header className="look-bar" data-tauri-drag-region>
        <div className="look-name" data-tauri-drag-region title={shown?.path}>
          {shown && <FileIcon name={shown.name} isDir={shown.kind === 'folder'} />}
          <b data-tauri-drag-region>{name}</b>
        </div>
        <div className="look-actions">
          {shown && HAS_SOURCE.includes(shown.kind) && (
            <Segmented
              value={source ? 'source' : 'rendered'}
              label={t('view.mode')}
              options={[
                ['rendered', t('view.rendered')],
                ['source', t('view.source')],
              ]}
              onChange={(m) => setSource(m === 'source')}
            />
          )}
          <button type="button" className="btn ghost" onClick={() => void api.openDefault()} title={t('view.openDefaultHint')}>
            <Icon name="external" />
            <span>{t('view.openDefault')}</span>
          </button>
          <div className="winctl">
            <button type="button" onClick={() => void getCurrentWindow().minimize()} aria-label={t('common.minimize')}>
              <Icon name="minus" />
            </button>
            <button type="button" className="close" onClick={() => void api.viewClose()} aria-label={t('common.close')}>
              <Icon name="x" />
            </button>
          </div>
        </div>
      </header>
      <main className="look-body">{shown && <Preview key={`${shown.id}-${source}`} shown={shown} onMeta={onMeta} source={source} />}</main>
      <footer className="look-status">
        {shown && (
          <>
            <span>{formatSize(shown.size)}</span>
            <span>{formatDate(shown.modified, i18n.language)}</span>
            {meta && <span>{meta}</span>}
            {/* A caller's title (OmniZip: "archive › path") stands for a file in its cache,
                whose path means nothing to the reader. */}
            {!shown.title && (
              <span className="grow ellipsis" title={shown.path}>
                {shown.path}
              </span>
            )}
          </>
        )}
      </footer>
    </div>
  );
}
