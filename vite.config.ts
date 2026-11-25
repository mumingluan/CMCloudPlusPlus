import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import { cpSync, mkdirSync } from 'node:fs';
import { createRequire } from 'node:module';
import { dirname, join } from 'node:path';

const pdfRoot = dirname(createRequire(import.meta.url).resolve('pdfjs-dist/package.json'));
export default defineConfig({
  plugins: [react(), {
    name: 'cmcloud-pdf-assets',
    apply: 'build',
    writeBundle(options) {
      if (!options.dir) return;
      const target = join(options.dir, 'pdf-assets');
      mkdirSync(target, { recursive: true });
      for (const name of ['cmaps', 'standard_fonts', 'wasm']) {
        cpSync(join(pdfRoot, name), join(target, name), { recursive: true });
      }
    },
  }],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: '0.0.0.0',
    watch: {
      ignored: ['**/src-tauri/**', '**/.impeccable/**', '**/.build-tmp/**', '**/releases/**'],
    },
  },
  envPrefix: ['VITE_', 'TAURI_ENV_*'],
  build: { target: ['es2022', 'chrome105', 'safari13'] },
});
