// 3D models (lib/model.ts): drawn with three.js. Dragging turns the model, the right button (or
// Shift) pans, the wheel zooms; a double click fits it again. Buttons for the standard views, the
// edges and a black ground. Loaded on demand: other previews do not carry three.js.

import { Icon } from 'omni-ui';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import * as THREE from 'three';
import { OrbitControls } from 'three/addons/controls/OrbitControls.js';

import { api, type Shown } from '../lib/api';
import { formatName, formatOf, isSolid, loadModel, type Loaded, MAX_BYTES, type ModelFormat, unitsOf, zUp } from '../lib/model';
import { type PartNode, partTree } from '../lib/parts';
import { PartTree } from './PartTree';

/** Whether the part list is shown (browser storage, best effort; shown unless closed). */
const TREE_KEY = 'omnilook.partTree';
function loadTree(): boolean {
  try {
    return localStorage.getItem(TREE_KEY) !== '0';
  } catch {
    return true;
  }
}

/** The colour a picked part is drawn in. */
const PICKED = new THREE.MeshStandardMaterial({
  color: 0x4f8cff,
  emissive: 0x1d4ed8,
  emissiveIntensity: 0.35,
  metalness: 0.05,
  roughness: 0.55,
  side: THREE.DoubleSide,
  polygonOffset: true,
  polygonOffsetFactor: 1,
  polygonOffsetUnits: 1,
});

/** The black ground, shared with CAD drawings (browser storage, best effort). */
const BLACK_KEY = 'omnilook.cadBlack';
function loadBlack(): boolean {
  try {
    return localStorage.getItem(BLACK_KEY) === '1';
  } catch {
    return false;
  }
}

/** Edges are drawn for models up to this many triangles (finding them takes a while). */
const EDGE_LIMIT = 1_500_000;
/** Faces meeting at more than this angle (degrees) have an edge drawn between them. */
const EDGE_ANGLE = 30;

type ViewName = 'iso' | 'front' | 'top' | 'right';

/** Shows or hides each of `objects` (three.js objects, changed in place). */
function showAll(objects: THREE.Object3D[] | null, on: boolean) {
  for (const o of objects ?? []) o.visible = on;
}

/** Four significant digits: `102100` → `102,100`. */
function short(n: number): string {
  return Number(n.toPrecision(4)).toLocaleString();
}

interface Scene {
  renderer: THREE.WebGLRenderer;
  scene: THREE.Scene;
  camera: THREE.PerspectiveCamera;
  controls: OrbitControls;
  model: THREE.Object3D;
  /** Each mesh's outline, a child of it (hidden with its part). */
  edges: THREE.LineSegments[] | null;
  box: THREE.Box3;
  up: THREE.Vector3;
}

export default function ModelView({ shown, onMeta }: { shown: Shown; onMeta: (meta: string) => void }) {
  const { t } = useTranslation();
  const format = formatOf(shown.name) as ModelFormat;
  const box = useRef<HTMLDivElement>(null);
  const canvas = useRef<HTMLCanvasElement>(null);
  const scene = useRef<Scene | null>(null);
  const [loaded, setLoaded] = useState<Loaded | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [elapsed, setElapsed] = useState(0);
  const [black, setBlack] = useState(loadBlack);
  const [edges, setEdges] = useState(true);
  const [appearance, setAppearance] = useState(() => document.documentElement.dataset.appearance);
  const [showTree, setShowTree] = useState(loadTree);
  const [hidden, setHidden] = useState<Set<number>>(() => new Set());
  const [picked, setPicked] = useState<PartNode | null>(null);
  const frame = useRef(0);
  const tree = useMemo(() => (loaded ? partTree(loaded.object, shown.name, (n) => t('model.unnamed', { n })) : null), [loaded, shown.name, t]);

  const render = useCallback(() => {
    frame.current = 0;
    const s = scene.current;
    if (s) s.renderer.render(s.scene, s.camera);
  }, []);
  const redraw = useCallback(() => {
    if (!frame.current) frame.current = requestAnimationFrame(render);
  }, [render]);

  const tooLarge = shown.size > (isSolid(format) ? MAX_BYTES.occt : MAX_BYTES.mesh);

  // Read the model.
  useEffect(() => {
    if (tooLarge) return;
    const abort = new AbortController();
    const started = performance.now();
    const tick = window.setInterval(() => setElapsed(Math.floor((performance.now() - started) / 1000)), 1000);
    const base = `${shown.url.replace('/f/', '/r/')}/`;
    const id = shown.id;
    loadModel(shown.url, format, {
      base,
      // Open CASCADE's WebAssembly, served from beside the program (look://, `/w/`).
      wasm: shown.url.replace(/\/f\/.*$/, '/w/occt-wasm.wasm'),
      cache: {
        get: async () => {
          const b = await api.solidCacheGet(id);
          return b.byteLength ? b : null;
        },
        put: (glb) => void api.solidCachePut(id, glb).catch(() => undefined),
      },
      signal: abort.signal,
      onTexture: () => redraw(),
    }).then(
      (l) => {
        window.clearInterval(tick);
        if (!abort.signal.aborted) setLoaded(l);
      },
      (e: unknown) => {
        window.clearInterval(tick);
        if (!abort.signal.aborted) setError(e instanceof Error ? e.message : String(e));
      },
    );
    return () => {
      window.clearInterval(tick);
      abort.abort();
    };
  }, [shown.url, shown.id, format, tooLarge, redraw]);

  // The theme can change while the preview is open.
  useEffect(() => {
    const o = new MutationObserver(() => setAppearance(document.documentElement.dataset.appearance));
    o.observe(document.documentElement, { attributes: true, attributeFilter: ['data-appearance'] });
    return () => o.disconnect();
  }, []);

  useEffect(() => {
    if (error || tooLarge) return onMeta(t(tooLarge ? 'model.tooLarge' : 'model.failed'));
    if (!loaded) return;
    const s = new THREE.Vector3();
    new THREE.Box3().setFromObject(loaded.object).getSize(s);
    const units = unitsOf(format);
    if (units) s.multiplyScalar(units.scale);
    const parts = [
      formatName(format),
      t('model.parts', { count: loaded.parts }),
      t('model.triangles', { count: loaded.triangles }),
      `${short(s.x)} × ${short(s.y)} × ${short(s.z)}${units?.unit ? ` ${units.unit}` : ''}`,
    ];
    onMeta(parts.join(' · '));
  }, [loaded, error, tooLarge, format, onMeta, t]);

  /** Brings `target` (a part, by default the whole model) into view, from the same side. */
  const focus = useCallback(
    (target: THREE.Object3D) => {
      const s = scene.current;
      const b = box.current;
      if (!s || !b) return;
      const area = new THREE.Box3().setFromObject(target, true);
      if (area.isEmpty()) return;
      const center = area.getCenter(new THREE.Vector3());
      const radius = Math.max(area.getSize(new THREE.Vector3()).length() / 2, 1e-6);
      const fov = (s.camera.fov * Math.PI) / 180;
      const aspect = b.clientWidth / Math.max(b.clientHeight, 1);
      const dist = (radius / Math.sin(Math.min(fov, fov * aspect) / 2)) * 1.05;
      const dir = s.camera.position.clone().sub(s.controls.target).normalize();
      s.camera.position.copy(center).addScaledVector(dir, dist);
      s.controls.target.copy(center);
      s.controls.update();
      redraw();
    },
    [redraw],
  );

  /** Looks at the whole model from `view`. */
  const look = useCallback(
    (view: ViewName) => {
      const s = scene.current;
      const b = box.current;
      if (!s || !b) return;
      const center = s.box.getCenter(new THREE.Vector3());
      const radius = Math.max(s.box.getSize(new THREE.Vector3()).length() / 2, 1e-6);
      const z = zUp(format);
      // Directions in a Z-up world; Y-up models swap Y and Z.
      const dirs: Record<ViewName, [number, number, number]> = {
        iso: [1, -1, 0.8],
        front: [0, -1, 0],
        top: [0, 0, 1],
        right: [1, 0, 0],
      };
      const [x, y, w] = dirs[view];
      const dir = (z ? new THREE.Vector3(x, y, w) : new THREE.Vector3(x, w, -y)).normalize();
      const fov = (s.camera.fov * Math.PI) / 180;
      const aspect = b.clientWidth / Math.max(b.clientHeight, 1);
      const dist = (radius / Math.sin(Math.min(fov, fov * aspect) / 2)) * 1.05;
      s.camera.up.copy(view === 'top' ? (z ? new THREE.Vector3(0, 1, 0) : new THREE.Vector3(0, 0, -1)) : s.up);
      s.camera.position.copy(center).addScaledVector(dir, dist);
      s.camera.near = Math.max(dist / 1000, radius / 10000);
      s.camera.far = dist + radius * 100;
      s.camera.updateProjectionMatrix();
      s.controls.target.copy(center);
      s.controls.update();
      redraw();
    },
    [format, redraw],
  );

  // The scene, once the model is read.
  useEffect(() => {
    const c = canvas.current;
    const b = box.current;
    if (!loaded || !c || !b) return;
    const renderer = new THREE.WebGLRenderer({ canvas: c, antialias: true, alpha: true });
    renderer.setPixelRatio(window.devicePixelRatio || 1);
    renderer.setSize(b.clientWidth, b.clientHeight, false);
    const s = new THREE.Scene();
    const up = zUp(format) ? new THREE.Vector3(0, 0, 1) : new THREE.Vector3(0, 1, 0);
    const camera = new THREE.PerspectiveCamera(35, b.clientWidth / Math.max(b.clientHeight, 1), 0.1, 1000);
    camera.up.copy(up);
    // Light from the sky and the ground, and a lamp at the camera: every side shows its shape.
    const hemi = new THREE.HemisphereLight(0xffffff, 0x8a8f99, 1.6);
    hemi.position.copy(up);
    s.add(hemi);
    const lamp = new THREE.DirectionalLight(0xffffff, 1.4);
    lamp.position.set(0.3, 0.6, 1);
    camera.add(lamp);
    s.add(camera);
    s.add(loaded.object);
    const controls = new OrbitControls(camera, c);
    controls.enableDamping = false;
    controls.screenSpacePanning = true;
    controls.zoomToCursor = true;
    controls.addEventListener('change', redraw);
    scene.current = { renderer, scene: s, camera, controls, model: loaded.object, edges: null, box: new THREE.Box3().setFromObject(loaded.object), up };
    look('iso');
    // Textures come after the model; missing ones are dropped then.
    void loaded.textures.then(redraw);
    return () => {
      controls.dispose();
      renderer.dispose();
      s.traverse((o) => {
        const m = o as THREE.Mesh;
        m.geometry?.dispose();
        const mats = Array.isArray(m.material) ? m.material : m.material ? [m.material] : [];
        for (const x of mats) x.dispose();
      });
      scene.current = null;
    };
  }, [loaded, format, look, redraw]);

  // Edges between faces at an angle: CAD's outline. Each mesh's are its child, so a hidden part
  // hides them too.
  useEffect(() => {
    const s = scene.current;
    if (!s || !loaded) return;
    if (edges && !s.edges && loaded.triangles <= EDGE_LIMIT) {
      const mat = new THREE.LineBasicMaterial({ color: 0x1f2329, transparent: true, opacity: 0.55 });
      const meshes: THREE.Mesh[] = [];
      s.model.traverse((o) => {
        if ((o as THREE.Mesh).isMesh) meshes.push(o as THREE.Mesh);
      });
      s.edges = meshes.map((m) => {
        const lines = new THREE.LineSegments(new THREE.EdgesGeometry(m.geometry, EDGE_ANGLE), mat);
        lines.raycast = () => {};
        m.add(lines);
        return lines;
      });
    }
    showAll(s.edges, edges);
    redraw();
  }, [edges, loaded, redraw]);

  // Hidden parts.
  useEffect(() => {
    if (!tree) return;
    const walk = (n: PartNode) => {
      n.object.visible = !hidden.has(n.id);
      for (const c of n.children) walk(c);
    };
    walk(tree);
    redraw();
  }, [tree, hidden, redraw]);

  // The picked part, in the pick colour (its own materials kept to come back to).
  useEffect(() => {
    if (!picked) return;
    const kept = new Map<THREE.Mesh, THREE.Material | THREE.Material[]>();
    picked.object.traverse((o) => {
      const m = o as THREE.Mesh;
      if (!m.isMesh) return;
      kept.set(m, m.material);
      m.material = PICKED;
    });
    redraw();
    return () => {
      for (const [m, mat] of kept) m.material = mat;
      redraw();
    };
  }, [picked, redraw]);

  // Ground and edge colour follow the theme (or the black ground).
  useEffect(() => {
    const s = scene.current;
    if (!s) return;
    const dark = black || appearance === 'dark';
    s.renderer.setClearColor(0x000000, black ? 1 : 0);
    // One material for all the outlines.
    const first = s.edges?.[0];
    if (first) (first.material as THREE.LineBasicMaterial).color.set(dark ? 0xd0d6de : 0x1f2329);
    redraw();
  }, [black, appearance, loaded, edges, redraw]);

  // Size changes.
  useEffect(() => {
    const b = box.current;
    if (!b) return;
    const o = new ResizeObserver(() => {
      const s = scene.current;
      if (!s) return;
      s.renderer.setSize(b.clientWidth, b.clientHeight, false);
      s.camera.aspect = b.clientWidth / Math.max(b.clientHeight, 1);
      s.camera.updateProjectionMatrix();
      redraw();
    });
    o.observe(b);
    return () => o.disconnect();
  }, [redraw]);

  useEffect(() => () => cancelAnimationFrame(frame.current), []);

  // A click (not a drag) on the model picks the part under the pointer; on nothing, none.
  const nodeOf = useMemo(() => {
    const map = new Map<THREE.Object3D, PartNode>();
    const walk = (n: PartNode) => {
      map.set(n.object, n);
      for (const c of n.children) walk(c);
    };
    if (tree) walk(tree);
    return map;
  }, [tree]);
  const down = useRef<{ x: number; y: number } | null>(null);
  const onPointerDown = (e: React.PointerEvent) => {
    down.current = e.button === 0 ? { x: e.clientX, y: e.clientY } : null;
  };
  const onPointerUp = (e: React.PointerEvent) => {
    const d = down.current;
    down.current = null;
    const s = scene.current;
    const b = box.current;
    if (!d || !s || !b || !tree || Math.hypot(e.clientX - d.x, e.clientY - d.y) > 4) return;
    const r = b.getBoundingClientRect();
    const ray = new THREE.Raycaster();
    ray.setFromCamera(new THREE.Vector2(((e.clientX - r.left) / r.width) * 2 - 1, -((e.clientY - r.top) / r.height) * 2 + 1), s.camera);
    const hit = ray.intersectObject(s.model, true).find((h) => {
      // Only what is shown.
      for (let o: THREE.Object3D | null = h.object; o; o = o.parent) if (!o.visible) return false;
      return true;
    });
    // The smallest part holding what was hit.
    let found: PartNode | null = null;
    for (let o: THREE.Object3D | null = hit?.object ?? null; o && !found; o = o.parent) found = nodeOf.get(o) ?? null;
    setPicked(found && found.id !== tree.id ? found : null);
  };

  const toggleTree = () => {
    setShowTree((on) => {
      try {
        localStorage.setItem(TREE_KEY, on ? '0' : '1');
      } catch {
        /* not kept */
      }
      return !on;
    });
  };

  const toggleHidden = (n: PartNode) =>
    setHidden((h) => {
      const next = new Set(h);
      if (next.has(n.id)) next.delete(n.id);
      else next.add(n.id);
      return next;
    });

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

  if (error || tooLarge) {
    return (
      <div className="look-note look-model-error">
        <span>{t(tooLarge ? 'model.tooLarge' : 'model.failed')}</span>
        {error && <span className="look-fallback-note">{error}</span>}
      </div>
    );
  }
  const slow = isSolid(format);
  return (
    <div className="look-model-wrap">
      {tree && showTree && (
        <PartTree
          tree={tree}
          hidden={hidden}
          selected={picked?.id ?? null}
          onToggle={toggleHidden}
          onShowAll={() => setHidden(new Set())}
          onSelect={setPicked}
          onFocus={(n) => {
            setPicked(n);
            focus(n.object);
          }}
        />
      )}
      <div
        ref={box}
        className={`look-cad look-model${black ? ' black' : ''}`}
        onDoubleClick={() => look('iso')}
        onPointerDown={onPointerDown}
        onPointerUp={onPointerUp}
      >
      <canvas ref={canvas} />
      {!loaded && (
        <div className="look-note look-cad-wait">
          {slow ? t('model.readingSolid', { seconds: elapsed }) : t('view.loading')}
        </div>
      )}
      {loaded && (
        <div className="look-cad-tools" onPointerDown={(e) => e.stopPropagation()} onDoubleClick={(e) => e.stopPropagation()}>
          {tree && (
            <button
              type="button"
              className={`btn ghost${showTree ? ' on' : ''}`}
              onClick={toggleTree}
              title={t('model.partListToggle')}
              aria-label={t('model.partListToggle')}
              aria-pressed={showTree}
            >
              <Icon name="list" />
            </button>
          )}
          {(['iso', 'front', 'top', 'right'] as ViewName[]).map((v) => (
            <button key={v} type="button" className="btn ghost text" onClick={() => look(v)} title={t(`model.view.${v}`)}>
              {t(`model.viewShort.${v}`)}
            </button>
          ))}
          <button
            type="button"
            className={`btn ghost${edges ? ' on' : ''}`}
            onClick={() => setEdges((e) => !e)}
            title={loaded.triangles > EDGE_LIMIT ? t('model.edgesTooMany') : t('model.edges')}
            aria-label={t('model.edges')}
            aria-pressed={edges}
            disabled={loaded.triangles > EDGE_LIMIT}
          >
            <Icon name="grid" />
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
    </div>
  );
}
