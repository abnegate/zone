import { Navigate, useLocation } from 'react-router-dom';
import { legacyOrganizationPath, relocate, SESSIONS_PATH, SOURCES_PATH } from './legacy';

export const LegacySettings = {
  Organization: 'organization',
  Sessions: 'sessions',
  Sources: 'sources',
} as const;

export type LegacySettings = (typeof LegacySettings)[keyof typeof LegacySettings];

function target(from: LegacySettings, search: string, hash: string): string {
  switch (from) {
    case LegacySettings.Organization:
      return legacyOrganizationPath(search, hash);
    case LegacySettings.Sessions:
      return relocate(SESSIONS_PATH, search, hash);
    case LegacySettings.Sources:
      return relocate(SOURCES_PATH, search, hash);
  }
}

export default function SettingsRedirect({ from }: { from: LegacySettings }) {
  const location = useLocation();
  return <Navigate to={target(from, location.search, location.hash)} replace />;
}
