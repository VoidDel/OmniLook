// One file's preview, by its kind (docs/development-plan.md §5.1). Images, PDF and media come
// from look:// (the file only, by a one-time token); text comes through a command.

import { FileIcon } from 'omni-ui';
import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';

import { api, type Shown } from '../lib/api';
import { formatSize } from '../lib/format';

function TextPreview({ shown }: { shown: Shown }) {
  const [text, setText] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    api.readText(shown.id).then(setText, (e) => setError(String(e)));
  }, [shown.id]);
  if (error) return <Fallback shown={shown} />;
  return <pre className="look-text">{text ?? ''}</pre>;
}

/** No preview for this kind (yet): the file's icon, name and size, and "open". */
function Fallback({ shown }: { shown: Shown }) {
  const { t } = useTranslation();
  return (
    <div className="look-fallback">
      <FileIcon name={shown.name} isDir={shown.kind === 'folder'} />
      <b>{shown.name}</b>
      <span>{formatSize(shown.size)}</span>
      <button type="button" className="btn primary" onClick={() => void api.openDefault()}>
        {t('view.openDefault')}
      </button>
    </div>
  );
}

export function Preview({ shown }: { shown: Shown }) {
  switch (shown.kind) {
    case 'text':
      return <TextPreview shown={shown} />;
    case 'image':
    case 'svg':
      // An SVG as an <img> runs no script and loads nothing from elsewhere.
      return (
        <div className="look-image">
          <img src={shown.url} alt={shown.name} draggable={false} />
        </div>
      );
    case 'pdf':
      return <iframe className="look-frame" src={shown.url} title={shown.name} />;
    case 'video':
      return (
        <div className="look-media">
          <video src={shown.url} controls autoPlay />
        </div>
      );
    case 'audio':
      return (
        <div className="look-media">
          <audio src={shown.url} controls autoPlay />
        </div>
      );
    default:
      return <Fallback shown={shown} />;
  }
}
