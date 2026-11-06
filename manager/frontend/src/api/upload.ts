export type UploadProgress = {
  loaded: number;
  total: number;
};

export type UploadInit = {
  method?: string;
  headers?: HeadersInit;
  body: XMLHttpRequestBodyInit;
  signal?: AbortSignal;
  onProgress?: (progress: UploadProgress) => void;
};

export function cancelled(): DOMException {
  return new DOMException('The upload was cancelled', 'AbortError');
}

export function isCancelled(error: unknown): boolean {
  return error instanceof DOMException && error.name === 'AbortError';
}

function responseHeaders(raw: string): Headers {
  const headers = new Headers();
  for (const line of raw.trim().split(/[\r\n]+/)) {
    const separator = line.indexOf(':');
    if (separator <= 0) continue;
    headers.append(line.slice(0, separator).trim(), line.slice(separator + 1).trim());
  }
  return headers;
}

function bodyless(status: number): boolean {
  return status === 204 || status === 205 || status === 304;
}

export function upload(url: string, init: UploadInit): Promise<Response> {
  const { method = 'POST', headers, body, signal, onProgress } = init;
  return new Promise((resolve, reject) => {
    if (signal?.aborted) {
      reject(cancelled());
      return;
    }
    const request = new XMLHttpRequest();
    request.open(method, url);
    request.responseType = 'blob';
    new Headers(headers).forEach((value, name) => {
      request.setRequestHeader(name, value);
    });

    const abort = () => request.abort();
    const settle = () => signal?.removeEventListener('abort', abort);
    signal?.addEventListener('abort', abort, { once: true });

    request.upload.addEventListener('progress', (event) => {
      if (!event.lengthComputable) return;
      onProgress?.({ loaded: event.loaded, total: event.total });
    });
    request.addEventListener('load', () => {
      settle();
      if (request.status === 0) {
        reject(new TypeError('The upload could not reach the server'));
        return;
      }
      resolve(
        new Response(bodyless(request.status) ? null : request.response, {
          status: request.status,
          statusText: request.statusText,
          headers: responseHeaders(request.getAllResponseHeaders()),
        })
      );
    });
    request.addEventListener('error', () => {
      settle();
      reject(new TypeError('The upload could not reach the server'));
    });
    request.addEventListener('timeout', () => {
      settle();
      reject(new TypeError('The upload timed out'));
    });
    request.addEventListener('abort', () => {
      settle();
      reject(cancelled());
    });
    request.send(body);
  });
}
