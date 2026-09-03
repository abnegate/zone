import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import tailwind from '@tailwindcss/vite';
import { VitePWA } from 'vite-plugin-pwa';

export default defineConfig({
  plugins: [
    react(),
    tailwind(),
    VitePWA({
      registerType: 'autoUpdate',
      includeAssets: ['favicon.ico', 'logo192.png', 'logo512.png'],
      manifest: {
        name: 'Zone Dashboard',
        short_name: 'Zone',
        description: 'Zone Dashboard - AI Infrastructure Management',
        theme_color: '#1a1612',
        background_color: '#1a1612',
        icons: [
          {
            src: 'logo192.png',
            sizes: '192x192',
            type: 'image/png',
          },
          {
            src: 'logo512.png',
            sizes: '512x512',
            type: 'image/png',
          },
        ],
      },
    }),
  ],
  server: {
    port: 3001,
    host: true, // Listen on all interfaces (required for Docker)
    open: !process.env.VITE_API_URL, // Don't open browser in Docker container
    // HMR configuration for Docker
    hmr: {
      host: 'localhost',
      port: 3001,
      protocol: 'ws',
    },
    // Watch configuration for Docker volumes
    watch: {
      usePolling: true, // Required for Docker volume mounts
      interval: 1000, // Poll every second
    },
    proxy: {
      '/api': {
        target: process.env.VITE_PROXY_TARGET || 'http://localhost:8000',
        changeOrigin: true,
      },
      '/ws': {
        target: process.env.VITE_PROXY_TARGET || 'http://localhost:8000',
        changeOrigin: true,
        ws: true, // Enable WebSocket proxying
      },
    },
  },
  build: {
    outDir: 'build',
    sourcemap: true,
  },
});
