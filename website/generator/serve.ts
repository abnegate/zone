import { existsSync, statSync } from 'node:fs';
import { join, normalize, sep } from 'node:path';
import { build, defaultBaseURL, outputDirectory } from './build.ts';

const notFoundPage = join(outputDirectory, '404.html');

function isDirectory(path: string): boolean {
  return existsSync(path) && statSync(path).isDirectory();
}

function isFile(path: string): boolean {
  return existsSync(path) && statSync(path).isFile();
}

export function respond(request: Request): Response {
  const url = new URL(request.url);
  let pathname: string;
  try {
    pathname = decodeURIComponent(url.pathname);
  } catch {
    return new Response('Bad request', { status: 400 });
  }
  const candidate = normalize(join(outputDirectory, pathname));
  if (candidate !== outputDirectory && !candidate.startsWith(outputDirectory + sep)) {
    return new Response('Forbidden', { status: 403 });
  }
  if (isDirectory(candidate)) {
    if (!pathname.endsWith('/')) {
      url.pathname = `${url.pathname}/`;
      return Response.redirect(url.href, 301);
    }
    return respondWithFile(join(candidate, 'index.html'));
  }
  return respondWithFile(candidate);
}

function respondWithFile(path: string): Response {
  if (isFile(path)) {
    return new Response(Bun.file(path));
  }
  return new Response(Bun.file(notFoundPage), {
    status: 404,
    headers: { 'content-type': 'text/html; charset=utf-8' },
  });
}

if (import.meta.main) {
  build(defaultBaseURL());
  const server = Bun.serve({
    hostname: '127.0.0.1',
    port: Number(process.env.PORT || 4321),
    fetch: respond,
  });
  console.log(`Serving website/dist at ${server.url}`);
}
