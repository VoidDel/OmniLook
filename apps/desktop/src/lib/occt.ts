// What the Open CASCADE worker (occt.worker.ts) is asked and answers, and the asking.

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
