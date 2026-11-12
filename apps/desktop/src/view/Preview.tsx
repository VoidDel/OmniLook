// One file's preview, by its kind (docs/development-plan.md §5.1). Images, PDF and media come
// from look:// (the file only, by a one-time token); the rest through commands.

import { FileIcon } from 'omni-ui';
import { lazy, Suspense, useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';

import { api, type HexPreview, type Shown } from '../lib/api';
import { formatSize } from '../lib/format';
import { ArchiveView } from './ArchiveView';
import { CadView } from './CadView';
import { FolderView } from './FolderView';
import { FontView } from './FontView';
import { HandlerView } from './HandlerView';
import { MarkdownView } from './MarkdownView';
import { MediaView } from './MediaView';
import { NativeView } from './NativeView';
import { OfficeView } from './OfficeView';
import { TextView } from './TextView';

// three.js only for the previews that draw with it.
const ModelView = lazy(() => import('./ModelView'));

/** No preview for this kind (yet): the file's icon, name and size, and "open". */
function Fallback({ shown, note }: { shown: Shown; note?: string }) {
  const { t } = useTranslation();
  return (
    <div className="look-fallback">
      <FileIcon name={shown.name} isDir={shown.kind === 'folder'} big />
      <b>{shown.name}</b>
      <span>{formatSize(shown.size)}</span>
      {note && <span className="look-fallback-note">{note}</span>}
      <button type="button" className="btn primary" onClick={() => void api.openDefault()}>
        {t('view.openDefault')}
      </button>
    </div>
  );
}

function HexView({ shown, onMeta }: { shown: Shown; onMeta: (meta: string) => void }) {
  const { t } = useTranslation();
  const [h, setH] = useState<HexPreview | null>(null);
  useEffect(() => {
    let gone = false;
    void api.readHex(shown.id).then((r) => !gone && setH(r));
    return () => {
      gone = true;
    };
  }, [shown.id]);
  useEffect(() => {
    if (h) onMeta(h.truncated ? t('hex.first') : t('hex.binary'));
  }, [h, onMeta, t]);
  return <pre className="look-hex">{h?.dump ?? ''}</pre>;
}

function ImageView({ shown, onMeta }: { shown: Shown; onMeta: (meta: string) => void }) {
  const { t } = useTranslation();
  const [failed, setFailed] = useState(false);
  // HEIC is decoded by Windows' HEIF extension: when that fails, it is most likely missing.
  if (failed) return <Fallback shown={shown} note={/.(heic|heif|hif)$/i.test(shown.name) ? t('view.heifNeeded') : undefined} />;
  return (
    <div className="look-image">
      {/* An SVG as an <img> runs no script and loads nothing from elsewhere. */}
      <img
        src={shown.url}
        alt={shown.name}
        draggable={false}
        onLoad={(e) => onMeta(`${e.currentTarget.naturalWidth} × ${e.currentTarget.naturalHeight}`)}
        onError={() => setFailed(true)}
      />
    </div>
  );
}

function Loading() {
  const { t } = useTranslation();
  return <div className="look-note">{t('view.loading')}</div>;
}

export const HAS_SOURCE: Shown['kind'][] = ['markdown', 'svg'];

export function Preview({ shown, onMeta, source }: { shown: Shown; onMeta: (meta: string) => void; source: boolean }) {
  if (source && HAS_SOURCE.includes(shown.kind)) return <TextView shown={shown} onMeta={onMeta} />;
  switch (shown.kind) {
    case 'text':
      return <TextView shown={shown} onMeta={onMeta} />;
    case 'markdown':
      return <MarkdownView shown={shown} onMeta={onMeta} />;
    case 'image':
    case 'svg':
      return <ImageView shown={shown} onMeta={onMeta} />;
    case 'pdf':
      return <iframe className="look-frame" src={shown.url} title={shown.name} />;
    case 'video':
    case 'audio':
      return <MediaView shown={shown} onMeta={onMeta} />;
    case 'archive':
      return <ArchiveView shown={shown} onMeta={onMeta} />;
    case 'folder':
      return <FolderView shown={shown} onMeta={onMeta} />;
    case 'font':
      return <FontView shown={shown} onMeta={onMeta} />;
    case 'cad':
      return <CadView shown={shown} onMeta={onMeta} />;
    case 'model':
      return (
        <Suspense fallback={<Loading />}>
          <ModelView shown={shown} onMeta={onMeta} />
        </Suspense>
      );
    case 'native':
      return <NativeView shown={shown} onMeta={onMeta} />;
    case 'office':
      return <OfficeView shown={shown} onMeta={onMeta} />;
    case 'handler':
      return <HandlerView shown={shown} onMeta={onMeta} />;
    case 'binary':
      return <HexView shown={shown} onMeta={onMeta} />;
    default:
      return <Fallback shown={shown} />;
  }
}
