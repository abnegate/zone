import { Button } from '@zone/ui';
import { useId, useState } from 'react';
import { includes } from '../../plans';
import { LicenseStatus, Plan } from '../../schemas';
import { useSso } from '../hooks';
import type { ProviderResponse, SettingsResponse } from '../types';
import { DeleteSsoProviderDialog } from './DeleteSsoProviderDialog';
import { RedirectUri } from './RedirectUri';
import { SsoEnforcement } from './SsoEnforcement';
import { SsoLapsedNotice } from './SsoLapsedNotice';
import { SsoProviderForm } from './SsoProviderForm';
import { SsoProviderList } from './SsoProviderList';
import { SsoUpgradeNotice } from './SsoUpgradeNotice';
import './SsoSection.css';

function lapsed(settings: SettingsResponse): boolean {
  return (
    !settings.entitled &&
    settings.status === LicenseStatus.Lapsed &&
    includes(settings.edition, Plan.Enterprise)
  );
}

function SsoSettings({ settings }: { settings: SettingsResponse }) {
  const [adding, setAdding] = useState(false);
  const [editing, setEditing] = useState<ProviderResponse | null>(null);
  const [deleting, setDeleting] = useState<ProviderResponse | null>(null);
  const { entitled, providers } = settings;

  return (
    <>
      {!entitled && (lapsed(settings) ? <SsoLapsedNotice /> : <SsoUpgradeNotice />)}
      {entitled && <RedirectUri uri={settings.redirect_uri} />}
      <SsoEnforcement settings={settings} editable={entitled} />
      {(entitled || providers.length > 0) && (
        <SsoProviderList
          providers={providers}
          editable={entitled}
          onAdd={() => setAdding(true)}
          onEdit={setEditing}
          onDelete={setDeleting}
        />
      )}
      {adding && <SsoProviderForm onClose={() => setAdding(false)} />}
      {editing && (
        <SsoProviderForm key={editing.id} provider={editing} onClose={() => setEditing(null)} />
      )}
      {deleting && (
        <DeleteSsoProviderDialog provider={deleting} onClose={() => setDeleting(null)} />
      )}
    </>
  );
}

export function SsoSection() {
  const headingId = useId();
  const { settings, loading, error, refresh } = useSso();

  if (loading) {
    return <div className="loading-state">Loading single sign-on...</div>;
  }

  return (
    <section className="sso-section" aria-labelledby={headingId}>
      <div className="section-row">
        <div className="section-row-copy">
          <h2 id={headingId} className="section-title">
            Single sign-on
          </h2>
          <p className="section-description">
            Let people sign in to this server with your identity provider.
          </p>
        </div>
      </div>
      {error && (
        <div className="alert alert-error alert-inline" role="alert">
          <span>{error}</span>
          <Button type="button" variant="ghost" size="sm" onClick={() => void refresh()}>
            Retry
          </Button>
        </div>
      )}
      {settings && <SsoSettings settings={settings} />}
    </section>
  );
}
