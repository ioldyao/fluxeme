import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'
import path from 'node:path'

export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      '@': path.resolve(import.meta.dirname, './src'),
      '@shared': path.resolve(import.meta.dirname, '../shared/src'),
      // Vite string aliases only match exact specifiers; add an explicit
      // trailing-slash rule so subpaths like '@shared/api/client' resolve too.
      '@shared/': path.resolve(import.meta.dirname, '../shared/src') + '/',
    },
  },
  server: {
    port: 5173,
    // Dev-mode proxy kept as a convenience fallback; production/preview
    // relies on VITE_API_BASE_URL instead (see src/api client in shared).
    proxy: {
      '/api': { target: 'http://localhost:8080', changeOrigin: true, ws: true },
      '/v1': { target: 'http://localhost:8080', changeOrigin: true },
      '/health': { target: 'http://localhost:8080', changeOrigin: true },
      '/tokenize': { target: 'http://localhost:8080', changeOrigin: true },
      '/detokenize': { target: 'http://localhost:8080', changeOrigin: true },
    },
  },
  preview: {
    port: 5173,
  },
  build: {
    outDir: 'dist',
    emptyOutDir: true,
  },
})
