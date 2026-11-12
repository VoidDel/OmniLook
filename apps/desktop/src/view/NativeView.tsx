// SolidWorks and Inventor files: closed formats, shown by the picture the program saved in them
// (crates/omnilook-core/src/thumbnail.rs), with what kind of file it is. Where SolidWorks or
// Inventor is installed, their own previewer is used instead (HandlerView).

import { FileIcon } from 'omni-ui';
import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';

import { api, type Shown } from '../lib/api';
import { formatSize } from '../lib/format';

/** `solidWorksPart`… by extension: the label's key. */
function kindOf(name: string): string {
  const ext = name.toLowerCase().split('.').pop() ?? '';
  return (
    {
      sldprt: 'solidWorksPart',
      sldasm: 'solidWorksAssembly',
      slddrw: 'solidWorksDrawing',
      ipt: 'inventorPart',
      iam: 'inventorAssembly',
      idw: 'inventorDrawing',
      ipn: 'inventorPresentation',
    }[ext] ?? 'solidWorksPart'
  );
}

export function NativeView({ shown, onMeta }: { shown: Shown; onMeta: (meta: string) => void }) {
  const { t } = useTranslation();
  const [url, setUrl] = useState<string | null>(null);
  const [failed, setFailed] = useState(false);
  const kind = t(`native.${kindOf(shown.name)}`);

  useEffect(() => {
    let gone = false;
    let made: string | null = null;
    api.readNative(shown.id).then(
      (png) => {
        if (gone) return;
        made = URL.createObjectURL(new Blob([png], { type: 'image/png' }));
        setUrl(made);
      },
      () => !gone && setFailed(true),
    );
    return () => {
      gone = true;
      if (made) URL.revokeObjectURL(made);
    };
  }, [shown.id]);

  useEffect(() => {
    if (failed) onMeta(`${kind} · ${t('native.none')}`);
  }, [failed, kind, onMeta, t]);

  if (failed) {
    return (
      <div className="look-fallback">
        <FileIcon name={shown.name} isDir={false} big />
        <b>{shown.name}</b>
        <span>
          {kind} · {formatSize(shown.size)}
        </span>
        <span className="look-fallback-note">{t('native.none')}</span>
        <button type="button" className="btn primary" onClick={() => void api.openDefault()}>
          {t('view.openDefault')}
        </button>
      </div>
    );
  }
  if (!url) return <div className="look-note">{t('view.loading')}</div>;
  return (
    <div className="look-native">
      <img
        src={url}
        alt={shown.name}
        draggable={false}
        onLoad={(e) => onMeta(`${kind} · ${t('native.preview', { w: e.currentTarget.naturalWidth, h: e.currentTarget.naturalHeight })}`)}
      />
      <span className="look-native-note">{t('native.note')}</span>
    </div>
  );
}
