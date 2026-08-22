import react from '@vitejs/plugin-react';
import { fileURLToPath, URL } from 'node:url';
import { defineConfig } from 'vite';

export default defineConfig({
  plugins: [react()],
  build: {
    target: 'esnext',
  },
  server: {
    fs: {
      allow: [fileURLToPath(new URL('../..', import.meta.url))],
    },
  },
  resolve: {
    alias: {
      formualizer: fileURLToPath(new URL('./src/formualizer-web.ts', import.meta.url)),
      'formualizer-wasm-init': fileURLToPath(
        new URL('../../bindings/wasm/pkg/formualizer_wasm.js', import.meta.url),
      ),
    },
  },
});
