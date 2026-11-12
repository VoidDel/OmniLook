// STEP and BREP models read with Open CASCADE (occt-wasm: OCCT 8 in WebAssembly, LGPL-2.1) off
// the page's thread, and handed back as a glTF binary (GLB) with the assembly's parts, names and
// colours: the page draws it as it draws any glTF. A large assembly takes half a minute.

import { type LabelTag, OcctKernel } from 'occt-wasm';

import { type OcctRequest, type OcctResponse, stepName } from './occt';

/** How finely surfaces are cut into triangles, as a share of the model's size. */
const DEFLECTION = 0.0004;

self.onmessage = async (e: MessageEvent<OcctRequest>) => {
  const { url, format, wasm } = e.data;
  try {
    const [kernel, file] = await Promise.all([
      OcctKernel.init({ wasm }),
      fetch(url).then((r) => {
        if (!r.ok) throw new Error(`HTTP ${r.status}`);
        return r.arrayBuffer();
      }),
    ]);
    // Both formats are text; STEP names are ASCII (non-ASCII written as \X2\ escapes).
    const text = new TextDecoder('latin1').decode(file);
    const doc = format === 'step' ? kernel.importXCAFFromSTEP(text) : kernel.createXCAFDocument();
    if (format === 'brep') doc.addShape(kernel.fromBREP(text));
    // Placed parts are named by their placement, which STEP exporters often leave as
    // `NAUO12` (the occurrence's entity): such names give way to the part's own.
    // Names with characters beyond ASCII come escaped (`\X2\8033673A\X0\`, or with the
    // backslashes turned to `_`): written out.
    const seen = new Set<LabelTag>();
    const tidy = (label: LabelTag, name: string) => {
      const plain = stepName(name);
      if (plain !== name) doc.setName(label, plain);
      return plain;
    };
    const rename = (label: LabelTag) => {
      if (seen.has(label)) return;
      seen.add(label);
      tidy(label, doc.getLabelInfo(label).name);
      for (const child of doc.getChildren(label)) {
        const info = doc.getLabelInfo(child);
        const target = info.isComponent ? doc.getReferredLabel(child) : null;
        if (target === null) continue;
        const own = tidy(target, doc.getLabelInfo(target).name);
        if (own && (!info.name || /^NAUO\d+$/i.test(info.name))) doc.setName(child, own);
        else tidy(child, info.name);
        rename(target);
      }
    };
    for (const root of doc.getRoots()) rename(root);
    // The size of the whole: the first root's shape bounds the rest.
    let size = 0;
    for (const root of doc.getRoots()) {
      const shape = doc.getLabelInfo(root).shapeHandle;
      if (!shape) continue;
      try {
        // Loose (from control hulls): a quick measure, which is all the deflection needs.
        const b = kernel.getBoundingBox(shape, { precise: false });
        size = Math.max(size, Math.hypot(b.xmax - b.xmin, b.ymax - b.ymin, b.zmax - b.zmin));
      } catch {
        /* an empty root */
      }
    }
    const glb = doc.exportGLTF({ linearDeflection: size > 0 ? size * DEFLECTION : 0.1, angularDeflection: 0.5 });
    doc.close();
    const bytes = glb.slice().buffer;
    const out: OcctResponse = { ok: true, glb: bytes };
    (self as unknown as Worker).postMessage(out, [bytes]);
  } catch (err) {
    const out: OcctResponse = { ok: false, error: err instanceof Error ? err.message : String(err) };
    (self as unknown as Worker).postMessage(out);
  }
};
