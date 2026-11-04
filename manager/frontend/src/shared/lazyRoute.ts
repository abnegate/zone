import { type ComponentType, createElement, lazy, useState } from 'react';
import { preload, preloaded } from '../routes';

export function lazyRoute<Props extends object>(
  load: () => Promise<{ default: ComponentType<Props> }>
): ComponentType<Props> {
  const Lazy = lazy(() => preload(load)) as unknown as ComponentType<Props>;
  return function Route(props: Props) {
    const [Resolved] = useState<ComponentType<Props>>(() => preloaded(load)?.default ?? Lazy);
    return createElement(Resolved, props);
  };
}
