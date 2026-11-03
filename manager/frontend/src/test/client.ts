import type * as ClientModule from '../api/client';

export function clientModule(client: object): Record<keyof typeof ClientModule, unknown> {
  return {
    API_BASE: import.meta.env.VITE_API_URL || '',
    client,
  };
}
