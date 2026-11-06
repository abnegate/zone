export function submitManifest(
  registration: { action: string; manifest: string },
  target: Document = document
): void {
  const form = target.createElement('form');
  form.method = 'post';
  form.action = registration.action;
  form.hidden = true;

  const input = target.createElement('input');
  input.type = 'hidden';
  input.name = 'manifest';
  input.value = registration.manifest;

  form.appendChild(input);
  target.body.appendChild(form);
  form.submit();
}
