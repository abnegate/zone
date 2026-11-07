import { requireWebUrl } from './webUrl';

export function redirectTo(url: string): void {
  window.location.assign(requireWebUrl(url));
}
