// CAD drawings (DWG, DXF): read and drawn into lines, areas and labels in Rust
// (crates/omnilook-core/src/cad.rs). Lines and areas go to the GPU (lib/cadgl.ts), labels to a
// canvas above; without WebGL, a plain canvas draws it all. The wheel zooms around the pointer,
// dragging pans, a double click fits the drawing again.

import { Icon } from 'omni-ui';
import { useCallback, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';

import { api, type Shown } from '../lib/api';
import { type CadDrawing, cssColor, decodeDrawing, fit, strokePath, trianglePath, type View, zoomAt } from '../lib/cad';
import { type GlScene, glScene } from '../lib/cadgl';

/** CAD's black ground, kept for this viewer (browser storage, best effort). */
const BLACK_KEY = 'omnilook.cadBlack';
function loadBlack(): boolean {
  try {
    return localStorage.getItem(BLACK_KEY) === '1';
  } catch {
    return false;
  }
}

/** Labels smaller than this on screen are not drawn. */
const MIN_TEXT_PX = 2.5;
/** CAD text height is the capitals' height; a font's em is about this much taller. */
const CAP = 0.7;

/** Four significant digits: `102100` → `102,100`, `43.2617` → `43.26`. */
function short(n: number): string {
  return Number(n.toPrecision(4)).toLocaleString();
}

/** What draws the lines and areas: the GPU, or a plain canvas. */
type Painter = { gl: GlScene } | { paths: { fills: [number, Path2D][]; strokes: [number, Path2D][] } };

export function CadView({ shown, onMeta }: { shown: Shown; onMeta: (meta: string) => void }) {
  const { t } = useTranslation();
  const [d, setD] = useState<CadDrawing | null>(null);
  const [error, setError] = useState(false);
  const [black, setBlack] = useState(loadBlack);
  const [appearance, setAppearance] = useState(() => document.documentElement.dataset.appearance);
  const box = useRef<HTMLDivElement>(null);
  const under = useRef<HTMLCanvasElement>(null);
  const over = useRef<HTMLCanvasElement>(null);
  const painter = useRef<Painter | null>(null);
  const view = useRef<View | null>(null);
  const frame = useRef(0);

  useEffect(() => {
    let gone = false;
    api.readCad(shown.id).then(
      (buf) => !gone && setD(decodeDrawing(buf)),
      () => !gone && setError(true),
    );
    return () => {
      gone = true;
    };
  }, [shown.id]);

  // The theme can change while the preview is open.
  useEffect(() => {
    const o = new MutationObserver(() => setAppearance(document.documentElement.dataset.appearance));
    o.observe(document.documentElement, { attributes: true, attributeFilter: ['data-appearance'] });
    return () => o.disconnect();
  }, []);

  useEffect(() => {
    if (error) onMeta(t('cad.failed'));
    if (!d) return;
    const parts: string[] = [[d.format, d.version].filter(Boolean).join(' ')];
    parts.push(t('cad.objects', { count: d.objects }));
    if (d.layers) parts.push(t('cad.layers', { count: d.layers }));
    if (d.bounds) {
      const [x0, y0, x1, y1] = d.bounds;
      parts.push(`${short(x1 - x0)} × ${short(y1 - y0)}${d.units ? ` ${d.units}` : ''}`);
    }
    if (d.paper) parts.push(t('cad.paper'));
    if (d.truncated) parts.push(t('cad.truncated'));
    onMeta(parts.join(' · '));
  }, [d, error, onMeta, t]);

  // Lines and areas go to the GPU once (or become paths, without WebGL).
  useEffect(() => {
    const c = under.current;
    if (!d || !c) return;
    const gl = glScene(c, d);
    painter.current = gl
      ? { gl }
      : {
          paths: {
            fills: d.fills.map((r) => [r.color, trianglePath(d.floats, r)]),
            strokes: d.strokes.map((r) => [r.color, strokePath(d.floats, r)]),
          },
        };
    return () => {
      gl?.dispose();
      painter.current = null;
    };
  }, [d]);

  const dark = black || appearance === 'dark';

  const paint = useCallback(() => {
    frame.current = 0;
    const b = box.current;
    const lo = under.current;
    const hi = over.current;
    const p = painter.current;
    if (!b || !lo || !hi || !d || !p) return;
    const dpr = window.devicePixelRatio || 1;
    const w = b.clientWidth;
    const h = b.clientHeight;
    for (const c of [lo, hi]) {
      if (c.width !== Math.round(w * dpr) || c.height !== Math.round(h * dpr)) {
        c.width = Math.round(w * dpr);
        c.height = Math.round(h * dpr);
      }
    }
    view.current ??= fit(d.bounds, w, h);
    const v = view.current;
    const color = (i: number) => cssColor(d.colors[i] ?? 0, dark);
    if ('gl' in p) {
      p.gl.draw(v, w, h, dark, black ? [0, 0, 0] : null);
    } else {
      const ctx = lo.getContext('2d');
      if (!ctx) return;
      ctx.setTransform(1, 0, 0, 1, 0, 0);
      ctx.clearRect(0, 0, lo.width, lo.height);
      ctx.setTransform(dpr * v.scale, 0, 0, -dpr * v.scale, dpr * v.x, dpr * v.y);
      for (const [c, path] of p.paths.fills) {
        ctx.fillStyle = color(c);
        ctx.fill(path);
      }
      ctx.lineWidth = 1 / v.scale;
      for (const [c, path] of p.paths.strokes) {
        ctx.strokeStyle = color(c);
        ctx.stroke(path);
      }
    }
    // Labels: drawn 100 units high, scaled to theirs.
    const ctx = hi.getContext('2d');
    if (!ctx) return;
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.clearRect(0, 0, hi.width, hi.height);
    ctx.font = `${100 / CAP}px ${getComputedStyle(b).fontFamily || 'sans-serif'}`;
    for (const l of d.labels) {
      const up = Math.hypot(l.ya[0], l.ya[1]);
      const px = up * v.scale;
      if (px < MIN_TEXT_PX) continue;
      const sx = v.x + l.x * v.scale;
      const sy = v.y - l.y * v.scale;
      // Off screen, by more than the label could reach.
      const reach = px * (l.text.length + 2) * Math.max(1, Math.hypot(l.xa[0], l.xa[1]) / up);
      if (sx < -reach || sy < -reach || sx > w + reach || sy > h + reach) continue;
      const k = (dpr * v.scale) / 100;
      ctx.setTransform(k * l.xa[0], -k * l.xa[1], -k * l.ya[0], k * l.ya[1], dpr * sx, dpr * sy);
      ctx.fillStyle = color(l.color);
      ctx.textAlign = l.h === 1 ? 'center' : l.h === 2 ? 'right' : 'left';
      ctx.textBaseline = l.v === 1 ? 'bottom' : l.v === 2 ? 'middle' : l.v === 3 ? 'top' : 'alphabetic';
      ctx.fillText(l.text, 0, 0);
    }
  }, [d, black, dark]);

  const redraw = useCallback(() => {
    if (!frame.current) frame.current = requestAnimationFrame(paint);
  }, [paint]);

  useEffect(() => {
    redraw();
  }, [redraw]);

  // Size changes: keep the middle of the drawing in the middle.
  useEffect(() => {
    const b = box.current;
    if (!b) return;
    let size = [b.clientWidth, b.clientHeight];
    const o = new ResizeObserver(() => {
      const v = view.current;
      if (v) view.current = { ...v, x: v.x + (b.clientWidth - size[0]) / 2, y: v.y + (b.clientHeight - size[1]) / 2 };
      size = [b.clientWidth, b.clientHeight];
      redraw();
    });
    o.observe(b);
    return () => o.disconnect();
  }, [redraw]);

  useEffect(() => () => cancelAnimationFrame(frame.current), []);

  const zoom = (factor: number) => {
    const b = box.current;
    if (!view.current || !b) return;
    view.current = zoomAt(view.current, factor, b.clientWidth / 2, b.clientHeight / 2);
    redraw();
  };
  const refit = () => {
    const b = box.current;
    if (!d || !b) return;
    view.current = fit(d.bounds, b.clientWidth, b.clientHeight);
    redraw();
  };

  const drag = useRef<{ x: number; y: number } | null>(null);
  const onPointerDown = (e: React.PointerEvent) => {
    if (e.button !== 0 && e.button !== 1) return;
    e.currentTarget.setPointerCapture(e.pointerId);
    drag.current = { x: e.clientX, y: e.clientY };
  };
  const onPointerMove = (e: React.PointerEvent) => {
    const g = drag.current;
    if (!g || !view.current) return;
    view.current = { ...view.current, x: view.current.x + e.clientX - g.x, y: view.current.y + e.clientY - g.y };
    drag.current = { x: e.clientX, y: e.clientY };
    redraw();
  };
  const onPointerUp = () => {
    drag.current = null;
  };

  // Wheel: a passive listener cannot keep the page from scrolling, so it is added by hand.
  useEffect(() => {
    const b = box.current;
    if (!b) return;
    const onWheel = (e: WheelEvent) => {
      e.preventDefault();
      if (!view.current) return;
      const r = b.getBoundingClientRect();
      const step = e.deltaMode === 1 ? e.deltaY * 33 : e.deltaY;
      view.current = zoomAt(view.current, Math.exp(-step * 0.0015), e.clientX - r.left, e.clientY - r.top);
      redraw();
    };
    b.addEventListener('wheel', onWheel, { passive: false });
    return () => b.removeEventListener('wheel', onWheel);
  }, [redraw]);

  const toggleBlack = () => {
    setBlack((on) => {
      try {
        localStorage.setItem(BLACK_KEY, on ? '0' : '1');
      } catch {
        /* not kept */
      }
      return !on;
    });
  };

  if (error) return <div className="look-note">{t('cad.failed')}</div>;
  return (
    <div
      ref={box}
      className={`look-cad${black ? ' black' : ''}`}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onPointerCancel={onPointerUp}
      onDoubleClick={refit}
    >
      <canvas ref={under} />
      <canvas ref={over} />
      {!d && <div className="look-note look-cad-wait">{t('view.loading')}</div>}
      {d && !d.bounds && <div className="look-note look-cad-wait">{t('cad.empty')}</div>}
      {d && (
        <div className="look-cad-tools" onPointerDown={(e) => e.stopPropagation()} onDoubleClick={(e) => e.stopPropagation()}>
          <button type="button" className="btn ghost" onClick={() => zoom(1.5)} title={t('cad.zoomIn')} aria-label={t('cad.zoomIn')}>
            <Icon name="plus" />
          </button>
          <button type="button" className="btn ghost" onClick={() => zoom(1 / 1.5)} title={t('cad.zoomOut')} aria-label={t('cad.zoomOut')}>
            <Icon name="minus" />
          </button>
          <button type="button" className="btn ghost" onClick={refit} title={t('cad.fit')} aria-label={t('cad.fit')}>
            <Icon name="square" />
          </button>
          <button
            type="button"
            className={`btn ghost${black ? ' on' : ''}`}
            onClick={toggleBlack}
            title={t('cad.black')}
            aria-label={t('cad.black')}
            aria-pressed={black}
          >
            <Icon name="moon" />
          </button>
        </div>
      )}
    </div>
  );
}
