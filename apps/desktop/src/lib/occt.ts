// What the Open CASCADE worker (occt.worker.ts) is asked and answers, and the asking.

/** A STEP name with its `\X2\…\X0\` (UTF-16, four hex digits a unit) and `\X\hh` escapes
 * written out; also when the backslashes have become `_`. */
export function stepName(s: string): string {
  return s
    .replace(/[\\_]X2[\\_]((?:[0-9A-Fa-f]{4})+)[\\_]X0[\\_]/g, (_, hex: string) =>
      String.fromCharCode(...(hex.match(/.{4}/g) ?? []).map((h) => parseInt(h, 16))),
    )
    .replace(/\\X\\([0-9A-Fa-f]{2})/g, (_, h: string) => String.fromCharCode(parseInt(h, 16)));
}

export type OcctFormat = 'step' | 'brep';

export interface OcctRequest {
  url: string;
  format: OcctFormat;
  /** Where the WebAssembly file is: installed beside the program (look:// `/w/`), replaceable. */
  wasm: string;
}

export type OcctResponse = { ok: true; glb: ArrayBuffer } | { ok: false; error: string };

/** Reads `url` in a worker of its own into a GLB, the worker stopped when done or on `signal`. */
export function readOcct(url: string, format: OcctFormat, wasm: string, signal: AbortSignal): Promise<ArrayBuffer> {
  return new Promise((resolve, reject) => {
    const w = new Worker(new URL('./occt.worker.ts', import.meta.url), { type: 'module' });
    const stop = () => {
      w.terminate();
      reject(new DOMException('aborted', 'AbortError'));
    };
    signal.addEventListener('abort', stop, { once: true });
    const done = () => {
      signal.removeEventListener('abort', stop);
      w.terminate();
    };
    w.onmessage = (e: MessageEvent<OcctResponse>) => {
      done();
      if (e.data.ok) resolve(e.data.glb);
      else reject(new Error(e.data.error));
    };
    w.onerror = (e) => {
      done();
      reject(new Error(e.message || 'worker failed'));
    };
    const req: OcctRequest = { url, format, wasm };
    w.postMessage(req);
  });
}
