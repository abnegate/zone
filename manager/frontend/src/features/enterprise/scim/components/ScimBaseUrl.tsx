import { BASE_URL_HINT, BASE_URL_NAME, BASE_URL_TITLE } from '../labels';
import { CopyValue } from './CopyValue';

interface ScimBaseUrlProps {
  url: string;
}

export function ScimBaseUrl({ url }: ScimBaseUrlProps) {
  return (
    <div className="settings-card scim-card">
      <h3 className="scim-card-title">{BASE_URL_TITLE}</h3>
      <CopyValue value={url} name={BASE_URL_NAME} />
      <p className="scim-hint">{BASE_URL_HINT}</p>
    </div>
  );
}
