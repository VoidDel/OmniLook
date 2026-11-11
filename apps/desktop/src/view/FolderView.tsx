// A folder: what is directly in it, and how much is below altogether.

import { FileIcon } from 'omni-ui';
import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';

import { api, type FolderPreview, type Shown } from '../lib/api';
import { formatDate, formatSize } from '../lib/format';

export function FolderView({ shown, onMeta }: { shown: Shown; onMeta: (meta: string) => void }) {
  const { t, i18n } = useTranslation();
  const [f, setF] = useState<FolderPreview | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let gone = false;
    api.readFolder(shown.id).then(
      (r) => !gone && setF(r),
      (e) => !gone && setError(String(e)),
    );
    return () => {
      gone = true;
    };
  }, [shown.id]);

  useEffect(() => {
    if (!f) return;
    const summary = t('folder.summary', { files: f.files, folders: f.folders, size: formatSize(f.bytes) });
    onMeta(f.complete ? summary : t('folder.atLeast', { summary }));
  }, [f, onMeta, t]);

  if (error) return <div className="look-note">{error}</div>;
  if (!f) return <div className="look-note">{t('view.loading')}</div>;
  if (!f.items.length) return <div className="look-note">{t('folder.empty')}</div>;
  return (
    <div className="look-list">
      {f.items.map((it) => (
        <div key={it.name} className="lr static">
          <FileIcon name={it.name} isDir={it.dir} />
          <span className="nm">{it.name}</span>
          <span className="dt">{formatDate(it.modified, i18n.language)}</span>
          <span className="sz">{it.dir ? '' : formatSize(it.size)}</span>
        </div>
      ))}
    </div>
  );
}
