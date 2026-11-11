import { describe, expect, it } from 'vitest';

import { pieces, spansByLine, splitLines } from './lines';

describe('lines', () => {
  it('splits CRLF and LF text and keeps offsets', () => {
    const l = splitLines('ab\r\ncd\nefg');
    expect(l.lines).toEqual(['ab', 'cd', 'efg']);
    expect(l.starts).toEqual([0, 4, 7]);
    expect(l.columns).toBe(3);
    // Wide characters count twice.
    expect(splitLines('中文x').columns).toBe(5);
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
