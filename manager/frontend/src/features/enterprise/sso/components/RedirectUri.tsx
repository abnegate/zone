import { CopyValue } from './CopyValue';

interface RedirectUriProps {
  uri: string;
}

export function RedirectUri({ uri }: RedirectUriProps) {
  return (
    <div className="settings-card sso-redirect">
      <h3 className="sso-card-title">Redirect URI</h3>
      <CopyValue value={uri} name="redirect URI" />
      <p className="sso-hint">
        Add this as the sign-in redirect URI when you register Zone with an OpenID Connect provider.
      </p>
    </div>
  );
}
