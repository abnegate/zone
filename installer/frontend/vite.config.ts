import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import tailwind from '@tailwindcss/vite';

const asyncCssPlugin = () => ({
  name: 'async-css',
  apply: 'build',
  transformIndexHtml(html: string) {
    return html.replace(
      /<link\s+rel="stylesheet"[^>]*href="(\/assets\/[^"]+\.css)"[^>]*>/g,
      (match, href) => {
        const crossorigin = match.includes('crossorigin') ? ' crossorigin' : '';
        return `<link rel="preload" href="${href}" as="style"${crossorigin} onload="this.onload=null;this.rel='stylesheet'"><noscript><link rel="stylesheet" href="${href}"${crossorigin}></noscript>`;
      }
    );
  },
});

export default defineConfig({
  plugins: [react(), tailwind(), asyncCssPlugin()],
  server: {
    port: 3000,
    open: true,
  },
  build: {
    outDir: 'build',
    sourcemap: true,
  },
});
