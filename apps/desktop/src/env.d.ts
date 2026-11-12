/// <reference types="vite/client" />

/** The app version from package.json (vite.config.ts `define`). */
declare const __APP_VERSION__: string;

/** OpenCascade's import functions in WebAssembly (no types of its own). */
declare module 'occt-import-js' {
  interface OcctResult {
    success: boolean;
    root: unknown;
    meshes: unknown[];
  }
  interface Occt {
    ReadStepFile(content: Uint8Array, params: object | null): OcctResult;
    ReadIgesFile(content: Uint8Array, params: object | null): OcctResult;
    ReadBrepFile(content: Uint8Array, params: object | null): OcctResult;
  }
  export default function occtimportjs(options?: { wasmBinary?: ArrayBuffer; locateFile?: (name: string) => string }): Promise<Occt>;
}
