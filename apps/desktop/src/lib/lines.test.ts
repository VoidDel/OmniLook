import { describe, expect, it } from 'vitest';

import { cells, pieces, spansByLine, splitLines } from './lines';

describe('lines', () => {
  it('splits CRLF and LF text and keeps offsets', () => {
    const l = splitLines('ab\r\ncd\nefg');
    expect(l.lines).toEqual(['ab', 'cd', 'efg']);
    expect(l.starts).toEqual([0, 4, 7]);
    expect(l.columns).toBe(3);
    // Wide characters count twice.
    expect(splitLines('中文x').columns).toBe(5);
  });

  it('gives wide characters and symbols a cell each', () => {
    expect(cells('plain │─┐')).toEqual(['plain │─┐']);
    expect(cells('')).toEqual([]);
    expect(cells('PC 侧（a）→ b')).toEqual([
      'PC ',
      { text: '侧', cols: 2 },
      { text: '（', cols: 2 },
      'a',
      { text: '）', cols: 2 },
      { text: '→', cols: 1 },
      ' b',
    ]);
    // An ideograph beyond the BMP stays whole.
    expect(cells('𠮷')).toEqual([{ text: '𠮷', cols: 2 }]);
  });

  it('maps ranges onto lines and cuts pieces', () => {
    const { lines, starts } = splitLines('let a\n// c\n');
    // "let" keyword (4) at 0, comment (1) at 6.
    const by = spansByLine([0, 3, 4, 6, 4, 1], starts);
    expect(by.get(0)).toEqual([0, 3, 4]);
    expect(by.get(1)).toEqual([0, 4, 1]);
    expect(pieces(lines[0], by.get(0))).toEqual([
      { text: 'let', cls: 4 },
      { text: ' a', cls: 0 },
    ]);
    expect(pieces('plain', undefined)).toEqual([{ text: 'plain', cls: 0 }]);
  });
});
