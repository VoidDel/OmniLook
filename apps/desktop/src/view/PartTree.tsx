// The parts of a 3D model (lib/parts.ts), beside it: open and close assemblies, hide parts,
// pick one to light it up, double-click to look at it. Only the rows on screen exist.

import { useVirtualizer } from '@tanstack/react-virtual';
import { Icon } from 'omni-ui';
import { useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';

import { initiallyOpen, type PartNode, visibleRows } from '../lib/parts';

const ROW_H = 26;

export interface PartTreeProps {
  tree: PartNode;
  /** Node ids hidden by the user (their parts and all below). */
  hidden: Set<number>;
  selected: number | null;
  onToggle: (node: PartNode) => void;
  onShowAll: () => void;
  onSelect: (node: PartNode | null) => void;
  onFocus: (node: PartNode) => void;
}

export function PartTree({ tree, hidden, selected, onToggle, onShowAll, onSelect, onFocus }: PartTreeProps) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(() => initiallyOpen(tree));
  const rows = useMemo(() => visibleRows(tree, open), [tree, open]);
  const scroller = useRef<HTMLDivElement>(null);
  // The virtualizer's functions change identity every render; the rows are not memoized.
  // eslint-disable-next-line react-hooks/incompatible-library
  const v = useVirtualizer({ count: rows.length, getScrollElement: () => scroller.current, estimateSize: () => ROW_H, overscan: 12 });

  // A part picked on the model: its assemblies open, its row in view.
  useEffect(() => {
    if (selected === null) return;
    const path: PartNode[] = [];
    const find = (n: PartNode): boolean => {
      if (n.id === selected) return true;
      for (const c of n.children) {
        if (find(c)) {
          path.push(n);
          return true;
        }
      }
      return false;
    };
    if (!find(tree)) return;
    setOpen((o) => (path.every((n) => o.has(n.id)) ? o : new Set([...o, ...path.map((n) => n.id)])));
  }, [selected, tree]);
  useEffect(() => {
    const i = rows.findIndex((r) => r.node.id === selected);
    if (i >= 0) v.scrollToIndex(i, { align: 'auto' });
  }, [rows, selected, v]);

  const flip = (id: number) =>
    setOpen((o) => {
      const n = new Set(o);
      if (n.has(id)) n.delete(id);
      else n.add(id);
      return n;
    });

  return (
    <aside className="look-parts" onPointerDown={(e) => e.stopPropagation()} onDoubleClick={(e) => e.stopPropagation()}>
      <header>
        <b>{t('model.partList', { count: tree.parts })}</b>
        <button type="button" className="btn ghost" onClick={onShowAll} disabled={!hidden.size} title={t('model.showAll')}>
          {t('model.showAll')}
        </button>
      </header>
      <div className="look-parts-rows" ref={scroller}>
        <div style={{ height: v.getTotalSize(), position: 'relative' }}>
          {v.getVirtualItems().map((vr) => {
            const { node, depth } = rows[vr.index];
            const group = node.children.length > 0;
            const off = hidden.has(node.id);
            return (
              <div
                key={node.id}
                className={`pr${selected === node.id ? ' sel' : ''}${off ? ' off' : ''}`}
                style={{ transform: `translateY(${vr.start}px)`, paddingLeft: 6 + depth * 14 }}
                onClick={() => onSelect(selected === node.id ? null : node)}
                onDoubleClick={() => onFocus(node)}
                title={node.name}
              >
                {group ? (
                  <button
                    type="button"
                    className="tw"
                    aria-label={open.has(node.id) ? t('model.collapse') : t('model.expand')}
                    onClick={(e) => {
                      e.stopPropagation();
                      flip(node.id);
                    }}
                  >
                    <Icon name={open.has(node.id) ? 'chevronDown' : 'chevronRight'} />
                  </button>
                ) : (
                  <span className="tw" />
                )}
                <button
                  type="button"
                  className="eye"
                  aria-label={off ? t('model.show') : t('model.hide')}
                  title={off ? t('model.show') : t('model.hide')}
                  onClick={(e) => {
                    e.stopPropagation();
                    onToggle(node);
                  }}
                >
                  <Icon name={off ? 'eyeOff' : 'eye'} />
                </button>
                <span className="nm">{node.name}</span>
                {group && <span className="ct">{node.parts}</span>}
              </div>
            );
          })}
        </div>
      </div>
    </aside>
  );
}
