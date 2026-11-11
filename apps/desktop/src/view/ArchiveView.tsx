// An archive: the folder tree of what is in it, read with OmniZip's formats. Folders open and
// close with a click.

import { useVirtualizer } from '@tanstack/react-virtual';
import { FileIcon, Icon } from 'omni-ui';
import { useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';

import { api, type ArchivePreview, type Shown } from '../lib/api';
import { formatSize } from '../lib/format';
import { buildTree, flatten } from '../lib/tree';

const ROW_H = 28;

export function ArchiveView({ shown, onMeta }: { shown: Shown; onMeta: (meta: string) => void }) {
  const { t, i18n } = useTranslation();
  const [a, setA] = useState<ArchivePreview | null>(null);
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());
  const scroller = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let gone = false;
    void api.readArchive(shown.id, i18n.language).then((r) => !gone && setA(r));
    return () => {
      gone = true;
    };
  }, [shown.id, i18n.language]);

  const tree = useMemo(() => (a ? buildTree(a.entries) : null), [a]);
  const rows = useMemo(() => (tree ? flatten(tree, collapsed) : []), [tree, collapsed]);

  useEffect(() => {
    if (!a || a.error) return;
    const parts = [a.format.toUpperCase(), t('archive.summary', { files: a.files, folders: a.folders, size: formatSize(a.size) })];
    if (a.encrypted) parts.push(t('archive.encrypted'));
    if (a.truncated) parts.push(t('archive.truncated'));
    onMeta(parts.join(' · '));
  }, [a, onMeta, t]);

  // The virtualizer's functions change identity every render; the rows are not memoized.
  // eslint-disable-next-line react-hooks/incompatible-library
  const v = useVirtualizer({
    count: rows.length,
    getScrollElement: () => scroller.current,
    estimateSize: () => ROW_H,
    overscan: 12,
  });

  if (!a) return <div className="look-note">{t('view.loading')}</div>;
  if (a.error) {
    return (
      <div className="look-note">
        <Icon name={a.error === 'passwordRequired' ? 'lock' : 'alert'} />
        <span>{a.error === 'passwordRequired' ? t('archive.passwordRequired') : t('archive.error')}</span>
      </div>
    );
  }
  const toggle = (path: string) =>
    setCollapsed((c) => {
      const n = new Set(c);
      if (n.has(path)) n.delete(path);
      else n.add(path);
      return n;
    });
  return (
    <div className="look-list" ref={scroller}>
      <div style={{ height: v.getTotalSize(), position: 'relative' }}>
        {v.getVirtualItems().map((vr) => {
          const r = rows[vr.index];
          return (
            <div
              key={r.path}
              className={`lr${r.dir ? ' dir' : ''}`}
              style={{ transform: `translateY(${vr.start}px)`, paddingLeft: 12 + r.depth * 18 }}
              onClick={r.dir ? () => toggle(r.path) : undefined}
            >
              {r.dir ? <Icon name={collapsed.has(r.path) ? 'chevronRight' : 'chevronDown'} className="tw" /> : <span className="tw" />}
              <FileIcon name={r.name} isDir={r.dir} />
              <span className="nm" title={r.path}>
                {r.name}
              </span>
              {r.encrypted && <Icon name="lock" className="enc" />}
              <span className="sz">{r.dir ? t('archive.folderFiles', { count: r.files }) : formatSize(r.size)}</span>
            </div>
          );
        })}
      </div>
    </div>
  );
}
