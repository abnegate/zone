const root = `${import.meta.dir}/build`;
const port = Number(process.env.PORT ?? 3001);

function pathFrom(request: Request): string {
  const url = new URL(request.url);
  let pathname = decodeURIComponent(url.pathname);
  if (pathname.endsWith('/')) pathname += 'index.html';
  const relative = pathname.replace(/^\/+/, '');
  if (!relative || relative.split('/').includes('..')) return 'index.html';
  return relative;
}

Bun.serve({
  port,
  hostname: '0.0.0.0',
  async fetch(request) {
    const relative = pathFrom(request);
    const file = Bun.file(`${root}/${relative}`);
    if (await file.exists()) {
      return new Response(file);
    }
    return new Response(Bun.file(`${root}/index.html`));
  },
});

console.log(`console listening on ${port}`);
