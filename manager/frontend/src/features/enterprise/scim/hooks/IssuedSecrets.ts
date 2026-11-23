import type { QueryClient } from '@tanstack/react-query';
import type { IssuedResponse } from '../types';

type Listener = () => void;

const stores = new WeakMap<QueryClient, IssuedSecrets>();

export class IssuedSecrets {
  private readonly issued = new Map<string, IssuedResponse>();
  private readonly listeners = new Set<Listener>();

  static of(queryClient: QueryClient): IssuedSecrets {
    let store = stores.get(queryClient);
    if (!store) {
      store = new IssuedSecrets();
      stores.set(queryClient, store);
    }
    return store;
  }

  get(organizationId: string): IssuedResponse | null {
    return this.issued.get(organizationId) ?? null;
  }

  keep(organizationId: string, issued: IssuedResponse): void {
    this.issued.set(organizationId, issued);
    this.notify();
  }

  forget(organizationId: string): void {
    if (this.issued.delete(organizationId)) {
      this.notify();
    }
  }

  subscribe(listener: Listener): () => void {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  }

  private notify(): void {
    for (const listener of this.listeners) {
      listener();
    }
  }
}
