import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

// Tauri serves the dev server at a fixed port (tauri.conf.json devUrl).
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: { port: 5190, strictPort: true },
  build: { target: 'es2022', outDir: 'dist', emptyOutDir: true },
});
