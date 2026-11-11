// Text split into lines, with the highlighter's class ranges (absolute UTF-16 offsets) turned into
// per-line pieces the text view renders.

export interface Piece {
  text: string;
  /** Token class (1–10), 0 for none. */
  cls: number;
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
