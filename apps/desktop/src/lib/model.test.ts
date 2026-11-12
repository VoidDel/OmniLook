import * as THREE from 'three';
import { describe, expect, it } from 'vitest';

import { formatName, formatOf, glbParts, isSolid, measure, unitsOf, zUp } from './model';

/** A glTF binary with just its JSON chunk. */
function glb(json: object): ArrayBuffer {
  const text = new TextEncoder().encode(JSON.stringify(json));
  const pad = (4 - (text.length % 4)) % 4;
  const buf = new ArrayBuffer(20 + text.length + pad);
  const v = new DataView(buf);
  v.setUint32(0, 0x46546c67, true);
  v.setUint32(4, 2, true);
  v.setUint32(8, buf.byteLength, true);
  v.setUint32(12, text.length + pad, true);
  v.setUint32(16, 0x4e4f534a, true);
  new Uint8Array(buf, 20).set(text);
  new Uint8Array(buf, 20 + text.length).fill(0x20);
  return buf;
}

describe('model', () => {
  it('knows formats by name', () => {
    expect(formatOf('Part.STEP')).toBe('step');
    expect(formatOf('a.stp')).toBe('step');
    expect(formatOf('x.glb')).toBe('glb');
    expect(formatOf('x.igs')).toBeNull();
    expect(formatOf('notes.txt')).toBeNull();
    expect(formatName('gltf')).toBe('glTF');
    expect(formatName('stl')).toBe('STL');
  });

  it('draws CAD formats with Z up, STEP in millimetres', () => {
    expect(zUp('step') && zUp('stl') && zUp('3mf')).toBe(true);
    expect(zUp('gltf') || zUp('obj') || zUp('fbx')).toBe(false);
    expect(unitsOf('step')).toEqual({ unit: 'mm', scale: 1000 });
    expect(unitsOf('stl')).toBeNull();
    expect(isSolid('brep') && !isSolid('glb')).toBe(true);
  });

  it('counts the parts a glTF places', () => {
    expect(glbParts(glb({ nodes: [{ children: [1, 2] }, { mesh: 0 }, { mesh: 0 }] }))).toBe(2);
    expect(glbParts(new ArrayBuffer(8))).toBeNull();
  });

  it('counts a mesh of several primitives as one part', () => {
    const geo = () => new THREE.BufferGeometry().setAttribute('position', new THREE.BufferAttribute(new Float32Array(9), 3));
    const part = new THREE.Group();
    part.name = 'bracket';
    for (const i of [0, 1]) {
      const m = new THREE.Mesh(geo());
      m.name = `bracket_${i}`;
      part.add(m);
    }
    const root = new THREE.Group();
    root.add(part, new THREE.Mesh(geo()));
    expect(measure(root)).toEqual({ parts: 2, triangles: 3 });
  });
});
