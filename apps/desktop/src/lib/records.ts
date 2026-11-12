// Firmware records (Intel HEX, Motorola S-records): each line on screen is cut into its fields
// here, so a 20 MB file costs no more than the lines shown. The checks (length, checksum) and the
// summary come from Rust (crates/omnilook-core/src/records.rs).

import type { Piece } from './lines';

export type RecordFormat = 'intelHex' | 'sRecord';

export interface BadRecord {
  /** Line index from 0. */
  line: number;
  problem: 'checksum' | 'length' | 'syntax';
  /** The right checksum byte, for a checksum problem. */
  expected: number | null;
}

export interface RecordSummary {
  format: RecordFormat;
  records: number;
  dataBytes: number;
  /** `[start, end)`, merged and sorted; the first 64. */
  ranges: [number, number][];
  rangeCount: number;
  /** From the first byte of data to past the last. */
  span: [number, number] | null;
  entry: number | null;
  header: string | null;
  /** Intel HEX: the end-of-file record was there. */
  ended: boolean;
  bad: BadRecord[];
  badCount: number;
  /** The file was too large to check all of it. */
  partial: boolean;
}

// Token classes (lines.ts, `.tk<n>` in app.css).
const MARK = 4; // keyword: start mark and record type
const ADDRESS = 5; // function
const CHECKSUM = 6; // type
const PAYLOAD = 7; // constant: what non-data records carry
const TEXT = 2; // string: the S0 header
const COUNT = 9; // attribute
export const ERROR = 11;

/** Address bytes of an S-record type. */
function srecAddress(type: string): number | null {
  if ('0159'.includes(type)) return 2;
  if ('268'.includes(type)) return 3;
  if ('37'.includes(type)) return 4;
  return null;
}

/**
 * `line` cut into its fields. A bad record gets the error class, on its checksum or (when the
 * record itself is wrong) all of it, with `title` saying why.
 */
export function recordPieces(line: string, format: RecordFormat, bad?: BadRecord, title?: string): Piece[] {
  const start = line.length - line.trimStart().length;
  const end = line.trimEnd().length;
  const r = line.slice(start, end);
  const fields: [number, number, number][] = []; // [from, to, class] within r
  if (bad && bad.problem !== 'checksum') {
    if (r) fields.push([0, r.length, ERROR]);
  } else if (format === 'intelHex' && r.startsWith(':') && r.length >= 11) {
    const data = r.slice(7, 9) === '00' ? 0 : PAYLOAD;
    fields.push([0, 1, MARK], [1, 3, COUNT], [3, 7, ADDRESS], [7, 9, MARK], [9, r.length - 2, data]);
    fields.push([r.length - 2, r.length, bad ? ERROR : CHECKSUM]);
  } else if (format === 'sRecord' && r.startsWith('S') && r.length >= 10) {
    const type = r[1];
    const n = srecAddress(type);
    if (n == null) return [{ text: line, cls: 0 }];
    const data = '123'.includes(type) ? 0 : type === '0' ? TEXT : PAYLOAD;
    const a = 4 + 2 * n;
    fields.push([0, 2, MARK], [2, 4, COUNT], [4, a, ADDRESS], [a, r.length - 2, data]);
    fields.push([r.length - 2, r.length, bad ? ERROR : CHECKSUM]);
  } else {
    return [{ text: line, cls: 0 }];
  }
  const out: Piece[] = [];
  if (start > 0) out.push({ text: line.slice(0, start), cls: 0 });
  for (const [from, to, cls] of fields) {
    if (to <= from) continue;
    const piece: Piece = { text: r.slice(from, to), cls };
    if (cls === ERROR && title) piece.title = title;
    out.push(piece);
  }
  if (end < line.length) out.push({ text: line.slice(end), cls: 0 });
  return out;
}

/** `0x08000000`; addresses up to `0xFFFF` keep four digits, unless `wide` (as another is). */
export function formatAddress(n: number, wide = false): string {
  return `0x${n.toString(16).toUpperCase().padStart(wide || n > 0xffff ? 8 : 4, '0')}`;
}

/** `0x08000000–0x0801FFFF`: the first byte to the last, of everything the data covers. */
export function addressSpan(s: RecordSummary): string | null {
  if (!s.span) return null;
  const last = s.span[1] - 1;
  const wide = last > 0xffff;
  return `${formatAddress(s.span[0], wide)}–${formatAddress(last, wide)}`;
}
