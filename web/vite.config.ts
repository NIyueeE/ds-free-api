import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'
import path from 'path'

export default defineConfig({
  base: '/admin/',
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
    },
  },
  server: {
    proxy: {
      '/admin/api': 'http://127.0.0.1:5317',
      '/v1': 'http://127.0.0.1:5317',
      '/anthropic': 'http://127.0.0.1:5317',
    },
  },
})
