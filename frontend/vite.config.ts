import { fileURLToPath, URL } from 'node:url'
import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'

export default defineConfig({
  plugins: [vue()],
  resolve: {
    alias: {
      '@': fileURLToPath(new URL('./src', import.meta.url)),
    },
  },
  build: {
    outDir: '../backend/crates/app/frontend-dist',
    emptyOutDir: true,
  },
  server: {
    port: 5173,
    proxy: {
      '/api': { target: 'http://localhost:8097', changeOrigin: true },
      '/mcp': { target: 'http://localhost:8097', changeOrigin: true },
      '/uploads': { target: 'http://localhost:8097', changeOrigin: true },
    },
  },
})
