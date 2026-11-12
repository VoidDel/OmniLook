import { describe, expect, it } from 'vitest';

import { cssColor, decodeDrawing, fit, FOREGROUND, rgbOf, segments, zoomAt } from './cad';

/** What crates/omnilook-core/src/cad.rs `Drawing::encode` writes. */
function encode(json: object, floats: number[]): ArrayBuffer {
  const text = new TextEncoder().encode(JSON.stringify(json));
  const at = Math.ceil((4 + text.length) / 4) * 4;
  const buf = new ArrayBuffer(at + floats.length * 4);
  new DataView(buf).setUint32(0, text.length, true);
  new Uint8Array(buf, 4).set(text);
  new Float32Array(buf, at).set(floats);
  return buf;
}

describe('cad', () => {
  it('reads what Rust sends', () => {
    const d = decodeDrawing(encode({ format: 'DXF', colors: [FOREGROUND] }, [2, 0, 0, 1, 1]));
    expect(d.format).toBe('DXF');
    expect(d.colors).toEqual([FOREGROUND]);
    expect([...d.floats]).toEqual([2, 0, 0, 1, 1]);
  });

  it('cuts lines into segments', () => {
    // A three-point line and a two-point one.
    const f = new Float32Array([3, 0, 0, 1, 0, 1, 1, 2, 5, 5, 6, 6]);
    expect([...segments(f, { color: 0, alpha: 255, start: 0, end: f.length })]).toEqual([0, 0, 1, 0, 1, 0, 1, 1, 5, 5, 6, 6]);
  });

  it('fits and zooms', () => {
    // 100 × 50 in a 240 × 240 area with 20 px margins: 2 px per unit, centred, y up.
    const v = fit([0, 0, 100, 50], 240, 240, 20);
    expect(v.scale).toBe(2);
    expect(v.x).toBe(20);
    expect(v.y).toBe(170);
    // The point under the pointer stays put.
    const z = zoomAt(v, 2, 70, 120);
    expect(z.scale).toBe(4);
    expect((70 - z.x) / z.scale).toBe((70 - v.x) / v.scale);
    expect(fit(null, 100, 80)).toEqual({ scale: 1, x: 50, y: 40 });
  });

  it('colours for the ground', () => {
    expect(cssColor(FOREGROUND, true)).toBe('#ffffff');
    expect(cssColor(FOREGROUND, false)).toBe('#000000');
    expect(cssColor(0xff0000, false)).toBe('#ff0000');
    // Yellow is darkened on a light ground, kept on a dark one.
    expect(cssColor(0xffff00, true)).toBe('#ffff00');
    const [r, g, b] = rgbOf(cssColor(0xffff00, false));
    expect(0.2126 * r + 0.7152 * g + 0.0722 * b).toBeLessThan(0.61);
    expect(b).toBe(0);
  });
});
