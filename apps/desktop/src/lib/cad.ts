// CAD drawings (crates/omnilook-core/src/cad.rs): the binary the Rust side sends, read back, and
// the sums the canvas needs (fitting the drawing, colours on a light or dark ground).

/**
 * Lines of one colour (`[n, x1, y1, … xn, yn]` repeated) or areas (triangles), as a range of
 * `floats`.
 */
export interface CadRun {
  color: number;
  alpha: number;
  start: number;
  end: number;
}

export interface CadLabel {
  text: string;
  x: number;
  y: number;
  /** The text's direction and its up, each as long as the text is high. */
  xa: [number, number];
  ya: [number, number];
  /** 0 left, 1 center, 2 right. */
  h: number;
  /** 0 baseline, 1 bottom, 2 middle, 3 top. */
  v: number;
  color: number;
}

export interface CadDrawing {
  format: 'DWG' | 'DXF';
  version: string | null;
  units: string | null;
  objects: number;
  layers: number;
  paper: boolean;
  origin: [number, number];
  bounds: [number, number, number, number] | null;
  /** 0xRRGGBB, or FOREGROUND. */
  colors: number[];
  strokes: CadRun[];
  fills: CadRun[];
  labels: CadLabel[];
  truncated: boolean;
  floats: Float32Array;
}

/** White on dark, black on light. */
export const FOREGROUND = 0x1000000;

/** `[json length: u32][json][padding to 4][f32…]`, all little-endian. */
export function decodeDrawing(buf: ArrayBuffer): CadDrawing {
  const n = new DataView(buf).getUint32(0, true);
  const json = JSON.parse(new TextDecoder().decode(new Uint8Array(buf, 4, n))) as Omit<CadDrawing, 'floats'>;
  const at = Math.ceil((4 + n) / 4) * 4;
  return { ...json, floats: new Float32Array(buf, at, (buf.byteLength - at) / 4) };
}

/** Screen = world × scale, y up, then moved by (x, y). */
export interface View {
  scale: number;
  x: number;
  y: number;
}

/** The drawing's bounds in a `w` × `h` area, with a margin. */
export function fit(bounds: CadDrawing['bounds'], w: number, h: number, margin = 24): View {
  if (!bounds) return { scale: 1, x: w / 2, y: h / 2 };
  const [x0, y0, x1, y1] = bounds;
  const bw = Math.max(x1 - x0, 1e-9);
  const bh = Math.max(y1 - y0, 1e-9);
  const scale = Math.min((w - 2 * margin) / bw, (h - 2 * margin) / bh);
  const s = Number.isFinite(scale) && scale > 0 ? scale : 1;
  // y is flipped: screen y = y − world y × scale.
  return { scale: s, x: w / 2 - ((x0 + x1) / 2) * s, y: h / 2 + ((y0 + y1) / 2) * s };
}

/** Zooms by `factor` keeping the screen point (px, py) where it is. */
export function zoomAt(v: View, factor: number, px: number, py: number): View {
  const scale = Math.min(Math.max(v.scale * factor, 1e-12), 1e12);
  const k = scale / v.scale;
  return { scale, x: px - (px - v.x) * k, y: py - (py - v.y) * k };
}

/**
 * A colour as CSS. On a light ground, colours made for CAD's black one (yellow, cyan, light grey)
 * are darkened until they can be read.
 */
export function cssColor(rgb: number, dark: boolean): string {
  if (rgb === FOREGROUND) return dark ? '#ffffff' : '#000000';
  let r = (rgb >> 16) & 255;
  let g = (rgb >> 8) & 255;
  let b = rgb & 255;
  if (!dark) {
    const lum = (0.2126 * r + 0.7152 * g + 0.0722 * b) / 255;
    if (lum > 0.6) {
      const k = 0.6 / lum;
      r = Math.round(r * k);
      g = Math.round(g * k);
      b = Math.round(b * k);
    }
  }
  return `#${((1 << 24) | (r << 16) | (g << 8) | b).toString(16).slice(1)}`;
}

/** Lines (`[n, x1, y1, … xn, yn]` repeated) as separate segments: `[ax, ay, bx, by]` repeated. */
export function segments(f: Float32Array, run: CadRun): Float32Array {
  let count = 0;
  for (let i = run.start; i < run.end; ) {
    const n = f[i];
    count += Math.max(n - 1, 0);
    i += 1 + 2 * n;
  }
  const out = new Float32Array(count * 4);
  let o = 0;
  for (let i = run.start; i < run.end; ) {
    const n = f[i++];
    for (let k = 0; k + 1 < n; k++) {
      const a = i + 2 * k;
      out[o++] = f[a];
      out[o++] = f[a + 1];
      out[o++] = f[a + 2];
      out[o++] = f[a + 3];
    }
    i += 2 * n;
  }
  return out;
}

/** Lines as a path, for a canvas without WebGL. */
export function strokePath(f: Float32Array, run: CadRun): Path2D {
  const p = new Path2D();
  for (let i = run.start; i < run.end; ) {
    const n = f[i++];
    p.moveTo(f[i], f[i + 1]);
    for (let k = 1; k < n; k++) p.lineTo(f[i + 2 * k], f[i + 2 * k + 1]);
    i += 2 * n;
  }
  return p;
}

/** Areas (triangles: `[x1, y1, x2, y2, x3, y3]` repeated) as a path, for a canvas without WebGL. */
export function trianglePath(f: Float32Array, run: CadRun): Path2D {
  const p = new Path2D();
  for (let i = run.start; i + 6 <= run.end; i += 6) {
    p.moveTo(f[i], f[i + 1]);
    p.lineTo(f[i + 2], f[i + 3]);
    p.lineTo(f[i + 4], f[i + 5]);
    p.closePath();
  }
  return p;
}

/** `#rrggbb` → `[r, g, b]` from 0 to 1. */
export function rgbOf(css: string): [number, number, number] {
  const n = parseInt(css.slice(1), 16);
  return [((n >> 16) & 255) / 255, ((n >> 8) & 255) / 255, (n & 255) / 255];
}
