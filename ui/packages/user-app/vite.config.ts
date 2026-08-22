import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'
import path from 'node:path'

export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
    },
  },
  server: {
    port: 5173,
    proxy: {
      '/api': {
        target: 'http://localhost:8080',
        // Preserve the browser-facing Host so the gateway's CSRF origin check
        // accepts cookie-authenticated development requests.
        changeOrigin: false,
        ws: true,
      },
      '/v1': {
        target: 'http://localhost:8080',
        changeOrigin: false,
      },
      '/health': {
        target: 'http://localhost:8080',
        changeOrigin: false,
      },
      '/tokenize': {
        target: 'http://localhost:8080',
        changeOrigin: false,
      },
      '/detokenize': {
        target: 'http://localhost:8080',
        changeOrigin: false,
      },
    },
  },
  preview: { port: 5173 },
  build: { outDir: 'dist' },
})
