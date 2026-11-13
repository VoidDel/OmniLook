// Text split into lines, with the highlighter's class ranges (absolute UTF-16 offsets) turned into
// per-line pieces the text view renders.

export interface Piece {
  text: string;
  /** Token class (1–11), 0 for none. */
  cls: number;
  /** Why the piece is marked (a bad record). */
  title?: string;
}

export interface Lines {
  lines: string[];
  /** Offset of each line's first character in the whole text. */
  starts: number[];
  /** Widest line in columns (wide characters count twice), for the scroll width. */
  columns: number;
}

const WIDE = /[ᄀ-ᅟ⺀-꓏가-힣豈-﫿︰-﹏＀-｠￠-￦]/g;

export function splitLines(text: string): Lines {
  const lines = text.split('\n');
  const starts = new Array<number>(lines.length);
  let at = 0;
  let columns = 0;
  for (let i = 0; i < lines.length; i++) {
    starts[i] = at;
    at += lines[i].length + 1;
    if (lines[i].endsWith('\r')) lines[i] = lines[i].slice(0, -1);
    const l = lines[i];
    const cols = l.length + (l.match(WIDE)?.length ?? 0) + (l.split('\t').length - 1) * 3;
    if (cols > columns) columns = cols;
  }
  return { lines, starts, columns };
}

/** A character that gets a box of its own, `cols` columns wide whatever font draws it. */
export interface Cell {
  text: string;
  cols: 1 | 2;
}

// The monospaced faces have neither CJK nor most symbols, so these come from a fallback font whose
// glyphs are not a whole number of columns wide. Group 1: wide characters (as WIDE, plus the
// ideographs beyond the BMP), two columns. Group 2: arrows, mathematical and technical symbols
// and geometric shapes, one column. Box drawing is not here: every monospaced face has it.
const CELL =
  /([\u1100-\u115F\u2E80-\uA4CF\uAC00-\uD7A3\uF900-\uFAFF\uFE30-\uFE4F\uFF00-\uFF60\uFFE0-\uFFE6\u{20000}-\u{3FFFD}])|([\u2190-\u23FF\u25A0-\u25FF])/gu;

/** `text` cut at the characters that need a cell: plain runs as strings, the rest one by one. */
export function cells(text: string): (string | Cell)[] {
  const out: (string | Cell)[] = [];
  let at = 0;
  for (const m of text.matchAll(CELL)) {
    if (m.index > at) out.push(text.slice(at, m.index));
    out.push({ text: m[0], cols: m[1] ? 2 : 1 });
    at = m.index + m[0].length;
  }
  if (at < text.length) out.push(text.slice(at));
  return out;
}

/** Class ranges by line: line index → [offset in line, length, class] triples. */
export function spansByLine(spans: number[], starts: number[]): Map<number, number[]> {
  const out = new Map<number, number[]>();
  let line = 0;
  for (let i = 0; i + 2 < spans.length; i += 3) {
    const [start, len, cls] = [spans[i], spans[i + 1], spans[i + 2]];
    while (line + 1 < starts.length && starts[line + 1] <= start) line++;
    const list = out.get(line) ?? [];
    list.push(start - starts[line], len, cls);
    out.set(line, list);
  }
  return out;
}

/** One line cut into pieces by its class ranges. */
export function pieces(line: string, ranges: number[] | undefined): Piece[] {
  if (!ranges?.length) return [{ text: line, cls: 0 }];
  const out: Piece[] = [];
  let at = 0;
  for (let i = 0; i + 2 < ranges.length; i += 3) {
    const [start, len, cls] = [ranges[i], ranges[i + 1], ranges[i + 2]];
    if (start > at) out.push({ text: line.slice(at, start), cls: 0 });
    out.push({ text: line.slice(start, start + len), cls });
    at = start + len;
  }
  if (at < line.length) out.push({ text: line.slice(at), cls: 0 });
  return out;
}
