import { describe, expect, it } from 'vitest';

import { buildTree, flatten } from './tree';

const e = (path: string, size = 0, dir = false) => ({ path, size, dir, encrypted: false });

describe('tree', () => {
  it('derives folders, sums sizes and orders folders first', () => {
    const root = buildTree([e('b.txt', 2), e('src/main.rs', 10), e('src/lib/a.rs', 5), e('empty/', 0, true)]);
    const rows = flatten(root, new Set());
    expect(rows.map((r) => `${'  '.repeat(r.depth)}${r.name}`)).toEqual(['empty', 'src', '  lib', '    a.rs', '  main.rs', 'b.txt']);
    const src = rows.find((r) => r.path === 'src')!;
    expect([src.size, src.files]).toEqual([15, 2]);
  });

  it('hides the inside of collapsed folders', () => {
    const rows = flatten(buildTree([e('src/main.rs'), e('a.txt')]), new Set(['src']));
    expect(rows.map((r) => r.path)).toEqual(['src', 'a.txt']);
  });
});
