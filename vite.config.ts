import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
export default defineConfig({
  plugins: [react()],
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
