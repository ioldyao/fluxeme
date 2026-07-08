import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import path from 'node:path'

export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
    },
  },
  build: {
    outDir: '../web',
    emptyOutDir: true,
  },
  server: {
    port: 5173,
    proxy: {
      '/admin/api': {
        target: 'http://localhost:8080',
        changeOrigin: true,
      },
      '/v1': {
        target: 'http://localhost:8080',
        changeOrigin: true,
      },
      '/health': {
        target: 'http://localhost:8080',
        changeOrigin: true,
      },
      '/tokenize': {
        target: 'http://localhost:8080',
        changeOrigin: true,
      },
      '/detokenize': {
        target: 'http://localhost:8080',
        changeOrigin: true,
      },
    },
  },
})
