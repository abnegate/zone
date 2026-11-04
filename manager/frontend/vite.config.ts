import { readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { createServer, defineConfig, type Plugin } from 'vite';
import react from '@vitejs/plugin-react';
import tailwind from '@tailwindcss/vite';
import { VitePWA } from 'vite-plugin-pwa';

import { SERVICE_WORKER_FILE } from './src/shared/serviceWorker';
import { FEATURE_STYLESHEET_ATTRIBUTE } from './src/shared/stylesheet';

const frontendRoot = fileURLToPath(new URL('.', import.meta.url));
const uiSrc = path.resolve(frontendRoot, '../../packages/ui/src');

const sourceAliases = {
  '@zone/ui/styles/globals.css': path.join(uiSrc, 'styles/globals.css'),
  '@zone/ui/styles/variables': path.join(uiSrc, 'styles/variables.css'),
  '@zone/ui/styles': path.join(uiSrc, 'styles/index.css'),
  '@zone/ui': path.join(uiSrc, 'index.ts'),
};

const SHARED_STYLESHEET = 'styles-shared';
const SIGNED_OUT_STYLESHEET = 'styles-signed-out';
const FEATURE_STYLESHEET = 'styles-features';
const STYLESHEETS = [SIGNED_OUT_STYLESHEET, FEATURE_STYLESHEET, SHARED_STYLESHEET];
const SHELL_MARKER = '<!--signed-out-shell-->';
const APP_CHUNK = 'app';
const RENDERER_CHUNK = 'renderer';
const EAGER_CHUNKS = [APP_CHUNK, RENDERER_CHUNK];
const EAGER_COMPILE_HINT = '//# allFunctionsCalledOnLoad';

function compileEagerly(): Plugin {
  return {
    name: 'zone:compile-eagerly',
    apply: 'build',
    enforce: 'post',
    writeBundle: async (options, bundle) => {
      const chunks = Object.values(bundle).filter(
        (output) => output.type === 'chunk' && EAGER_CHUNKS.includes(output.name)
      );
      if (chunks.length !== EAGER_CHUNKS.length) {
        throw new Error(`Found ${chunks.length} of the chunks ${EAGER_CHUNKS.join(', ')}`);
      }
      for (const chunk of chunks) {
        const file = path.join(options.dir ?? '', chunk.fileName);
        await writeFile(file, `${EAGER_COMPILE_HINT}\n${await readFile(file, 'utf8')}`);
        const map = JSON.parse(await readFile(`${file}.map`, 'utf8')) as { mappings: string };
        map.mappings = `;${map.mappings}`;
        await writeFile(`${file}.map`, JSON.stringify(map));
      }
    },
  };
}

function deferFeatureStylesheet(): Plugin {
  return {
    name: 'zone:defer-feature-stylesheet',
    apply: 'build',
    transformIndexHtml: {
      order: 'post',
      handler: (html) => {
        const found: string[] = [];
        const names = STYLESHEETS.join('|');
        const output = html.replace(
          new RegExp(`<link rel="stylesheet" crossorigin href="(/assets/(${names})-[^"/]+\\.css)">`, 'g'),
          (link, href: string, stylesheet: string) => {
            found.push(stylesheet);
            return stylesheet === FEATURE_STYLESHEET
              ? `<link rel="stylesheet" crossorigin ${FEATURE_STYLESHEET_ATTRIBUTE}="${href}">`
              : link;
          }
        );
        const links = html.match(/<link rel="stylesheet"/g)?.length ?? 0;
        if (found.join() !== STYLESHEETS.join() || links !== STYLESHEETS.length) {
          throw new Error(
            `Found ${links} stylesheets (${found.join(', ')}), expected ${STYLESHEETS.join(', ')}`
          );
        }
        return output;
      },
    },
  };
}

function signedOutShell(): Plugin {
  return {
    name: 'zone:signed-out-shell',
    apply: 'build',
    transformIndexHtml: {
      order: 'post',
      handler: async (html) => {
        if (!html.includes(SHELL_MARKER)) {
          throw new Error('index.html has no signed-out shell marker');
        }
        const server = await createServer({
          configFile: false,
          root: frontendRoot,
          logLevel: 'error',
          appType: 'custom',
          resolve: { alias: sourceAliases },
          server: { middlewareMode: true, hmr: false, ws: false },
        });
        try {
          const { prerender } = (await server.ssrLoadModule('/src/shell/prerender.tsx')) as {
            prerender: () => string;
          };
          return html.replace(SHELL_MARKER, prerender());
        } finally {
          await server.close();
        }
      },
    },
  };
}

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
      deferFeatureStylesheet(),
      signedOutShell(),
      compileEagerly(),
      VitePWA({
        disable: process.env.VITE_DISABLE_PWA === '1',
        registerType: 'autoUpdate',
        injectRegister: false,
        filename: SERVICE_WORKER_FILE,
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
        ? { alias: sourceAliases }
        : {
            alias: [{ find: /^@zone\/ui$/, replacement: path.join(uiSrc, 'index.ts') }],
          },
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
      rolldownOptions: {
        output: {
          codeSplitting: {
            groups: [
              { name: 'runtime', test: /preload-helper|rolldown:runtime|staleChunk|\/src\/routes\.ts|\/api\/session\.ts|\/shared\/stylesheet\.ts|\/shared\/lastPath\.ts|\/shell\/(paint|release)\.ts/, priority: 4 },
              {
                name: SHARED_STYLESHEET,
                test: /(\/src\/App\.css|\/styles\/globals\.css)($|\?)/,
                priority: 3,
              },
              {
                name: SIGNED_OUT_STYLESHEET,
                test: /(\/ZoneLogo\.css|\/pages\/AuthPage\.css|\/ResendVerificationButton\.css|\/VerificationPendingBanner\.css|\/InvitationAcceptPage\.css)($|\?)/,
                priority: 3,
              },
              { name: FEATURE_STYLESHEET, test: /\.css($|\?)/, priority: 2 },
              {
                name: RENDERER_CHUNK,
                test: /\/src\/renderer\.ts$|\/node_modules\/(\.bun\/[^/]+\/node_modules\/)?(react-dom\/(client\.js|cjs\/react-dom-client\.)|scheduler\/)/,
                priority: 2,
              },
              {
                name: 'react',
                test: /\/node_modules\/(\.bun\/[^/]+\/node_modules\/)?(react\/|react-dom\/(index\.js|cjs\/react-dom\.))/,
                priority: 3,
              },
              { name: APP_CHUNK, test: /\/src\/main\.tsx$/, priority: 1 },
            ],
          },
        },
      },
    },
  };
});
