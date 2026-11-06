type Listener = (event: unknown) => void;

class Listeners {
  private readonly listeners = new Map<string, Listener[]>();

  addEventListener(type: string, listener: Listener): void {
    this.listeners.set(type, [...(this.listeners.get(type) ?? []), listener]);
  }

  emit(type: string, event: unknown = { type }): void {
    for (const listener of this.listeners.get(type) ?? []) listener(event);
  }
}

export class FakeXMLHttpRequest extends Listeners {
  static requests: FakeXMLHttpRequest[] = [];

  readonly upload = new Listeners();
  method = '';
  url = '';
  headers: Record<string, string> = {};
  body: unknown;
  responseType = '';
  status = 0;
  statusText = '';
  response: Blob | null = null;
  aborted = false;
  private rawHeaders = '';

  open(method: string, url: string): void {
    this.method = method;
    this.url = url;
  }

  setRequestHeader(name: string, value: string): void {
    this.headers[name.toLowerCase()] = value;
  }

  getAllResponseHeaders(): string {
    return this.rawHeaders;
  }

  send(body: unknown): void {
    this.body = body;
    FakeXMLHttpRequest.requests.push(this);
  }

  abort(): void {
    this.aborted = true;
    this.emit('abort');
  }

  progress(loaded: number, total: number): void {
    this.upload.emit('progress', { lengthComputable: true, loaded, total });
  }

  respond(status: number, body: unknown, headers: Record<string, string> = {}): void {
    this.status = status;
    this.statusText = status < 400 ? 'OK' : 'Error';
    this.response = new Blob([JSON.stringify(body)], { type: 'application/json' });
    this.rawHeaders = Object.entries({ 'content-type': 'application/json', ...headers })
      .map(([name, value]) => `${name}: ${value}`)
      .join('\r\n');
    this.emit('load');
  }

  fail(): void {
    this.emit('error');
  }
}

export function installFakeXhr(): { requests: FakeXMLHttpRequest[]; restore: () => void } {
  const original = globalThis.XMLHttpRequest;
  FakeXMLHttpRequest.requests = [];
  globalThis.XMLHttpRequest = FakeXMLHttpRequest as unknown as typeof XMLHttpRequest;
  return {
    requests: FakeXMLHttpRequest.requests,
    restore: () => {
      globalThis.XMLHttpRequest = original;
    },
  };
}
