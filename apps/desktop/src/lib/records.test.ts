import { describe, expect, it } from 'vitest';

import { addressSpan, ERROR, formatAddress, recordPieces, type RecordSummary } from './records';

const cut = (ps: { text: string; cls: number }[]) => ps.map((p) => [p.text, p.cls]);

describe('records', () => {
  it('cuts Intel HEX records into fields', () => {
    expect(cut(recordPieces(':0400100010111213A6', 'intelHex'))).toEqual([
      [':', 4],
      ['04', 9],
      ['0010', 5],
      ['00', 4],
      ['10111213', 0],
      ['A6', 6],
    ]);
    // An extended address record: its payload is not data. Trailing CR stays plain.
    expect(cut(recordPieces(':020000040800F2\r', 'intelHex'))).toEqual([
      [':', 4],
      ['02', 9],
      ['0000', 5],
      ['04', 4],
      ['0800', 7],
      ['F2', 6],
      ['\r', 0],
    ]);
    // End of file: no data at all.
    expect(cut(recordPieces(':00000001FF', 'intelHex')).map((p) => p[0])).toEqual([':', '00', '0000', '01', 'FF']);
  });

  it('cuts S-records by their address size', () => {
    expect(cut(recordPieces('S1130000285F245F2212226A000424290008237C2A', 'sRecord')).map((p) => p[0])).toEqual([
      'S1',
      '13',
      '0000',
      '285F245F2212226A000424290008237C',
      '2A',
    ]);
    const s3 = cut(recordPieces('S3250003FFE00000000000000000000000000000000000000000000000000000000000000000F8', 'sRecord'));
    expect(s3[2]).toEqual(['0003FFE0', 5]);
    // The S0 header's bytes are text.
    expect(cut(recordPieces('S00F000068656C6C6F202020202000003C', 'sRecord'))[3]).toEqual(['68656C6C6F20202020200000', 2]);
    // S4 does not exist.
    expect(cut(recordPieces('S41300000000', 'sRecord'))).toEqual([['S41300000000', 0]]);
  });

  it('marks bad records', () => {
    const wrongSum = recordPieces(':0400100010111213A7', 'intelHex', { line: 0, problem: 'checksum', expected: 0xa6 }, 'why');
    expect(wrongSum.at(-1)).toEqual({ text: 'A7', cls: ERROR, title: 'why' });
    expect(wrongSum[1].title).toBeUndefined();
    // A record that is wrong as a whole is marked whole.
    expect(recordPieces('  :04001000101112', 'intelHex', { line: 3, problem: 'length', expected: null }, 'why')).toEqual([
      { text: '  ', cls: 0 },
      { text: ':04001000101112', cls: ERROR, title: 'why' },
    ]);
    // Lines that are not records stay plain.
    expect(recordPieces('', 'intelHex')).toEqual([{ text: '', cls: 0 }]);
    expect(recordPieces('hello', 'sRecord')).toEqual([{ text: 'hello', cls: 0 }]);
  });

  it('formats addresses and the span', () => {
    expect(formatAddress(0x1f)).toBe('0x001F');
    expect(formatAddress(0x8000000)).toBe('0x08000000');
    const s = { span: [0x8000000, 0x8020000] } as RecordSummary;
    expect(addressSpan(s)).toBe('0x08000000–0x0801FFFF');
    // Both ends as wide as the wider one.
    expect(addressSpan({ span: [0x8410, 0x40000] } as RecordSummary)).toBe('0x00008410–0x0003FFFF');
    expect(addressSpan({ span: [0xc60, 0xd0c] } as RecordSummary)).toBe('0x0C60–0x0D0B');
    expect(addressSpan({ span: null } as RecordSummary)).toBeNull();
  });
});
