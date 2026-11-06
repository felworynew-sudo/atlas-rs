import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

// Запросы /api уходят на Rust-сервер (по умолчанию http://127.0.0.1:8787; BIND=127.0.0.1:8787 у сервера).
const api = process.env.ATLAS_API ?? 'http://127.0.0.1:8787';

export default defineConfig({
  plugins: [react()],
  server: { port: 5173, proxy: { '/api': { target: api, changeOrigin: true } } },
  test: { environment: 'node' },
});
