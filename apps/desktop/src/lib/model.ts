// 3D models read in the page: meshes with three.js's loaders, CAD solids (STEP, BREP) with
// Open CASCADE in a worker (occt.ts), which hands them over as glTF. The file comes from look:// (`/f/<token>`); what a model
// loads from beside it (an OBJ's materials and textures, a glTF's buffers) from `/r/<token>/`,
// which serves only those (src-tauri/src/ui/look.rs).

import * as THREE from 'three';

import { readOcct } from './occt';

export type ModelFormat = 'stl' | 'obj' | 'ply' | 'gltf' | 'glb' | '3mf' | 'fbx' | 'dae' | '3ds' | 'amf' | 'step' | 'brep';

const BY_EXT: Record<string, ModelFormat> = {
  stl: 'stl',
  obj: 'obj',
  ply: 'ply',
  gltf: 'gltf',
  glb: 'glb',
  '3mf': '3mf',
  fbx: 'fbx',
  dae: 'dae',
  '3ds': '3ds',
  amf: 'amf',
  step: 'step',
  stp: 'step',
  brep: 'brep',
};

export function formatOf(name: string): ModelFormat | null {
  const ext = name.toLowerCase().split('.').pop() ?? '';
  return BY_EXT[ext] ?? null;
}

/** The name shown for a format. */
export function formatName(f: ModelFormat): string {
  return f === 'gltf' || f === 'glb' ? 'glTF' : f.toUpperCase();
}

/**
 * CAD and printing formats are drawn with Z up (STEP and BREP stay so in the glTF Open CASCADE
 * makes of them); the others with Y up, as glTF is.
 */
export function zUp(f: ModelFormat): boolean {
  return ['stl', 'step', 'brep', '3mf', 'amf', '3ds'].includes(f);
}

/** A STEP model's glTF is in metres: shown in millimetres again. (BREP has no unit; its glTF keeps
 * its numbers.) */
export function unitsOf(f: ModelFormat): { unit: string; scale: number } | null {
  return f === 'step' ? { unit: 'mm', scale: 1000 } : null;
}

/** Solids are read by Open CASCADE (slowly: half a minute for a large assembly). */
export function isSolid(f: ModelFormat): boolean {
  return f === 'step' || f === 'brep';
}

/** Files larger than this are not read (a STEP assembly of 90 MB takes half a minute). */
export const MAX_BYTES: Record<'occt' | 'mesh', number> = { occt: 300 << 20, mesh: 512 << 20 };

/** The surface of what has no material of its own: a light grey-blue, as CAD viewers use. */
export function plainMaterial(color = 0xb4bcc8): THREE.MeshStandardMaterial {
  return new THREE.MeshStandardMaterial({ color, metalness: 0.08, roughness: 0.62, side: THREE.DoubleSide, polygonOffset: true, polygonOffsetFactor: 1, polygonOffsetUnits: 1 });
}

export interface Loaded {
  object: THREE.Object3D;
  /** Settles once what the model loads after itself (textures) is in; failed ones are dropped. */
  textures: Promise<void>;
  /** Parts (meshes), for the status line. */
  parts: number;
  triangles: number;
}

/** `name` (from inside a model) as a path below `/r/<token>/`. */
function resourceUrl(base: string, name: string): string {
  return base + name.replace(/\\/g, '/').split('/').map(encodeURIComponent).join('/');
}

async function fetchBuffer(url: string, signal: AbortSignal): Promise<ArrayBuffer> {
  const r = await fetch(url, { signal });
  if (!r.ok) throw new Error(`HTTP ${r.status}`);
  return r.arrayBuffer();
}

/** An OBJ's material libraries (`mtllib a.mtl b.mtl`), read from beside it; none when missing. */
async function objMaterials(text: string, base: string, manager: THREE.LoadingManager) {
  const { MTLLoader } = await import('three/addons/loaders/MTLLoader.js');
  const names = [...text.matchAll(/^\s*mtllib\s+(.+?)\s*$/gm)].map((m) => m[1]);
  for (const n of names) {
    try {
      const loader = new MTLLoader(manager).setResourcePath(base);
      const m = await loader.loadAsync(resourceUrl(base, n));
      m.preload();
      return m;
    } catch {
      /* the next one, or none */
    }
  }
  return null;
}

/** Solids' glTF materials as CAD's: matte, in the part's colour (plain grey where it has none). */
function cadMaterials(o: THREE.Object3D) {
  const made = new Map<string, THREE.MeshStandardMaterial>();
  o.traverse((c) => {
    const m = c as THREE.Mesh;
    if (!m.isMesh) return;
    const from = (Array.isArray(m.material) ? m.material[0] : m.material) as THREE.MeshStandardMaterial | undefined;
    const color = from?.color;
    const plain = !color || (color.r >= 0.999 && color.g >= 0.999 && color.b >= 0.999) || color.getHex() === 0;
    const key = plain ? 'plain' : color.getHexString();
    let mat = made.get(key);
    if (!mat) {
      mat = plainMaterial(plain ? undefined : color.getHex());
      made.set(key, mat);
    }
    m.material = mat;
  });
}

/**
 * A solid's glTF has a mesh per face (61 000 for a large assembly: as many draws each frame).
 * The faces of each part are merged by material: a few hundred draws.
 */
async function mergeFaces(o: THREE.Object3D) {
  const { mergeGeometries } = await import('three/addons/utils/BufferGeometryUtils.js');
  const parents = new Set<THREE.Object3D>();
  o.traverse((c) => {
    if ((c as THREE.Mesh).isMesh && c.parent) parents.add(c.parent);
  });
  for (const p of parents) {
    const meshes = p.children.filter((c) => (c as THREE.Mesh).isMesh) as THREE.Mesh[];
    if (meshes.length < 2) continue;
    const byMat = new Map<THREE.Material, THREE.Mesh[]>();
    for (const m of meshes) {
      const mat = m.material as THREE.Material;
      byMat.set(mat, [...(byMat.get(mat) ?? []), m]);
    }
    for (const [mat, list] of byMat) {
      if (list.length < 2) continue;
      // Position, normal and triangles only, in the parent's space.
      const geoms = list.map((m) => {
        m.updateMatrix();
        const g = new THREE.BufferGeometry();
        g.setAttribute('position', m.geometry.getAttribute('position'));
        const n = m.geometry.getAttribute('normal');
        if (n) g.setAttribute('normal', n);
        else g.computeVertexNormals();
        g.setIndex(m.geometry.index ?? [...Array(m.geometry.getAttribute('position').count).keys()]);
        return g.applyMatrix4(m.matrix);
      });
      const merged = mergeGeometries(geoms, false);
      if (!merged) continue;
      const one = new THREE.Mesh(merged, mat);
      one.name = p.name;
      for (const m of list) {
        p.remove(m);
        m.geometry.dispose();
      }
      p.add(one);
    }
  }
}

/** Parts of a glTF binary: its nodes that place a mesh. */
export function glbParts(glb: ArrayBuffer): number | null {
  try {
    const v = new DataView(glb);
    const n = v.getUint32(12, true);
    const j = JSON.parse(new TextDecoder().decode(new Uint8Array(glb, 20, n))) as { nodes?: { mesh?: number }[] };
    return (j.nodes ?? []).filter((x) => x.mesh !== undefined).length;
  } catch {
    return null;
  }
}

const MAPS = ['map', 'normalMap', 'roughnessMap', 'metalnessMap', 'emissiveMap', 'aoMap', 'bumpMap', 'specularMap', 'alphaMap', 'lightMap', 'displacementMap'] as const;

/** Textures that did not load (missing beside the model) would draw black: they go. */
function dropMissingMaps(o: THREE.Object3D) {
  o.traverse((c) => {
    const m = c as THREE.Mesh;
    if (!m.isMesh) return;
    for (const mat of Array.isArray(m.material) ? m.material : [m.material]) {
      const slots = mat as unknown as Record<string, THREE.Texture | null | undefined>;
      for (const k of MAPS) {
        const t = slots[k];
        const img = t?.image as { width?: number } | undefined;
        if (t && (!img || !img.width)) {
          slots[k] = null;
          // The glow colour scaled the missing texture: without it, the whole would glow.
          if (k === 'emissiveMap') (mat as THREE.MeshStandardMaterial).emissive?.setHex(0);
          mat.needsUpdate = true;
        }
      }
      // A texture that never loaded leaves no slot at all (glTF): a white glow with nothing to
      // scale is that, not a lamp.
      const std = mat as THREE.MeshStandardMaterial;
      const e = std.emissive;
      if (e && !std.emissiveMap && e.r > 0.9 && e.g > 0.9 && e.b > 0.9) {
        e.setHex(0);
        mat.needsUpdate = true;
      }
    }
  });
}

/** Counts the meshes and triangles under `o`. */
export function measure(o: THREE.Object3D): { parts: number; triangles: number } {
  // A glTF mesh of several primitives (a solid's faces by colour) is a group of meshes named
  // after it, `<name>_0`, `<name>_1`…: one part.
  const parts = new Set<THREE.Object3D>();
  let triangles = 0;
  o.traverse((c) => {
    const m = c as THREE.Mesh;
    if (!m.isMesh) return;
    const p = m.parent;
    parts.add(p && p.name && m.name.startsWith(`${p.name}_`) && /_\d+$/.test(m.name) ? p : m);
    const g = m.geometry;
    triangles += (g.index ? g.index.count : (g.getAttribute('position')?.count ?? 0)) / 3;
  });
  return { parts: parts.size, triangles: Math.round(triangles) };
}

async function parseGltf(data: ArrayBuffer, base: string, manager: THREE.LoadingManager): Promise<THREE.Object3D> {
  const { GLTFLoader } = await import('three/addons/loaders/GLTFLoader.js');
  const { DRACOLoader } = await import('three/addons/loaders/DRACOLoader.js');
  const { MeshoptDecoder } = await import('three/addons/libs/meshopt_decoder.module.js');
  // Its decoder files come bundled (three.js names them by URL).
  const draco = new DRACOLoader();
  const loader = new GLTFLoader(manager).setDRACOLoader(draco).setMeshoptDecoder(MeshoptDecoder);
  try {
    return (await loader.parseAsync(data, base)).scene;
  } finally {
    draco.dispose();
  }
}

/** Where a solid's glTF is kept between previews (the Rust side, by path, size and date). */
export interface SolidCache {
  get(): Promise<ArrayBuffer | null>;
  put(glb: ArrayBuffer): void;
}

export interface LoadOptions {
  /** Where what the model names lies (`/r/<token>/`). */
  base: string;
  /** The Open CASCADE WebAssembly file. */
  wasm: string;
  cache: SolidCache;
  signal: AbortSignal;
  /** A texture came in: draw again. */
  onTexture: () => void;
}

/**
 * Reads the model at `url`. Meshes without materials get the plain one; vertex colours are
 * kept.
 */
export async function loadModel(url: string, format: ModelFormat, opts: LoadOptions): Promise<Loaded> {
  const { base, signal, onTexture } = opts;
  let object: THREE.Object3D;
  /** Parts as the file counts them, where it does. */
  let parts: number | null = null;
  let pending = 0;
  let settle = () => {};
  const textures = new Promise<void>((resolve) => (settle = resolve));
  const manager = new THREE.LoadingManager(
    () => settle(),
    (_url, loaded, total) => {
      pending = total - loaded;
      onTexture();
    },
  );
  manager.onStart = (_url, loaded, total) => (pending = total - loaded);
  switch (format) {
    case 'step':
    case 'brep': {
      let glb = await opts.cache.get().catch(() => null);
      if (!glb) {
        glb = await readOcct(url, format, opts.wasm, signal);
        opts.cache.put(glb.slice(0));
      }
      parts = glbParts(glb);
      object = await parseGltf(glb, base, manager);
      cadMaterials(object);
      await mergeFaces(object);
      break;
    }
    case 'stl': {
      const { STLLoader } = await import('three/addons/loaders/STLLoader.js');
      const g = new STLLoader().parse(await fetchBuffer(url, signal));
      const colored = (g as THREE.BufferGeometry & { hasColors?: boolean }).hasColors;
      const mat = plainMaterial();
      if (colored) mat.vertexColors = true;
      object = new THREE.Mesh(g, mat);
      break;
    }
    case 'ply': {
      const { PLYLoader } = await import('three/addons/loaders/PLYLoader.js');
      const g = new PLYLoader().parse(await fetchBuffer(url, signal));
      const colored = !!g.getAttribute('color');
      if (!g.index && g.getAttribute('position').count % 3 !== 0) {
        // A point cloud.
        object = new THREE.Points(g, new THREE.PointsMaterial({ size: 1.5, sizeAttenuation: false, vertexColors: colored, color: colored ? 0xffffff : 0x8090a0 }));
      } else {
        if (!g.getAttribute('normal')) g.computeVertexNormals();
        const mat = plainMaterial(colored ? 0xffffff : undefined);
        mat.vertexColors = colored;
        object = new THREE.Mesh(g, mat);
      }
      break;
    }
    case 'obj': {
      const { OBJLoader } = await import('three/addons/loaders/OBJLoader.js');
      const text = new TextDecoder().decode(await fetchBuffer(url, signal));
      const loader = new OBJLoader();
      const mtl = await objMaterials(text, base, manager);
      if (mtl) loader.setMaterials(mtl);
      object = loader.parse(text);
      break;
    }
    case 'gltf':
    case 'glb': {
      object = await parseGltf(await fetchBuffer(url, signal), base, manager);
      break;
    }
    case '3mf': {
      const { ThreeMFLoader } = await import('three/addons/loaders/3MFLoader.js');
      object = new ThreeMFLoader().parse(await fetchBuffer(url, signal));
      break;
    }
    case 'fbx': {
      const { FBXLoader } = await import('three/addons/loaders/FBXLoader.js');
      object = new FBXLoader(manager).parse(await fetchBuffer(url, signal), base);
      break;
    }
    case 'dae': {
      const { ColladaLoader } = await import('three/addons/loaders/ColladaLoader.js');
      const text = new TextDecoder().decode(await fetchBuffer(url, signal));
      const c = new ColladaLoader(manager).parse(text, base);
      if (!c) throw new Error('not read');
      object = c.scene;
      break;
    }
    case '3ds': {
      const { TDSLoader } = await import('three/addons/loaders/TDSLoader.js');
      object = new TDSLoader(manager).setResourcePath(base).parse(await fetchBuffer(url, signal), base);
      break;
    }
    case 'amf': {
      const { AMFLoader } = await import('three/addons/loaders/AMFLoader.js');
      object = new AMFLoader().parse(await fetchBuffer(url, signal));
      break;
    }
  }
  // Meshes without a material of their own (some loaders give a default white or none).
  object.traverse((c) => {
    const m = c as THREE.Mesh;
    if (m.isMesh && !m.material) m.material = plainMaterial();
  });
  if (pending === 0) settle();
  const done = textures.then(() => dropMissingMaps(object));
  const m = measure(object);
  return { object, textures: done, triangles: m.triangles, parts: parts ?? m.parts };
}

export { resourceUrl };
