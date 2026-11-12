// A model's parts as a tree (assemblies, sub-assemblies, parts), from its three.js objects: what
// the part list shows. Groups without a name of their own and a single child are folded away;
// the meshes a glTF makes of one part's faces (`<part>_0`, `<part>_1`…, or merged into one named
// as the part) are that part, not parts of their own.

import type * as THREE from 'three';

export interface PartNode {
  /** Unique within the tree, in the order of the rows. */
  id: number;
  name: string;
  object: THREE.Object3D;
  children: PartNode[];
  /** Parts (leaves) under it, itself for a leaf. */
  parts: number;
}

const isMesh = (o: THREE.Object3D) => (o as THREE.Mesh).isMesh === true || (o as THREE.Points).isPoints === true;

/** A mesh that is one of its parent's faces, not a part. */
function isFaceOf(m: THREE.Object3D, parent: THREE.Object3D): boolean {
  return !!parent.name && (m.name === parent.name || (m.name.startsWith(`${parent.name}_`) && /_\d+$/.test(m.name)));
}

/** Whether anything is drawn under `o`. */
function drawsSomething(o: THREE.Object3D): boolean {
  let found = false;
  o.traverse((c) => {
    if (!found && isMesh(c)) found = true;
  });
  return found;
}

/**
 * The tree under `root`, or null when there is a single part (no list is worth showing). The top
 * is called `rootName` (the file's) when the model does not name it; `unnamed(n)` names the
 * n-th other thing without a name.
 */
export function partTree(root: THREE.Object3D, rootName: string, unnamed: (n: number) => string): PartNode | null {
  let next = 0;
  let nameless = 0;

  /** The nodes `o` stands for: one, or its children's when it is folded away. `top`: nothing
   * kept above it. */
  const build = (o: THREE.Object3D, top = false): PartNode[] => {
    if (!drawsSomething(o)) return [];
    if (isMesh(o)) return [{ id: next++, name: o.name || unnamed(++nameless), object: o, children: [], parts: 1 }];
    const kids = o.children.filter((c) => drawsSomething(c));
    // All its meshes are its own faces: a part.
    if (o.name && kids.length && kids.every((c) => isMesh(c) && isFaceOf(c, o))) {
      return [{ id: next++, name: o.name, object: o, children: [], parts: 1 }];
    }
    if (!o.name && kids.length === 1) return build(kids[0], top);
    // Numbered before its children, as the rows read.
    const id = next++;
    const name = o.name || (top ? rootName : unnamed(++nameless));
    const children = kids.flatMap((c) => (isMesh(c) && isFaceOf(c, o) ? [] : build(c)));
    // Only one child: it stands for the group; a placement of a single part is that part.
    if (children.length === 1 && (!o.name || children[0].name === o.name)) return children;
    if (children.length === 1 && !children[0].children.length) return [{ id, name, object: o, children: [], parts: 1 }];
    if (!children.length) return [];
    return [{ id, name, object: o, children, parts: children.reduce((s, c) => s + c.parts, 0) }];
  };

  const top = build(root, true);
  const tree: PartNode =
    top.length === 1 ? top[0] : { id: next++, name: root.name || rootName, object: root, children: top, parts: top.reduce((s, c) => s + c.parts, 0) };
  return tree.parts > 1 ? tree : null;
}

/** The rows shown: `node` and, below the open ones, their children, with their depth. */
export function visibleRows(node: PartNode, open: Set<number>, depth = 0, out: { node: PartNode; depth: number }[] = []) {
  out.push({ node, depth });
  if (open.has(node.id)) for (const c of node.children) visibleRows(c, open, depth + 1, out);
  return out;
}

/** The ids of the nodes open at first: the top two levels. */
export function initiallyOpen(node: PartNode, levels = 2): Set<number> {
  const open = new Set<number>();
  const walk = (n: PartNode, d: number) => {
    if (d >= levels || !n.children.length) return;
    open.add(n.id);
    for (const c of n.children) walk(c, d + 1);
  };
  walk(node, 0);
  return open;
}
