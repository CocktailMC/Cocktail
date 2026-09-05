import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

export default defineConfig({
  plugins: [react()],
  server: {
    port: 5173,
    proxy: {
      '/api': {
        target: 'http://127.0.0.1:11011',
        changeOrigin: true,
        ws: true,
        // Large jar / world uploads can take a while.
        timeout: 600_000,
        proxyTimeout: 600_000,
      },
    },
  },
})
