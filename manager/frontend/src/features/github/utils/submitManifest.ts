import { requireWebUrl } from './webUrl';

export function submitManifest(
  registration: { action: string; manifest: string },
  target: Document = document
): void {
  const action = requireWebUrl(registration.action);
  const form = target.createElement('form');
  form.method = 'post';
  form.action = action;
  form.hidden = true;

  const input = target.createElement('input');
  input.type = 'hidden';
  input.name = 'manifest';
  input.value = registration.manifest;

  form.appendChild(input);
  target.body.appendChild(form);
  form.submit();
}
