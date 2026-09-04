import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import tailwind from '@tailwindcss/vite';
import { VitePWA } from 'vite-plugin-pwa';

const frontendRoot = fileURLToPath(new URL('.', import.meta.url));
const uiSrc = path.resolve(frontendRoot, '../../packages/ui/src');

export default defineConfig(({ command }) => {
  const inDocker = Boolean(process.env.VITE_PROXY_TARGET);
  const usePolling =
    process.env.VITE_USE_POLLING === 'true' ||
    process.env.CHOKIDAR_USEPOLLING === 'true' ||
    inDocker;
  const hmrHost = process.env.VITE_HMR_HOST;

  return {
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
    resolve:
      command === 'serve'
        ? {
            alias: {
              '@zone/ui/styles/globals.css': path.join(uiSrc, 'styles/globals.css'),
              '@zone/ui/styles/variables': path.join(uiSrc, 'styles/variables.css'),
              '@zone/ui/styles': path.join(uiSrc, 'styles/index.css'),
              '@zone/ui': path.join(uiSrc, 'index.ts'),
            },
          }
        : undefined,
    optimizeDeps: {
      exclude: ['@zone/ui'],
    },
    server: {
      port: 3001,
      strictPort: true,
      host: true,
      allowedHosts: true,
      open: !inDocker,
      // Default HMR follows the page host (localhost:3001 or Traefik).
      // Set VITE_HMR_HOST to force a client target.
      hmr: hmrHost
        ? {
            host: hmrHost,
            clientPort: Number(process.env.VITE_HMR_CLIENT_PORT || 3001),
            protocol: (process.env.VITE_HMR_PROTOCOL || 'ws') as 'ws' | 'wss',
          }
        : true,
      watch: {
        usePolling,
        interval: Number(process.env.VITE_POLL_INTERVAL || 300),
      },
      fs: {
        allow: [frontendRoot, uiSrc],
      },
      proxy: {
        '/api': {
          target: process.env.VITE_PROXY_TARGET || 'http://localhost:8000',
          changeOrigin: true,
        },
        '/ws': {
          target: process.env.VITE_PROXY_TARGET || 'http://localhost:8000',
          changeOrigin: true,
          ws: true,
        },
      },
    },
    build: {
      outDir: 'build',
      sourcemap: true,
    },
  };
});
