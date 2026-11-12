import * as THREE from 'three';
import { describe, expect, it } from 'vitest';

import { initiallyOpen, partTree, visibleRows } from './parts';

const mesh = (name = '') => {
  const m = new THREE.Mesh(new THREE.BufferGeometry());
  m.name = name;
  return m;
};
const group = (name: string, ...kids: THREE.Object3D[]) => {
  const g = new THREE.Group();
  g.name = name;
  g.add(...kids);
  return g;
};
const names = (n: { name: string; children: { name: string }[] }) => [n.name, n.children.map((c) => c.name)];

describe('parts', () => {
  it('builds an assembly tree, a part from its faces', () => {
    // An assembly of a sub-assembly (two parts) and a part of two faces.
    const scene = group(
      '',
      group(
        'housing',
        group('rod-assembly', group('rod', mesh('rod')), group('nut', mesh('nut_0'), mesh('nut_1'))),
        group('base', mesh('base_0'), mesh('base_1')),
      ),
    );
    const t = partTree(scene, 'file', (n) => `#${n}`)!;
    expect(names(t)).toEqual(['housing', ['rod-assembly', 'base']]);
    expect(t.parts).toBe(3);
    expect(names(t.children[0])).toEqual(['rod-assembly', ['rod', 'nut']]);
    expect(t.children[1].children).toEqual([]);
  });

  it('names unnamed parts, folds unnamed groups', () => {
    const scene = group('', group('', mesh(), mesh('b')));
    const t = partTree(scene, 'model.step', (n) => `part ${n}`)!;
    expect(names(t)).toEqual(['model.step', ['part 1', 'b']]);
  });

  it('shows the placement of a single part as that part', () => {
    const scene = group('board', group('U4~SOIC-8', group('SOIC-8', mesh('SOIC-8'))), group('R1', mesh('R1')));
    const t = partTree(scene, 'f', String)!;
    expect(names(t)).toEqual(['board', ['U4~SOIC-8', 'R1']]);
    expect(t.children[0].children).toEqual([]);
  });

  it('has no tree for a single part', () => {
    expect(partTree(group('', mesh('only')), 'f', String)).toBeNull();
    expect(partTree(group('p', mesh('p_0'), mesh('p_1')), 'f', String)).toBeNull();
  });

  it('lists the open rows', () => {
    const scene = group('a', group('b', mesh('c'), mesh('d')), mesh('e'));
    const t = partTree(scene, 'f', String)!;
    const open = initiallyOpen(t, 1);
    expect(visibleRows(t, open).map((r) => [r.node.name, r.depth])).toEqual([
      ['a', 0],
      ['b', 1],
      ['e', 1],
    ]);
    open.add(t.children[0].id);
    expect(visibleRows(t, open)).toHaveLength(5);
  });
});
