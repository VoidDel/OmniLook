import { readFileSync } from 'node:fs';
import { fileURLToPath, URL } from 'node:url';
import react from '@vitejs/plugin-react';
import { defineConfig, type Plugin } from 'vite';

const pkg = JSON.parse(readFileSync(new URL('./package.json', import.meta.url), 'utf8')) as { version: string };

// occt-wasm's glue names its WebAssembly by URL (`new URL('occt-wasm.wasm', import.meta.url)`),
// which would build its 21 MB into the program. The page always says where the file is (installed
// beside the program, replaceable: LGPL), so the reference goes.
function occtWasmOutside(): Plugin {
  return {
    name: 'omnilook-occt-wasm-outside',
    transform(code, id) {
      if (!id.replace(/\\/g, '/').includes('/occt-wasm/dist/occt-wasm.js')) return null;
      return code.replace(/new URL\(\s*["']occt-wasm\.wasm["']\s*,\s*import\.meta\.url\s*\)/g, '"occt-wasm.wasm"');
    },
  };
}

// Tauri expects a fixed port (1440: OmniZip uses 1420, Omni Download 1430) and no clearing of its
// output.
export default defineConfig({
  plugins: [react(), occtWasmOutside()],
  // The Open CASCADE worker is built on its own: it needs the same.
  worker: { plugins: () => [occtWasmOutside()] },
  clearScreen: false,
  define: {
    __APP_VERSION__: JSON.stringify(pkg.version),
  },
  resolve: {
    alias: {
      '@icons': fileURLToPath(new URL('../../icons', import.meta.url)),
      '@locales': fileURLToPath(new URL('../../locales', import.meta.url)),
    },
  },
  server: {
    port: 1440,
    strictPort: true,
    fs: { allow: ['..', '../..'] },
    watch: { ignored: ['**/src-tauri/**'] },
  },
  envPrefix: ['VITE_', 'TAURI_ENV_'],
  build: {
    target: 'es2022',
    minify: 'oxc',
    sourcemap: false,
    chunkSizeWarningLimit: 1500,
  },
  test: {
    environment: 'node',
    include: ['src/**/*.test.ts'],
  },
});
