// An archive's flat entry list as a folder tree, flattened again into rows for the list view.

import type { ArchiveEntry } from './api';

export interface Row {
  /** Full path, the row's key. */
  path: string;
  name: string;
  depth: number;
  dir: boolean;
  /** Files: their size; folders: everything below. */
  size: number;
  encrypted: boolean;
  /** Folders: how many files below. */
  files: number;
}

interface Node {
  name: string;
  path: string;
  dir: boolean;
  size: number;
  encrypted: boolean;
  files: number;
  children: Map<string, Node>;
}

function node(name: string, path: string, dir: boolean): Node {
  return { name, path, dir, size: 0, encrypted: false, files: 0, children: new Map() };
}

export function buildTree(entries: ArchiveEntry[]): Node {
  const root = node('', '', true);
  for (const e of entries) {
    const parts = e.path.split('/').filter(Boolean);
    let at = root;
    const chain: Node[] = [root];
    parts.forEach((part, i) => {
      const last = i === parts.length - 1;
      const path = parts.slice(0, i + 1).join('/');
      let next = at.children.get(part);
      if (!next) {
        next = node(part, path, !last || e.dir);
        at.children.set(part, next);
      }
      at = next;
      chain.push(next);
    });
    if (!e.dir && parts.length) {
      at.size = e.size;
      at.encrypted = e.encrypted;
      for (const n of chain.slice(0, -1)) {
        n.size += e.size;
        n.files += 1;
      }
    }
  }
  return root;
}

/** Rows in display order (folders first, then by name), skipping the insides of `collapsed`. */
export function flatten(root: Node, collapsed: Set<string>): Row[] {
  const rows: Row[] = [];
  const walk = (n: Node, depth: number) => {
    const kids = [...n.children.values()].sort((a, b) =>
      a.dir !== b.dir ? (a.dir ? -1 : 1) : a.name.localeCompare(b.name, undefined, { numeric: true }),
    );
    for (const k of kids) {
      rows.push({ path: k.path, name: k.name, depth, dir: k.dir, size: k.size, encrypted: k.encrypted, files: k.files });
      if (k.dir && !collapsed.has(k.path)) walk(k, depth + 1);
    }
  };
  walk(root, 0);
  return rows;
}
